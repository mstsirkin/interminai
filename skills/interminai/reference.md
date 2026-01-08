# Command Reference

Complete reference for all interminai commands.

## interminai start

Start an interactive terminal session.

```bash
interminai start [--socket PATH] [--size WxH] [--no-daemon] -- COMMAND...
```

**Options:**
- `--socket PATH` - Unix socket path (auto-generated if not specified)
- `--size WxH` - Terminal size (default: 80x24)
- `--no-daemon` - Run in foreground instead of daemon mode

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
interminai output --socket PATH [--format ascii] [--cursor MODE]
```

**Options:**
- `--format ascii` - Output format (default: ascii)
- `--cursor MODE` - Cursor display mode (default: none)
  - `none` - No cursor indication (default)
  - `print` - Show "Cursor: row X, col Y" before screen output (1-based)
  - `inverse` - Highlight cursor position with inverse video
  - `both` - Both print and inverse modes

**Output:** ASCII art representation of terminal screen (rows × columns).

**Example output (default):**
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

## interminai running

Check if the child process is still running.

```bash
interminai running --socket PATH
```

**Exit codes:**
- `0` - Process is running
- `1` - Process has exited (exit code printed to stdout)

**Example:**
```bash
if interminai running --socket /tmp/app.sock; then
    echo "Still running"
else
    echo "Process exited"
fi
```

## interminai wait

Block until the child process exits.

```bash
interminai wait --socket PATH
```

**Returns:** Exit code of child process (printed to stdout).

**Example:**
```bash
interminai wait --socket /tmp/vim.sock
echo "Vim exited with code: $?"
```

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
- **No colors** - Output is plain ASCII text
- **Limited escape sequences** - Complex terminal features may not work
