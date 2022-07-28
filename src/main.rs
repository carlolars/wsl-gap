use simplelog::*;
use std::env;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::{BufRead, Error, ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use structopt::StructOpt;

#[macro_use]
extern crate log;
extern crate dirs;
extern crate simplelog;
extern crate winapi;

type Hwnd = winapi::shared::windef::HWND;

/// The length of a nonce in bytes.
const NONCE_LENGTH: usize = 16;

#[derive(StructOpt)]
struct Cli {
    /// Path to a socket on the Windows side if not using the default
    #[structopt(parse(from_os_str))]
    socket_path: Option<PathBuf>,
    /// Run as gpg-agent proxy
    #[structopt(long)]
    gpg: bool,
    /// Run as ssh-pageant proxy
    #[structopt(long)]
    ssh: bool,
    /// Activate debug logging to file in the same directory as the exe.
    #[structopt(short, long)]
    debug: bool,
    /// Show version information.
    #[structopt(long)]
    version: bool,
}

/// The information found in a socket file created by gpg-agent on Windows.
///
/// A socket file created by gpg-agent on Windows is not a UNIX socket, instead
/// it contains information used to connect to a port that the gpg-agent is
/// listening on.
#[derive(Debug)]
struct SocketInfo {
    /// The port number used by the gpg-agent.
    port: u32,
    /// The 128-bit nonce used to authenticate when connecting.
    nonce: [u8; NONCE_LENGTH],
}

fn main() {
    let args = Cli::from_args();

    if args.version {
        print_version_string();
        return;
    }

    if args.debug {
        match init_logging() {
            Ok(_) => (),
            Err(e) => {
                error!("{}", e);
                eprintln!("{}", e);
                return;
            }
        };
    }

    if args.gpg == args.ssh {
        error!("Must provide exactly one of --gpg or --ssh.");
        eprintln!("ERROR: Must provide exactly one of --gpg or --ssh.");
        return;
    }

    let gnupghome = match gnupghome_path() {
        Ok(p) => p,
        Err(e) => {
            error!("{}", e);
            eprintln!("{}", e);
            return;
        }
    };

    let gpg_agent_hwnd = match find_gpg_agent(&gnupghome) {
        Ok(hwnd) => hwnd,
        Err(e) => {
            error!("{}", e);
            eprintln!("{}", e);
            return;
        }
    };

    if args.gpg {
        let socket_path = gpg_socket_path(args.socket_path, gnupghome);
        match gpg_proxy(socket_path) {
            Ok(_) => (),
            Err(e) => {
                error!("{}", e);
                eprintln!("{}", e);
                return;
            }
        }
    } else if args.ssh {
        match ssh_proxy(gpg_agent_hwnd) {
            Ok(_) => (),
            Err(e) => {
                error!("{}", e);
                eprintln!("{}", e);
                return;
            }
        }
    }

    info!("done");
}

fn print_version_string() {
    println!(
        "{} {}-g{}",
        env!("CARGO_PKG_NAME"),
        env!("VERGEN_SEMVER"),
        env!("VERGEN_SHA_SHORT")
    );
}

fn init_logging() -> Result<(), Error> {
    let mut path = env::current_exe()?;
    path.set_extension("log");

    let file = OpenOptions::new().append(true).create(true).open(path)?;
    match WriteLogger::init(LevelFilter::Info, Config::default(), file) {
        Ok(_) => Ok(()),
        Err(e) => Err(Error::new(ErrorKind::Other, e.to_string())),
    }
}

fn gnupghome_path() -> Result<PathBuf, Error> {
    let path = match env::var("GNUPGHOME") {
        // Use path from GNUPGHOME environment variable
        Ok(val) => {
            info!("GNUPGHOME = {}", val);
            PathBuf::from(val)
        }
        Err(_) => match dirs::data_dir() {
            // Use path C:\Users\<user>\AppData\Roaming\gnupg
            Some(mut path) => {
                info!("data_dir() -> {:?}", path);
                path.push("gnupg");
                path
            }
            None => {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    "GNUPGHOME not set and RoamingAppData folder not found.".to_string(),
                ));
            }
        },
    };

    Ok(path)
}

fn find_window(title: &str) -> Option<Hwnd> {
    use std::ffi::CString;
    use winapi::um::winuser::FindWindowA;

    let title_ptr = CString::new(title).expect("CString::new failed").into_raw();
    let hwnd = unsafe { FindWindowA(title_ptr, title_ptr) };
    // retake pointer to free memory
    let _ = unsafe { CString::from_raw(title_ptr) };

    if hwnd.is_null() {
        None
    } else {
        Some(hwnd)
    }
}

fn find_gpg_agent(_gnupghome: &Path) -> Result<Hwnd, Error> {
    info!("find gpg-agent");
    match find_window("Pageant") {
        Some(hwnd) => Ok(hwnd),
        None => {
            info!("not found, trying to start gpg-agent");
            // First remove the files S.gpg-agent* from the gnupghome directory
            // since sometimes when the gpg-agent has not shut down correctly
            // those files prevent it from starting.
            // TODO remove files?

            let _status = Command::new("gpg-connect-agent.exe")
                .arg("--quiet")
                .arg("/bye")
                .status()?;

            info!("gpg-agent started, trying to find again.");

            match find_window("Pageant") {
                Some(hwnd) => Ok(hwnd),
                None => Err(Error::new(ErrorKind::Other, "Failed to start gpg-agent")),
            }
        }
    }
}

fn gpg_socket_path(socket_path: Option<PathBuf>, gnupghome: PathBuf) -> PathBuf {
    match socket_path {
        Some(p) => p,
        None => {
            let mut p = gnupghome;
            p.push("S.gpg-agent");
            p
        }
    }
}

fn gpg_proxy(socket_path: PathBuf) -> Result<(), Error> {
    info!("start gpg-proxy");

    let socket_data = read_socket_file(socket_path)?;
    let socket_info = parse_socket_data(socket_data)?;

    let mut reader = TcpStream::connect(format!("127.0.0.1:{}", socket_info.port))?;

    let mut writer = reader.try_clone()?;

    info!("Authenticating.");
    writer.write_all(&socket_info.nonce)?;

    let child = thread::spawn(move || {
        io::copy(&mut reader, &mut io::stdout()).expect("Copy reader->stdout failed.");
        info!("reader->stdout closed.");
    });

    // When stdin is closed then this copy will close
    io::copy(&mut io::stdin(), &mut writer)?;
    info!("stdin->writer closed.");

    // Shutdown the TcpStream to stop the child thread.
    writer.shutdown(Shutdown::Both)?;

    match child.join() {
        Ok(_) => Ok(()),
        Err(_) => Err(Error::new(ErrorKind::Other, "Failed to join threads.")),
    }
}

fn read_socket_file(path: PathBuf) -> io::Result<Vec<u8>> {
    info!("Reading socket file: {:?}", path);

    match File::open(path) {
        Err(e) => Err(e),
        Ok(mut file) => {
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)?;
            Ok(buffer)
        }
    }
}

fn parse_socket_data(data: Vec<u8>) -> Result<SocketInfo, Error> {
    info!("Parsing socket data.");

    let mut socket_info: SocketInfo = SocketInfo {
        port: 0,
        nonce: [0; NONCE_LENGTH],
    };
    let mut reading_port = true;
    let mut nonce_byte_count = 0;

    for byte in data {
        if reading_port {
            if byte == b'\n' {
                reading_port = false;
            } else {
                // The port number is a string of ASCII characters that should be 0-9
                if !(b'0'..=b'9').contains(&byte) {
                    return Err(Error::new(
                        ErrorKind::InvalidData,
                        "Failed to parse port number in socket file: Unexpected character in port number string",
                    ));
                }

                // Change the byte value range to 0-9
                let value = byte - b'0';
                let value: u32 = value.into();

                if value > 9 {
                    return Err(Error::new(
                        ErrorKind::InvalidData,
                        "Failed to parse port number in socket file: Incorrect decimal character.",
                    ));
                }

                // port is initially zero so 0*10 will remain zero.
                socket_info.port *= 10;
                socket_info.port += value;

                if socket_info.port > 65_535 {
                    return Err(Error::new(
                        ErrorKind::InvalidData,
                        "Failed to parse port number in socket file: Invalid port number.",
                    ));
                }
            }
        } else if nonce_byte_count < NONCE_LENGTH {
            socket_info.nonce[nonce_byte_count] = byte;
            nonce_byte_count += 1;
        } else {
            nonce_byte_count += 1;
        }
    }

    if nonce_byte_count != NONCE_LENGTH {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Failed to read nounce: Incorrect length.",
        ));
    }

    Ok(socket_info)
}

fn ssh_proxy(pageant_hwnd: Hwnd) -> Result<(), Error> {
    info!("start ssh-proxy");

    let stdin = io::stdin();
    let mut handle = stdin.lock();

    loop {
        let request = handle.fill_buf()?;
        let request_length = request.len();
        info!("request read from stdin, length: {}", request_length);
        info!("request: {:?}", request);

        if request_length == 0 {
            break;
        }

        let response = agent_query(request, pageant_hwnd)?;
        info!("response received, length: {}", response.len());
        // info!("response: {:?}", response);

        io::stdout().write_all(response)?;
        let _ = io::stdout().flush();
        info!("response written");

        handle.consume(request_length);
    }

    Ok(())
}

const AGENT_COPYDATA_ID: usize = 0x804e50ba;
const AGENT_MAX_MSGLEN: u32 = 8192;

fn agent_query(request: &[u8], hwnd: Hwnd) -> Result<&[u8], Error> {
    info!("agent_query");

    use std::ffi::CString;
    use std::ptr;
    use std::ptr::null_mut;
    use winapi::shared::minwindef::*;
    use winapi::um::handleapi::{CloseHandle, *};
    use winapi::um::memoryapi::{MapViewOfFile, UnmapViewOfFile, *};
    use winapi::um::winbase::CreateFileMappingA;
    use winapi::um::winnt::*;
    use winapi::um::winuser::{SendMessageA, *};

    // char* mapname = dupprintf("PageantRequest%08x", (unsigned)GetCurrentThreadId());
    let pid = std::process::id();
    let mapname = format!("PageantRequest{:08x}", pid);
    let mapname_len = mapname.len();
    let mapname_ptr = CString::new(mapname)
        .expect("CString::new failed")
        .into_raw();

    // SECURITY_ATTRIBUTES *psa = NULL;
    // HANDLE filemap = CreateFileMapping(INVALID_HANDLE_VALUE, psa, PAGE_READWRITE, 0, AGENT_MAX_MSGLEN, mapname);
    let filemap = unsafe {
        CreateFileMappingA(
            INVALID_HANDLE_VALUE,
            null_mut(),
            PAGE_READWRITE,
            0,
            AGENT_MAX_MSGLEN,
            mapname_ptr,
        )
    } as HANDLE;

    // if (filemap == NULL || filemap == INVALID_HANDLE_VALUE)
    //     return 1;		       /* *out == NULL, so failure */
    // if filemap == null_mut() || filemap == INVALID_HANDLE_VALUE {
    if filemap.is_null() || filemap == INVALID_HANDLE_VALUE {
        return Err(Error::new(
            ErrorKind::Other,
            "Failed to create file mapping.",
        ));
    }

    // unsigned char* p = MapViewOfFile(filemap, FILE_MAP_WRITE, 0, 0, 0);
    let p = unsafe { MapViewOfFile(filemap, FILE_MAP_WRITE, 0, 0, 0) } as *mut u8;
    // memcpy(p, in, inlen);
    let src_len = request.len();
    let src_ptr = request.as_ptr();
    unsafe { ptr::copy_nonoverlapping(src_ptr, p, src_len) };

    // COPYDATASTRUCT cds;
    // cds.dwData = AGENT_COPYDATA_ID;
    // cds.cbData = 1 + strlen(mapname);
    // cds.lpData = mapname;
    let cds = COPYDATASTRUCT {
        dwData: AGENT_COPYDATA_ID,
        cbData: (1 + mapname_len) as DWORD,
        lpData: mapname_ptr as PVOID,
    };
    let cds_ptr = &cds as *const COPYDATASTRUCT as isize;

    // int id = SendMessage(hwnd, WM_COPYDATA, (WPARAM) NULL, (LPARAM) &cds);
    let id = unsafe { SendMessageA(hwnd, WM_COPYDATA, 0, cds_ptr) } as i32;

    // int retlen;
    // if (id > 0) {
    //     retlen = 4 + GET_32BIT(p);
    //     ret = snewn(retlen, unsigned char);
    //     if (ret) {
    //         memcpy(ret, p, retlen);
    //         *out = ret;
    //         *outlen = retlen;
    //     }
    // }
    if id <= 0 {
        return Err(Error::new(
            ErrorKind::Other,
            format!("SendMessageA failed, return value '{}'.", id),
        ));
    }

    let ret = {
        let length_bytes_slice = unsafe { std::slice::from_raw_parts(p, 4) };
        let mut length_bytes: [u8; 4] = [0; 4];
        length_bytes.clone_from_slice(length_bytes_slice);
        let ret_len = (4 + i32::from_be_bytes(length_bytes)) as usize;
        unsafe { std::slice::from_raw_parts(p, ret_len) }
    };

    // retake pointer to free memory
    let _ = unsafe { CString::from_raw(mapname_ptr) };

    // UnmapViewOfFile(p);
    let _ = unsafe { UnmapViewOfFile(filemap) };
    // CloseHandle(filemap);
    let _ = unsafe { CloseHandle(filemap) };

    Ok(ret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnupghome_path_from_env_var() {
        use std::path::Path;
        env::set_var("GNUPGHOME", r"..\tests\.gnupg");
        assert_eq!(
            gnupghome_path().unwrap().as_path(),
            Path::new(r"..\tests\.gnupg")
        );
    }

    #[test]
    fn gnupghome_path_in_roaming_appdata() {
        use std::path::Path;
        env::remove_var("GNUPGHOME");
        let user_profile = env::var("USERPROFILE").unwrap();
        let expected_path = format!("{}\\AppData\\Roaming\\gnupg", user_profile);
        assert_eq!(
            gnupghome_path().unwrap().as_path(),
            Path::new(expected_path.as_str())
        );
    }

    #[test]
    fn get_gpg_socket_path() {
        use std::path::Path;

        let gnupghome = PathBuf::from(r"..\tests\.gnupg-notused");
        let socket_path = Some(PathBuf::from(r"..\tests\.gnupg\S.gpg-agent"));
        assert_eq!(
            gpg_socket_path(socket_path, gnupghome).as_path(),
            Path::new(r"..\tests\.gnupg\S.gpg-agent")
        );

        let gnupghome = PathBuf::from(r"..\tests\.gnupg");
        let socket_path = None;
        assert_eq!(
            gpg_socket_path(socket_path, gnupghome).as_path(),
            Path::new(r"..\tests\.gnupg\S.gpg-agent")
        );
    }

    #[test]
    fn reading_socket_file() {
        let socket_path = PathBuf::from(r"tests\.gnupg\S.gpg-agent");

        let socket_data = read_socket_file(socket_path).unwrap();
        assert_eq!(
            socket_data,
            [
                0x35, 0x36, 0x39, 0x37, 0x34, 0x0A, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
                0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10
            ]
        );
    }

    #[test]
    fn parsing_socket_data() -> Result<(), Error> {
        let socket_data: Vec<u8> = vec![
            0x31, 0x32, 0x33, 0x34, 0x35, 0x0A, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
            0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10,
        ];

        let socket_info = parse_socket_data(socket_data)?;
        assert_eq!(socket_info.port, 12345);
        assert_eq!(
            socket_info.nonce,
            [
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
                0x0F, 0x10
            ]
        );
        Ok(())
    }
}
