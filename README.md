# wsl-gap

`wsl-gap` is a gpg-agent proxy that relays communication between `stdin/stdout` 
and the `gpg-agent` running on Windows.  
Together with `socat` in WSL it is used to forward a socket from WSL 
to the `gpg-agent` running on Windows to be able to access the agents keys from
WSL. 
This makes it possible to use keys from a hardware dongle, like a `Yubikey`, or
a smartcard, which are not directly accessible inside WSL.

`wsl-gap` can be used for both `gpg` and `ssh` and works in both `WSL1` and `WSL2`.

![Connection Diagram](./flowchart.png)

Releases can be verified using:

```sh
minisign -Vm wsl-gap.exe -P RWQnxc7i4M9iJ5oE7m4ry4dchugn3fgsHHdbUHcJOMvf/KSO+t7fxQ/v
```

## Usage
```
$ wsl-gap.exe -h
wsl-gap 1.0.0

USAGE:
    wsl-gap.exe [FLAGS] [socket-path]

FLAGS:
    -d, --debug      Activate debug logging to file in the same directory as the exe
        --gpg        Run as gpg-agent proxy
    -h, --help       Prints help information
        --ssh        Run as ssh-pageant proxy
        --version    Show version information

ARGS:
    <socket-path>    Path to a socket on the Windows side if not using the default
```

* The `gpg-agent` is started if not already running.
* The `GNUPGHOME` environment variable (if set) or `AppData\Roaming\gnupg` will be
  used as path to the `S.gpg-agent` socket on the `Windows` side.
* The GPG and/or SSH sockets are created in `/tmp`.
    * `gpg` v2.1+ expects the `S.gpg-agent` socket to be located in `GNUPGHOME`, 
      so it must be forwarded by creating the file `$GNUPGHOME/S.gpg-agent` with
      the following content:
      ```
      %Assuan%
      socket=/tmp/S.gpg-agent
      ```

### Requirements
* `GnuPG` installed and working **in Windows**, with the path to `gnupg\bin` added
  to the Windows `Path` environment variable.
* `socat` installed **in WSL**.
* `wsl-gap.exe` accessible and **executable from WSL**.

### SSH
To only setup for `ssh`, just export `SSH_AUTH_SOCK` and use `socat` to relay between
the socket and `wsl-gap.exe` with the `--ssh` argument.
```bash
export SSH_AUTH_SOCK=/tmp/S.gpg-agent.ssh

if [ ! -f "$SSH_AUTH_SOCK" ]; then
    (setsid socat UNIX-LISTEN:$SSH_AUTH_SOCK,fork EXEC:"/absolute/path/to/wsl-gap.exe --ssh" &) >/dev/null 2>&1
fi
```

### GnuPG
To use for `gpg`, configure gpg on the WSL side, create the `S.gpg-agent` file
to redirect to the actual socket, and use `socat` to relay between the socket and 
`wsl-gap.exe` with the `--gpg` argument.  
Then import the public keys and it should work.  
**Note!** The `GNUPGHOME` directory cannot be the same on the WSL side as on the Windows
side. Using the default home directory (/home/user) in WSL is a safe way to avoid this.
```bash
export GNUPGHOME=/home/user/.gnupg
GPG_AGENT_SOCK=/tmp/S.gpg-agent

# Create S.gpg-agent redirect file if it doesn't exist.
if [ ! -f "$GNUPGHOME/S.gpg-agent" ]; then
    echo -e "%Assuan%\nsocket=$GPG_AGENT_SOCK" > $GNUPGHOME/S.gpg-agent
fi

if [ ! -f "$GPG_AGENT_SOCK" ]; then
    (setsid socat UNIX-LISTEN:$GPG_AGENT_SOCK,fork EXEC:"/absolute/path/to/wsl-gap.exe --gpg" &) >/dev/null 2>&1
fi
```

## Example
See the script [wsl-gap.sh](./wsl-gap.sh) for an example on how to setup. The script
can be included in your `.bashrc` and used to setup for both gpg and ssh.

## Credits
https://github.com/benpye/wsl-ssh-pageant and https://github.com/BlackReloaded/wsl2-ssh-pageant for inspiration.  
https://github.com/Yasushi/putty/ for showing how to communicate with Pageant.
