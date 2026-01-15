# Command Reference

Complete reference for all interminai commands.

## interminai start

Start an interactive terminal session.

```bash
interminai start [--socket PATH] [--size WxH] [--emulator BACKEND] [--no-daemon] -- COMMAND...
```

**Options:**
- `--socket PATH` - Unix socket path (auto-generated if not specified)
- `--size WxH` - Terminal size (default: 80x24)
- `--emulator BACKEND` - Terminal emulator backend (default: xterm)
  - `xterm` - Full xterm emulation with color support (recommended)
  - `custom` - Basic ANSI emulation, no colors
- `--no-daemon` - Run in foreground instead of daemon mode
- `--pty-dump FILE` - Dump raw PTY output to file (for debugging)

**Output:**
```
Socket: /tmp/interminai-xyz/socket
PID: 12345
Auto-generated: true
```

**Behavior:**
- **Default (daemon mode):** Forks into background and returns immediately. Perfect for AI agents and scripts.
- **With `--no-daemon`:** Runs in foreground and blocks until stopped. Useful for debugging and testing.

**Examples:**
```bash
# Daemon mode (default) - returns immediately
interminai start --socket /tmp/s.sock -- vim file.txt

# Foreground mode - blocks until stopped
interminai start --socket /tmp/s.sock --no-daemon -- vim file.txt
```

**Always capture the socket path from output!**

## interminai input

Send keyboard input to the session.

```bash
interminai input --socket PATH --text ':wq\n'
# equivalent to:
printf ':wq\n' | interminai input --socket PATH
```

**Options:**
- `--text TEXT` - Input text with escape sequences (preferred, alternative to stdin)
- `--password` - Prompt user to type password and press Enter (sent as `\r`)

### Using --text (Recommended)

The `--text` flag supports C-style escape sequences:

| Escape | Value | Description |
|--------|-------|-------------|
| `\n` | 0x0A | Newline (Enter) |
| `\r` | 0x0D | Carriage return |
| `\t` | 0x09 | Tab |
| `\e` | 0x1B | ESC key |
| `\f` | 0x0C | Form feed (Ctrl+L) |
| `\a` | 0x07 | Bell |
| `\b` | 0x08 | Backspace |
| `\v` | 0x0B | Vertical tab |
| `\\` | 0x5C | Literal backslash |
| `\xHH` | - | Hex byte (e.g., `\x1b`) |

**Arrow keys and special keys:**

| Key | Escape Sequence |
|-----|-----------------|
| Arrow Up | `\e[A` |
| Arrow Down | `\e[B` |
| Arrow Right | `\e[C` |
| Arrow Left | `\e[D` |
| Home | `\e[H` |
| End | `\e[F` |
| Page Up | `\e[5~` |
| Page Down | `\e[6~` |
| F1-F4 | `\eOP`, `\eOQ`, `\eOR`, `\eOS` |
| F5-F12 | `\e[15~` through `\e[24~` |

**Examples:**
```bash
# Type text and press Enter
interminai input --socket /tmp/vim.sock --text 'Hello World\n'

# Send Escape then :wq
interminai input --socket /tmp/vim.sock --text '\e:wq\n'

# Send Ctrl+L to redraw
interminai input --socket /tmp/app.sock --text '\f'

# Arrow key navigation
interminai input --socket /tmp/vim.sock --text '\e[A\e[A'  # Up twice

# Vim: insert "Hello", escape, save and quit
interminai input --socket /tmp/vim.sock --text 'iHello\e:wq\n'
```

### Using stdin (Alternative)

```bash
printf 'Hello\n' | interminai input --socket /tmp/vim.sock
```

**Note:** Use `printf`, NOT `echo` (which adds an unwanted newline).

## interminai output

Get the current screen contents.

```bash
interminai output --socket PATH [--color] [--no-color] [--cursor MODE]
```

**Options:**
- `--color` - Enable color output with ANSI escape codes (default)
- `--no-color` - Disable color output, plain text only (use for grep/head)
- `--cursor MODE` - Cursor display mode (default: none)
  - `none` - No cursor indication (default)
  - `print` - Show "Cursor: row X, col Y" before screen output (1-based)
  - `inverse` - Highlight cursor position with inverse video
  - `both` - Both print and inverse modes

**Output:** Terminal screen content (rows × columns).

**Example output (default, with colors):**
```
Hello World
~
~
~
"file.txt" [New] 1L, 12B written       1,1           All
```

**Example output (--cursor print):**
```
Cursor: row 1, col 12
Hello World
~
~
~
"file.txt" [New] 1L, 12B written       1,1           All
```

**Note:** The `print` mode uses 1-based indexing (row 1, col 1 = top-left corner), following standard terminal conventions.

**When to use cursor modes:**
- Use `--cursor print` when you need exact cursor position for navigation
- Use `--cursor inverse` for visual debugging of where the cursor is
- Use `--cursor both` when you want both textual and visual confirmation
- Most applications (bash, TUI apps) don't show cursor position on screen, so cursor flags are helpful for knowing where input will go

**Using --no-color for grep/head:**

Use `--no-color` when piping output to tools that don't handle ANSI escape codes:

```bash
# Get first 5 lines without color codes
interminai output --socket /tmp/app.sock --no-color | head -5

# Search for a pattern
interminai output --socket /tmp/app.sock --no-color | grep "error"
```

**Color output details:**

Default output includes ANSI escape codes for colors and text attributes.
Supported attributes:
- Foreground/background colors (named, 256-color, 24-bit RGB)
- Bold, dim, italic, underline, inverse, strikethrough

**Note:** Colors require `--emulator xterm` (default). With `--emulator custom`
you get plain text regardless of the --color flag.

## interminai status

Check process status.

```bash
interminai status --socket PATH [--quiet]
```

**Options:**
- `--quiet` - Just exit status (0 if running, 1 if exited)

**Default output:**
```
Running: true
Activity: true
```
or when process has exited:
```
Running: false
Activity: false
Exit code: 0
```

**With `--quiet`:**
- Exit codes: `0` if running, `1` if exited (prints exit code to stdout)

**Example:**
```bash
if interminai status --socket /tmp/app.sock --quiet; then
    echo "Still running"
else
    echo "Process exited"
fi
```

## interminai wait

Block until the child process exits, or until activity occurs.

```bash
interminai wait --socket PATH [--quiet]
```

**Options:**
- `--quiet` - Wait for exit only, print exit code

**Default output:**
Reports both terminal activity and exit status:
```
Terminal activity: true
Application exited: false
```
The activity flag is cleared after reading.

**With `--quiet`:**
- Exit code of child process (printed to stdout)

**Examples:**
```bash
# Wait for any activity (output or exit)
interminai wait --socket /tmp/app.sock
# Output:
#   Terminal activity: true
#   Application exited: false

# Wait for process to exit only
interminai wait --socket /tmp/vim.sock --quiet
echo "Vim exited with code: $?"
```

**Wait triggers:**
- Any output received from the PTY (the application printed something)
- Child process exited

**Use cases for default mode:**
- Detect when a long-running command finishes or produces output
- Wait for a prompt to appear before sending input
- Monitor for error conditions that produce output

**Use cases for --quiet mode:**
- Solely wait for program to exit, ignoring its output


## interminai kill

Send a signal to the child process.

```bash
interminai kill --socket PATH --signal SIGNAL
```

**Signals (named):**
- `SIGTERM` (15) - Graceful termination
- `SIGKILL` (9) - Force kill
- `SIGINT` (2) - Interrupt (Ctrl+C)
- `SIGHUP` (1) - Hangup
- `SIGQUIT` (3) - Quit
- `SIGUSR1`, `SIGUSR2` - User-defined

**Signals (numeric):** `1`, `2`, `9`, `15`, etc.

**Examples:**
```bash
# Graceful termination
interminai kill --socket /tmp/app.sock --signal SIGTERM

# Force kill
interminai kill --socket /tmp/app.sock --signal SIGKILL

# Send Ctrl+C
interminai kill --socket /tmp/app.sock --signal SIGINT

# Numeric signal
interminai kill --socket /tmp/app.sock --signal 9
```

## interminai resize

Change terminal dimensions.

```bash
interminai resize --socket PATH --size WxH
```

**Size format:** `<columns>x<rows>` (e.g., `120x40`)

**Example:**
```bash
interminai resize --socket /tmp/vim.sock --size 120x40
```

The child process receives `SIGWINCH` signal and can respond to the resize.

## interminai stop

Stop the daemon and clean up.

```bash
interminai stop --socket PATH
```

**Always call this** when done, even if the child process has exited.

If the socket was auto-generated by `interminai start`, it will be removed. If you specified the socket path, it will be left in place for reuse.

## interminai debug

Show debug information: unhandled escape sequences and terminal (termios) settings.

```bash
interminai debug --socket PATH [--clear]
```

**Options:**
- `--clear` - Clear the unhandled sequences buffer after displaying

**Example output:**
```
Unhandled escape sequences:
  \e\ (1b5c)
Termios:
  Mode: raw
  Flags: OPOST
  Hex: iflag=0x0000 oflag=0x0001 lflag=0x0a20 cflag=0xf00bf
  c_cc: VINTR=^C VEOF=^D VERASE=^? VKILL=^U VSUSP=^Z VQUIT=^\
```

**Fields explained:**
- `Mode`: "cooked" (canonical, line-buffered) or "raw" (each keystroke immediate)
- `Flags`: Active termios flags
  - ECHO - Input is echoed back
  - ISIG - Ctrl+C sends SIGINT, Ctrl+Z sends SIGTSTP
  - ICRNL - CR (\\r) translated to NL (\\n) on input
  - IXON - XON/XOFF flow control enabled
  - OPOST/ONLCR - Output processing (NL→CRNL)
- `Hex`: Hex values of c_iflag, c_oflag, c_lflag, c_cflag
- `c_cc`: Control characters (e.g., ^C = 0x03 triggers VINTR)

**Use cases:**
- Debug why \\r vs \\n behaves differently (check ICRNL flag)
- Verify an app is in raw mode (TUI apps should be)
- Identify unhandled escape sequences causing rendering issues

### --pty-dump (on start command)

For low-level debugging, capture all raw PTY output:

```bash
interminai start --socket /tmp/s.sock --pty-dump /tmp/pty.log -- vim file.txt
# ... interact with session ...
interminai stop --socket /tmp/s.sock

# Examine raw bytes
xxd /tmp/pty.log | head -50
```

The dump file contains raw bytes exactly as received from the PTY, useful for:
- Debugging escape sequence issues
- Reverse engineering terminal protocols
- Reproducing rendering bugs

## Error Handling

### "No such file or directory"
Socket doesn't exist - daemon not started or crashed.

**Solution:**
```bash
ls -la /tmp/your-socket.sock  # Check if exists
interminai start --socket /tmp/your-socket.sock -- COMMAND
```

### "Connection refused"
Daemon not listening yet.

**Solution:** Add delay after start:
```bash
interminai start --socket /tmp/app.sock -- COMMAND
sleep 0.5  # Wait for daemon to initialize
interminai output --socket /tmp/app.sock
```

### "Invalid size"
Size format must be `WxH`.

**Wrong:** `80,24`, `80 24`, `80-24`
**Right:** `80x24`

## Limitations

- **No mouse support** - PTY is keyboard-only
- **No clipboard access** - Can't copy/paste from system clipboard
- **Limited escape sequences** - Complex terminal features may not work
