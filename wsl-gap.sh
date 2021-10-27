# Include this script in your .bashrc and then call the wsl_gap_init function to
# setup gpg-agent in WSL to use the gpg-agent in Windows.
# The script also adds two aliases: gpg-restart-windows-agent and gpg-kill-windows-agent
#
# In .bashrc:
#   [ -f wsl-gap.sh ] && source wsl-gap.sh && wsl_gap_init
#

wsl_gap_init() {
    # GNUPGHOME in WSL/WSL2
    export GNUPGHOME=$HOME/.gnupg
    # Absolute path to wsl-gap.exe
    WSL_GAP_BIN=$HOME/bin/wsl-gap.exe

    GNUPG_BIN_DIR=$(wslpath 'C:/Program Files (x86)/gnupg/bin')
    [ -d "$GNUPG_BIN_DIR" ] && [ ! -x "$GNUPG_BIN_DIR/gpg-agent.exe" ] && echo "WARNING: gpg-agent.exe not executable in WSL!"

    # Paths to socket files.
    GPG_AGENT_SOCK_PATH=/tmp/S.gpg-agent
    SSH_AUTH_SOCK_PATH=/tmp/S.gpg-agent.ssh

    # Make sure the exe exists and is executable.
    chmod +x $WSL_GAP_BIN
    if [ $? -ne 0 ]; then
        echo "[$BASH_SOURCE] ERROR: $WSL_GAP_BIN not found." >&2; return 1;
    fi

    if [ ! -z $WSL_INTEROP ]; then
        # WSL2: can use the `ss` command to see if sockets exists, and if not
        # make sure that the socket file doesn't exist.
        SOCKETS=$(ss -lx)
        echo "$SOCKETS" | grep -q $GPG_AGENT_SOCK_PATH
        [ $? -ne 0 ] && rm -rf $GPG_AGENT_SOCK_PATH
        echo "$SOCKETS" | grep -q $SSH_AUTH_SOCK_PATH
        [ $? -ne 0 ] && rm -rf $SSH_AUTH_SOCK_PATH
    fi

    # gpg expects S.gpg-agent socket to be in the GNUPGHOME folder, so it must be redirected
    [ ! -f "$GNUPGHOME/S.gpg-agent" ] && echo -e "%Assuan%\nsocket=$GPG_AGENT_SOCK_PATH" > $GNUPGHOME/S.gpg-agent
    # Forward the GPG_AGENT_SOCK to stdin/stdout of the gpg-agent proxy
    if [ ! -f "$GPG_AGENT_SOCK_PATH" ]; then
        # Use setsid to force new session to keep it running when current terminal closes
        (setsid socat UNIX-LISTEN:$GPG_AGENT_SOCK_PATH,fork EXEC:"$WSL_GAP_BIN --gpg" &) >/dev/null 2>&1
    fi

    # Forward the SSH_AUTH_SOCK to stdin/stdout of the gpg-agent proxy
    if [ ! -f "$SSH_AUTH_SOCK_PATH" ]; then
        # Use setsid to force new session to keep it running when current terminal closes
        (setsid socat UNIX-LISTEN:$SSH_AUTH_SOCK_PATH,fork EXEC:"$WSL_GAP_BIN --ssh" &) >/dev/null 2>&1
    fi
    export SSH_AUTH_SOCK=$SSH_AUTH_SOCK_PATH

    # Useful aliases for managing the Windows gpgagent.
    alias gpg-restart-windows-agent='_restart_windows_gpg_agent'
    alias gpg-kill-windows-agent='_kill_windows_gpg_agent'

    _kill_windows_gpg_agent() {
        GNUPG_BIN_DIR=$(wslpath 'C:/Program Files (x86)/gnupg/bin')
        "$GNUPG_BIN_DIR/gpgconf.exe" --kill gpg-agent
    }

    _restart_windows_gpg_agent() {
        GNUPG_BIN_DIR=$(wslpath 'C:/Program Files (x86)/gnupg/bin')
        "$GNUPG_BIN_DIR/gpgconf.exe" --kill gpg-agent
        echo "/bye" | "$GNUPG_BIN_DIR/gpg-connect-agent.exe" "scd serialno" "learn --force"
    }
}
