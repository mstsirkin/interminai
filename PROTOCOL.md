# interminai Socket Protocol

## Overview

The `interminai` daemon communicates with client commands via a Unix domain socket using a simple JSON-based request-response protocol.

**Note on encoding:** JSON strings automatically escape special characters:
- Escape sequences like `\x1b` (ESC) become `\u001b` in JSON
- Newlines `\n` become `\\n`
- Rust's `serde_json` handles encoding/decoding automatically
- Application code works with raw bytes/strings, JSON library handles escaping

**Alternatives considered:**
- Binary protocol with length prefixes (more efficient but harder to debug)
- Base64 encoding (adds 33% overhead, less human-readable)
- JSON chosen for debuggability and simplicity

## Connection Model

- Client connects to Unix socket
- Client sends one JSON request (newline-terminated)
- Daemon sends one JSON response (newline-terminated)
- Connection closes after response (except for WAIT which may block)

## Request Format

All requests are JSON objects with a `type` field:

```json
{
  "type": "COMMAND_NAME",
  ... additional fields ...
}
```

## Response Format

All responses are JSON objects:

```json
{
  "status": "ok" | "error",
  "data": { ... },
  "error": "error message if status is error"
}
```

## Commands

### INPUT - Send input to process

**Request:**
```json
{
  "type": "INPUT",
  "data": "keys to send (may contain escape sequences)"
}
```

**Response:**
```json
{
  "status": "ok"
}
```

**Errors:**
- Process not running
- Failed to write to PTY

---

### OUTPUT - Get current screen state

**Request:**
```json
{
  "type": "OUTPUT",
  "format": "ascii" | "ansi"
}
```

**Response (ascii format - default):**
```json
{
  "status": "ok",
  "data": {
    "screen": "Plain text representation of screen\nwith newlines...",
    "cursor": {
      "row": 5,
      "col": 10
    },
    "size": {
      "rows": 24,
      "cols": 80
    }
  }
}
```

**Response (ansi format):**
```json
{
  "status": "ok",
  "data": {
    "screen": "\u001b[38;2;255;0;0mColored text\u001b[0m with ANSI codes...",
    "cursor": {
      "row": 5,
      "col": 10
    },
    "size": {
      "rows": 24,
      "cols": 80
    }
  }
}
```

**Notes:**
- `ascii`: Plain text, no color codes (default, works with all backends)
- `ansi`: Text with embedded ANSI escape codes for colors and attributes.
  Supported by Alacritty and pyte backends. Custom backend returns plain text.

---

### STATUS - Check process status

**Request:**
```json
{
  "type": "STATUS",
  "activity": false
}
```

The `activity` field is optional (default: false).

**Response (normal mode, activity=false):**
```json
{
  "status": "ok",
  "data": {
    "running": true
  }
}
```

**Response (process finished):**
```json
{
  "status": "ok",
  "data": {
    "running": false,
    "exit_code": 0
  }
}
```

**Response (activity mode, activity=true):**
```json
{
  "status": "ok",
  "data": {
    "running": true,
    "activity": true
  }
}
```

**Fields (activity mode):**
- `activity`: true if PTY output was received since last STATUS/WAIT with activity mode

The activity flag is cleared after reading.

---

### WAIT - Block until process exits or activity occurs

**Request:**
```json
{
  "type": "WAIT",
  "activity": false
}
```

The `activity` field is optional (default: false).

**Response (normal mode, activity=false):**
```json
{
  "status": "ok",
  "data": {
    "exit_code": 0
  }
}
```

**Response (activity mode, activity=true):**
```json
{
  "status": "ok",
  "data": {
    "activity": true,
    "exited": false
  }
}
```

**Fields (activity mode):**
- `activity`: true if PTY output was received (application printed something)
- `exited`: true if the child process has exited

**Notes:**
- Normal mode: blocks until the process exits
- Activity mode: returns as soon as PTY output is received OR process exits
- Connection stays open while waiting
- Returns immediately if condition already met (process exited, or activity pending)
- In activity mode, the activity flag is cleared after reading (subsequent calls block until new activity)

---

### KILL - Send signal to process

**Request:**
```json
{
  "type": "KILL",
  "signal": "SIGTERM" | "SIGKILL" | "SIGINT" | "9" | "15" | "2" | ...
}
```

**Response:**
```json
{
  "status": "ok",
  "data": {
    "signal_sent": "SIGTERM"
  }
}
```

**Errors:**
- Invalid signal name/number
- Process already dead
- Failed to send signal

---

### STOP - Shutdown daemon

**Request:**
```json
{
  "type": "STOP"
}
```

**Response:**
```json
{
  "status": "ok",
  "data": {
    "message": "Shutting down"
  }
}
```

**Notes:**
- Daemon will kill child process (if running)
- Daemon will close socket
- Daemon will exit after sending response
- If socket was auto-generated, daemon unlinks it before exit

---

### DEBUG - Get debug information

Returns unhandled escape sequences and terminal (termios) settings. Useful for
debugging rendering issues, identifying escape sequences an application uses,
and understanding terminal mode configuration.

**Request:**
```json
{
  "type": "DEBUG",
  "data": {
    "clear": false
  }
}
```

The `data` field is optional. If omitted or `clear` is false, the unhandled
sequences buffer is returned without modification. If `clear` is true, the
buffer is atomically returned and then cleared.

**Response:**
```json
{
  "status": "ok",
  "data": {
    "unhandled": [
      {"sequence": "\\e[?25l", "raw_hex": "1b5b3f32356c"},
      {"sequence": "\\e7", "raw_hex": "1b37"}
    ],
    "dropped": 5,
    "termios": {
      "mode": "cooked",
      "flags": ["ECHO", "ISIG", "ICRNL", "IXON", "OPOST", "ONLCR"],
      "hex": {
        "iflag": "0x0500",
        "oflag": "0x0005",
        "lflag": "0x8a3b",
        "cflag": "0xf00bf"
      },
      "c_cc": {
        "VINTR": "^C",
        "VEOF": "^D",
        "VERASE": "^?",
        "VKILL": "^U",
        "VSUSP": "^Z",
        "VQUIT": "^\\"
      }
    }
  }
}
```

**Fields:**
- `unhandled`: Array of unhandled escape sequences in FIFO order (oldest first)
  - `sequence`: Human-readable escape sequence (e.g., `\e[?25l`)
  - `raw_hex`: Raw bytes in hexadecimal
- `dropped`: Number of sequences dropped from the buffer due to overflow
- `termios`: Terminal settings (from `tcgetattr()`)
  - `mode`: "cooked" (canonical) or "raw" (non-canonical)
  - `flags`: Active termios flags (ECHO, ISIG, ICRNL, IXON, OPOST, ONLCR, etc.)
  - `hex`: Hex values for c_iflag, c_oflag, c_lflag, c_cflag
  - `c_cc`: Control characters in `^X` notation (e.g., `^C` = 0x03)

**Notes:**
- Buffer size is configurable via `--debug-buffer` flag to `start` (default: 10)
- Intentionally ignored sequences (like SGR/colors) are not recorded
- The `mode` field reflects ICANON: "cooked" means line-buffered input with
  editing (backspace works), "raw" means each keystroke is passed immediately
- Common flags: ECHO (echo input), ISIG (Ctrl+C sends SIGINT), ICRNL (CR→NL
  translation), OPOST/ONLCR (output processing)

---

## Error Handling

### Malformed Requests

If a request cannot be parsed as JSON or is missing required fields:

```json
{
  "status": "error",
  "error": "Invalid request: missing 'type' field"
}
```

### Unknown Commands

If the `type` field contains an unknown command:

```json
{
  "status": "error",
  "error": "Unknown command: INVALID_COMMAND"
}
```

### Operation Failures

If a command fails to execute:

```json
{
  "status": "error",
  "error": "Failed to send input: Broken pipe"
}
```

**Important:** The daemon **must not crash** on errors. It should:
1. Send an error response
2. Close the connection
3. Continue serving other requests

---

## Client Disconnection

If a client disconnects before reading the full response:
- Daemon detects broken pipe / connection reset
- Daemon discards remaining response data
- Daemon logs the event (if debugging enabled)
- Daemon continues serving other clients

The daemon **must be resilient** to clients dying mid-response.

---

## Concurrency

**Commands are processed sequentially, one at a time.** The daemon does not
process commands in parallel. This guarantees that if you send command A
then command B, A will complete before B starts.

This means:
- No race conditions between commands
- Predictable ordering
- WAIT will block all other commands until the process exits

If you need to send input while a WAIT is pending, don't use WAIT - poll
with STATUS instead.

---

## Wire Format Example

**Client sends (pressing 'i', typing 'hello', ESC, then :wq):**
```
{"type":"INPUT","data":"ihello\u001b:wq\n"}\n
```

**Daemon responds:**
```
{"status":"ok"}\n
```

**Client sends (getting screen output):**
```
{"type":"OUTPUT","format":"ascii"}\n
```

**Daemon responds (screen contains escape sequences in output):**
```
{"status":"ok","data":{"screen":"  File  Edit  View\n~\n~\n","cursor":{"row":1,"col":0},"size":{"rows":24,"cols":80}}}\n
```

**Notes:**
- Each message is a single line of JSON terminated by `\n`
- Binary data (like ESC = `\x1b`) is escaped as `\u001b` in JSON
- JSON libraries handle escaping automatically - app code uses raw strings
- Maximum message size: 10MB (reasonable limit for screen output)
- The `\n` at the end of each JSON message is the message delimiter, not part of the JSON

---

## Signal Mapping

Named signals to numbers (POSIX standard):

| Name     | Number |
|----------|--------|
| SIGHUP   | 1      |
| SIGINT   | 2      |
| SIGQUIT  | 3      |
| SIGKILL  | 9      |
| SIGTERM  | 15     |
| SIGUSR1  | 10     |
| SIGUSR2  | 12     |

Both formats are accepted. Daemon normalizes to signal number internally.
