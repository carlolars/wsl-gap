# Include this script in your .bashrc and then call the _wsl_gap_init function to
# setup gpg-agent in WSL to use the gpg-agent in Windows.
#
# Example, add to your .bashrc:
#   [ -f wsl-gap.sh ] && source wsl-gap.sh && _wsl_gap_init
#
# Environment Variables
# =====================
# The script exports the following environment variables:
#   GNUPGHOME - set to $HOME/.gnupg
#   GPG_SSH_AUTH_SOCK - Path to the SSH auth socket used to talk to the gpg-agent.
#   SSH_AUTH_SOCK - Set to GPG_SSH_AUTH_SOCK, used by ssh.
#   SSH_AGENT_PID - Set to the PID of the socat process, used by some apps to
#                   detect if an ssh-agent is running.
#
# The SSH_* variables can be overwritten if for example starting a "real" ssh-agent
# after running _wsl_gap_init.
#
# Aliases
# =======
# The script adds a few aliases:
#   gpg-restart-windows-agent - restarts the windows gpg-agent
#   gssh - the ssh command but explicitly using the GPG_SSH_AUTH_SOCK, useful if
#          also running another ssh-agent in parallel.
#   gssh-add - the ssh-add command but explicitly using the GPG_SSH_AUTH_SOCK.
#

_wsl_gap_init() {
    # Absolute path to wsl-gap.exe
    local WSL_GAP_BIN=$HOME/bin/wsl-gap.exe
    # Paths to socket files.
    local GPG_AGENT_SOCK_PATH=/tmp/S.gpg-agent
    local GPG_SSH_AUTH_SOCK_PATH=/tmp/S.gpg-agent.ssh

    # Make sure that wsl-gap.exe exists and is executable.
    if [ ! -e $WSL_GAP_BIN ]; then
         echo "[$BASH_SOURCE] ERROR: The file '$WSL_GAP_BIN' does not exist." >&2; return 1;
    elif [ ! -x $WSL_GAP_BIN ]; then
        if [ `chmod +x $WSL_GAP_BIN` ]; then
            echo "[$BASH_SOURCE] ERROR: Failed to set execution bit on $WSL_GAP_BIN ." >&2; return 1;
        fi
    fi

    local GNUPG_WIN_BIN_DIR=$(wslpath 'C:/Program Files (x86)/gnupg/bin')
    if [ -d "$GNUPG_WIN_BIN_DIR" ]; then
        if [ ! -x "$GNUPG_WIN_BIN_DIR/gpg-agent.exe" ]; then
            echo "Warning: $GNUPG_WIN_BIN_DIR/gpg-agent.exe not executable in WSL!"
        fi
    fi

    # Find out if socat is running, if not cleanup the socket file
    if [ -z "$(pgrep -f 'socat UNIX-LISTEN:/tmp/S.gpg-agent,fork')" ]; then
        if [ -e $GPG_AGENT_SOCK_PATH ]; then
            rm -f $GPG_AGENT_SOCK_PATH
        fi
    fi

    if [ -z "$(pgrep -f 'socat UNIX-LISTEN:/tmp/S.gpg-agent.ssh,fork')" ]; then
        if [ -e $GPG_SSH_AUTH_SOCK_PATH ]; then
            rm -f $GPG_SSH_AUTH_SOCK_PATH
        fi
    fi

    # GNUPGHOME in WSL/WSL2
    export GNUPGHOME=$HOME/.gnupg

    # gpg expects S.gpg-agent socket to be in the GNUPGHOME folder, so it must be redirected
    if [ ! -f "$GNUPGHOME/S.gpg-agent" ]; then
        echo -e "%Assuan%\nsocket=$GPG_AGENT_SOCK_PATH" > $GNUPGHOME/S.gpg-agent
    fi

    # Forward the GPG_AGENT_SOCK to stdin/stdout of the gpg-agent proxy
    if [ ! -e "$GPG_AGENT_SOCK_PATH" ]; then
        # Use setsid to force new session to keep it running when current terminal closes
        (setsid socat UNIX-LISTEN:$GPG_AGENT_SOCK_PATH,fork EXEC:"$WSL_GAP_BIN --gpg" &) >/dev/null 2>&1
    fi

    # Forward the SSH_AUTH_SOCK to stdin/stdout of the gpg-agent proxy
    if [ ! -e "$GPG_SSH_AUTH_SOCK_PATH" ]; then
        # Use setsid to force new session to keep it running when current terminal closes
        (setsid socat UNIX-LISTEN:$GPG_SSH_AUTH_SOCK_PATH,fork EXEC:"$WSL_GAP_BIN --ssh" &) >/dev/null 2>&1
    fi

    export GPG_SSH_AUTH_SOCK=$GPG_SSH_AUTH_SOCK_PATH

    export SSH_AUTH_SOCK=$GPG_SSH_AUTH_SOCK_PATH
    export SSH_AGENT_PID="$(pgrep -f 'socat UNIX-LISTEN:/tmp/S.gpg-agent.ssh,fork')"

    # Aliases for ssh explicitly using the GPG_SSH_AUTH_SOCK
    alias gssh='SSH_AUTH_SOCK=$GPG_SSH_AUTH_SOCK ssh'
    alias gssh-add='SSH_AUTH_SOCK=$GPG_SSH_AUTH_SOCK ssh-add'

    # Useful aliases for managing the Windows gpgagent.
    alias gpg-restart-windows-agent='_restart_windows_gpg_agent'
    alias gpg-kill-windows-agent='_kill_windows_gpg_agent'

    _kill_windows_gpg_agent() {
        GNUPG_WIN_BIN_DIR=$(wslpath 'C:/Program Files (x86)/gnupg/bin')
        "$GNUPG_WIN_BIN_DIR/gpgconf.exe" --kill gpg-agent
    }

    _restart_windows_gpg_agent() {
        GNUPG_WIN_BIN_DIR=$(wslpath 'C:/Program Files (x86)/gnupg/bin')
        "$GNUPG_WIN_BIN_DIR/gpgconf.exe" --kill gpg-agent
        echo "/bye" | "$GNUPG_WIN_BIN_DIR/gpg-connect-agent.exe" "scd serialno" "learn --force"
    }
}
