// 🌀 an Interactive Terminal for AI (interminai)
//
// Author: Michael S. Tsirkin <mst@kernel.org>
//
// A PTY-based tool for interacting with terminal applications (Rust version).

mod terminal;
mod custom_screen;
mod alacritty_backend;

use clap::{Parser as ClapParser, Subcommand};
use anyhow::{Result, Context, bail};
use std::process::{Command as ProcessCommand};
use std::os::unix::process::CommandExt;
use tempfile::Builder;
use serde::{Deserialize, Serialize};
use std::os::unix::net::{UnixListener, UnixStream};
use std::io::{BufRead, BufReader, Write, Read};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use nix::pty::{openpty, Winsize};
use nix::unistd::{setsid, Pid};
use nix::sys::wait::{waitpid, WaitStatus, WaitPidFlag};
use nix::sys::signal::{kill, Signal};
use nix::sys::termios::{tcgetattr, LocalFlags, InputFlags, OutputFlags, SpecialCharacterIndices};
use std::os::fd::{AsRawFd, OwnedFd};
use std::fs;
use std::path::Path;

use terminal::TerminalEmulator;

/// Terminal emulator backend
#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
pub enum Emulator {
    /// Full xterm-256color terminal emulation (alacritty backend)
    #[default]
    Xterm,
    /// Basic ANSI terminal emulation (custom backend)
    Custom,
}

#[derive(ClapParser)]
#[command(name = "interminai")]
#[command(about = "🌀 an Interactive Terminal for AI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a new interactive terminal session
    Start {
        /// Unix socket path (auto-generated if not specified)
        #[arg(long)]
        socket: Option<String>,

        /// Terminal size (e.g., 80x24)
        #[arg(long, default_value = "80x24")]
        size: String,

        /// Terminal emulator backend (xterm or custom)
        #[arg(long, value_enum, default_value = "xterm")]
        emulator: Emulator,

        /// Run in foreground (for debugging/testing, default: daemon mode)
        #[arg(long)]
        no_daemon: bool,

        /// Dump all raw PTY output to this file (for debugging)
        #[arg(long)]
        pty_dump: Option<String>,

        /// Command to run
        #[arg(required = true, last = true)]
        command: Vec<String>,
    },

    /// Send input to running session
    Input {
        /// Unix socket path (required)
        #[arg(long, required = true)]
        socket: String,

        /// Input text with escape sequences (alternative to stdin)
        /// Supports: \n \r \t \a \b \f \v \\ \e \xHH
        #[arg(long)]
        text: Option<String>,

        /// Read password from terminal with echo disabled
        /// Automatically appends \r (Enter) after input
        #[arg(long)]
        password: bool,
    },

    /// Get screen output from running session
    Output {
        /// Unix socket path (required)
        #[arg(long, required = true)]
        socket: String,

        /// Enable color output (default)
        #[arg(long)]
        color: bool,

        /// Disable color output (plain text, useful for grep/head)
        #[arg(long)]
        no_color: bool,

        /// Cursor display mode (none, inverse, print, both)
        #[arg(long, default_value = "none")]
        cursor: String,
    },

    /// Stop running session
    Stop {
        /// Unix socket path (required)
        #[arg(long, required = true)]
        socket: String,
    },

    /// Get session status
    Status {
        /// Unix socket path (required)
        #[arg(long, required = true)]
        socket: String,

        /// Quiet mode: just exit status (0 if running, 1 if exited)
        #[arg(long)]
        quiet: bool,
    },

    /// Wait until session exits or activity occurs
    Wait {
        /// Unix socket path (required)
        #[arg(long, required = true)]
        socket: String,

        /// Quiet mode: wait for exit only, print exit code
        #[arg(long)]
        quiet: bool,

        /// Wait until content of this line number changes (1-based)
        #[arg(long = "line", value_name = "LINE")]
        line: Option<usize>,

        /// With --line: wait until line does NOT contain this pattern
        #[arg(long = "not-contains", value_name = "PATTERN")]
        not_contains: Option<String>,

        /// With --line: wait until line contains this pattern
        #[arg(long = "contains", value_name = "PATTERN")]
        contains: Option<String>,
    },

    /// Send signal to running process
    Kill {
        /// Unix socket path (required)
        #[arg(long, required = true)]
        socket: String,

        /// Signal to send (named like SIGTERM, SIGKILL, SIGINT or numeric like 9, 15, 2)
        #[arg(long, default_value = "SIGTERM")]
        signal: String,
    },

    /// Resize the terminal
    Resize {
        /// Unix socket path (required)
        #[arg(long, required = true)]
        socket: String,

        /// New terminal size (e.g., 120x40)
        #[arg(long)]
        size: String,
    },

    /// Show unhandled escape sequences (for debugging)
    Debug {
        /// Unix socket path (required)
        #[arg(long, required = true)]
        socket: String,

        /// Clear the buffer after reading
        #[arg(long)]
        clear: bool,
    },
}

// Protocol messages
#[derive(Deserialize)]
struct Request {
    #[serde(rename = "type")]
    req_type: String,
    #[serde(flatten)]
    data: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
struct Response {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl Response {
    fn ok(data: serde_json::Value) -> Self {
        Response {
            status: "ok".to_string(),
            data: Some(data),
            error: None,
        }
    }

    fn error(msg: String) -> Self {
        Response {
            status: "error".to_string(),
            data: None,
            error: Some(msg),
        }
    }
}

// Terminal emulator factory
fn create_terminal(rows: usize, cols: usize, emulator: Emulator) -> Box<dyn TerminalEmulator> {
    match emulator {
        Emulator::Xterm => Box::new(alacritty_backend::AlacrittyTerminal::new(rows, cols)),
        Emulator::Custom => Box::new(custom_screen::CustomScreen::new(rows, cols)),
    }
}

struct DaemonState {
    master_fd: OwnedFd,
    child_pid: Pid,
    terminal: Box<dyn TerminalEmulator>,
    exit_code: Option<i32>,
    socket_path: String,
    socket_was_auto_generated: bool,
    should_shutdown: bool,
    pty_dump: Option<std::fs::File>,
    /// Activity flag: set when PTY output is received
    activity: bool,
}

impl DaemonState {
    fn check_child_status(&mut self) {
        if self.exit_code.is_some() {
            return;
        }

        match waitpid(self.child_pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(_, code)) => {
                self.exit_code = Some(code);
            }
            Ok(WaitStatus::Signaled(_, sig, _)) => {
                self.exit_code = Some(128 + sig as i32);
            }
            _ => {}
        }
    }

    fn read_pty_output(&mut self) {
        let mut buf = [0u8; 4096];
        loop {
            match nix::unistd::read(self.master_fd.as_raw_fd(), &mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    // Any output from PTY is activity
                    self.activity = true;
                    // Dump raw bytes if pty_dump is enabled
                    if let Some(ref mut dump) = self.pty_dump {
                        let _ = dump.write_all(&buf[..n]);
                    }
                    self.terminal.process_bytes(&buf[..n]);
                }
                Err(_) => break,
            }
        }

        // Send any pending responses back to the PTY (e.g., cursor position reports)
        for response in self.terminal.take_pending_responses() {
            let _ = nix::unistd::write(self.master_fd.as_raw_fd(), &response);
        }
    }

}

fn parse_terminal_size(size: &str) -> Result<(u16, u16)> {
    let parts: Vec<&str> = size.split('x').collect();
    if parts.len() != 2 {
        bail!("Invalid size format, expected WxH like 80x24");
    }
    let cols = parts[0].parse::<u16>().context("Invalid columns")?;
    let rows = parts[1].parse::<u16>().context("Invalid rows")?;
    Ok((cols, rows))
}

/// Unescape C-style escape sequences in a string.
/// Supports: \n \r \t \a \b \f \v \\ \e \xHH
fn unescape(s: &str) -> Result<String> {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('a') => result.push('\x07'),  // bell
                Some('b') => result.push('\x08'),  // backspace
                Some('f') => result.push('\x0C'),  // form feed (Ctrl+L)
                Some('v') => result.push('\x0B'),  // vertical tab
                Some('\\') => result.push('\\'),
                Some('e') | Some('E') => result.push('\x1B'),  // ESC
                Some('x') => {
                    // Parse two hex digits
                    let h1 = chars.next().ok_or_else(|| anyhow::anyhow!("incomplete \\x escape"))?;
                    let h2 = chars.next().ok_or_else(|| anyhow::anyhow!("incomplete \\x escape"))?;
                    let hex_str: String = [h1, h2].iter().collect();
                    let byte = u8::from_str_radix(&hex_str, 16)
                        .context(format!("invalid hex escape: \\x{}", hex_str))?;
                    result.push(byte as char);
                }
                Some(other) => {
                    // Unknown escape - keep as-is
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }

    Ok(result)
}

fn parse_signal(sig: &str) -> Result<Signal> {
    // Try parsing as number first
    if let Ok(num) = sig.parse::<i32>() {
        return Signal::try_from(num).context("Invalid signal number");
    }

    // Parse named signals
    let sig_upper = sig.to_uppercase();
    let sig_name = if sig_upper.starts_with("SIG") {
        sig_upper.as_str()
    } else {
        // Add SIG prefix if not present
        return parse_signal(&format!("SIG{}", sig));
    };

    match sig_name {
        "SIGHUP" => Ok(Signal::SIGHUP),
        "SIGINT" => Ok(Signal::SIGINT),
        "SIGQUIT" => Ok(Signal::SIGQUIT),
        "SIGKILL" => Ok(Signal::SIGKILL),
        "SIGTERM" => Ok(Signal::SIGTERM),
        "SIGUSR1" => Ok(Signal::SIGUSR1),
        "SIGUSR2" => Ok(Signal::SIGUSR2),
        _ => bail!("Unknown signal: {}", sig),
    }
}

fn auto_generate_socket_path() -> Result<String> {
    // Create a temporary directory with proper prefix
    let temp_dir = Builder::new()
        .prefix("interminai-")
        .tempdir()?;

    // Get the path and keep it persistent (leak the TempDir)
    let socket_path = temp_dir.path().join("socket").to_string_lossy().to_string();

    // Leak the temp dir so it doesn't get deleted
    std::mem::forget(temp_dir);

    Ok(socket_path)
}

fn cmd_start(socket: Option<String>, size: String, emulator: Emulator, daemon: bool, pty_dump: Option<String>, command: Vec<String>) -> Result<()> {
    let socket_was_auto_generated = socket.is_none();
    let socket_path = match socket {
        Some(path) => path,
        None => auto_generate_socket_path()?,
    };

    let (cols, rows) = parse_terminal_size(&size)?;

    if !daemon {
        // Run in foreground (default for now)
        println!("Socket: {}", socket_path);
        println!("PID: {}", std::process::id());
        println!("Auto-generated: {}", socket_was_auto_generated);

        return run_daemon(socket_path, socket_was_auto_generated, rows, cols, emulator, pty_dump, command);
    }

    // Double-fork to properly daemonize
    // Use fork crate which provides a safe wrapper around libc::fork()
    use fork::{fork as safe_fork, Fork};

    match safe_fork() {
        Ok(Fork::Parent(child)) => {
            // Parent process: wait for intermediate child to exit (avoid zombie)
            use nix::sys::wait::waitpid;
            use nix::unistd::Pid;
            let _ = waitpid(Pid::from_raw(child), None);

            // The intermediate child has printed the grandchild PID to stdout
            // Now print the rest of the info
            println!("Socket: {}", socket_path);
            println!("Auto-generated: {}", socket_was_auto_generated);
            Ok(())
        }
        Ok(Fork::Child) => {
            // Intermediate child: fork again, print grandchild PID, and exit
            match safe_fork() {
                Ok(Fork::Parent(grandchild_pid)) => {
                    // Intermediate parent: print grandchild PID to stdout and exit
                    println!("PID: {}", grandchild_pid);
                    std::process::exit(0);
                }
                Ok(Fork::Child) => {
                    // Grandchild: become daemon
                    setsid().expect("Failed to create new session");

                    // Redirect stdin/stdout/stderr to /dev/null (standard daemon behavior)
                    // Note: Programs running in the PTY are unaffected - they get their own
                    // stdin/stdout/stderr connected to the PTY slave
                    use std::fs::OpenOptions;
                    use std::os::unix::io::AsRawFd;
                    use nix::unistd::dup2;

                    // Open /dev/null - use std::fs for safe file opening
                    match OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open("/dev/null")
                    {
                        Ok(devnull) => {
                            let devnull_fd = devnull.as_raw_fd();

                            // Redirect stdin/stdout/stderr to /dev/null using safe dup2 wrapper
                            // dup2 handles both open and closed fds - it closes them first if needed
                            let _ = dup2(devnull_fd, 0);
                            let _ = dup2(devnull_fd, 1);
                            let _ = dup2(devnull_fd, 2);

                            // devnull will be automatically closed when it goes out of scope
                        }
                        Err(_) => {
                            // If we can't open /dev/null, continue anyway
                            // The daemon will just have closed fds
                        }
                    }

                    // Run daemon
                    if let Err(e) = run_daemon(socket_path, socket_was_auto_generated, rows, cols, emulator, pty_dump, command) {
                        // Daemon errors go to /dev/null in daemon mode, which is fine
                        eprintln!("Daemon error: {}", e);
                        std::process::exit(1);
                    }
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("Second fork failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            bail!("Failed to fork: {}", e)
        }
    }
}

fn run_daemon(socket_path: String, socket_was_auto_generated: bool, rows: u16, cols: u16, emulator: Emulator, pty_dump: Option<String>, command: Vec<String>) -> Result<()> {
    // Create PTY
    let winsize = Winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    let pty = openpty(Some(&winsize), None)?;

    // Fork to spawn child in PTY
    // Use fork crate which provides a safe wrapper around libc::fork()
    use fork::{fork as safe_fork, Fork};

    match safe_fork() {
        Ok(Fork::Parent(child)) => {
            // Close slave side in parent by dropping it (not using close() to avoid double-close)
            drop(pty.slave);

            // Set master to non-blocking
            // Use nix's safe fcntl wrapper (requires 'fs' feature)
            use nix::fcntl::{fcntl, FcntlArg, OFlag};

            let flags = fcntl(pty.master.as_raw_fd(), FcntlArg::F_GETFL)
                .context("Failed to get PTY flags")?;
            let mut oflags = OFlag::from_bits_truncate(flags);
            oflags.insert(OFlag::O_NONBLOCK);
            fcntl(pty.master.as_raw_fd(), FcntlArg::F_SETFL(oflags))
                .context("Failed to set PTY non-blocking")?;

            // Open PTY dump file if specified
            let pty_dump_file = match &pty_dump {
                Some(path) => Some(std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .context("Failed to open PTY dump file")?),
                None => None,
            };

            // Create state
            let state = Arc::new(Mutex::new(DaemonState {
                master_fd: pty.master,
                child_pid: Pid::from_raw(child),
                terminal: create_terminal(rows as usize, cols as usize, emulator),
                exit_code: None,
                socket_path: socket_path.clone(),
                socket_was_auto_generated,
                should_shutdown: false,
                pty_dump: pty_dump_file,
                activity: false,
            }));

            // Start PTY reader thread - use poll() for efficient event-driven I/O
            let state_clone = state.clone();
            // Dup the fd so the thread owns its own copy for polling
            let poll_fd = rustix::io::dup(&state.lock().unwrap().master_fd)?;
            thread::spawn(move || {
                use rustix::event::{poll, PollFd, PollFlags};
                let mut pty_closed = false;
                loop {
                    if pty_closed {
                        // PTY closed but child may still be running - poll child status only
                        let mut state = state_clone.lock().unwrap();
                        state.check_child_status();
                        if state.exit_code.is_some() {
                            break;
                        }
                        drop(state);
                        thread::sleep(Duration::from_millis(100));
                        continue;
                    }

                    // Wait for PTY events using poll()
                    let mut poll_fds = [PollFd::new(&poll_fd, PollFlags::IN | PollFlags::HUP)];
                    if poll(&mut poll_fds, None).is_err() {
                        break;
                    }

                    let mut state = state_clone.lock().unwrap();
                    let revents = poll_fds[0].revents();
                    if revents.contains(PollFlags::IN) {
                        state.read_pty_output();
                    }
                    if revents.intersects(PollFlags::HUP | PollFlags::ERR) {
                        state.read_pty_output();
                        pty_closed = true;
                    }
                    state.check_child_status();
                    if state.exit_code.is_some() {
                        break;
                    }
                }
            });

            // Create socket and listen
            let _ = fs::remove_file(&socket_path); // Clean up if exists
            let listener = UnixListener::bind(&socket_path)?;

            // Set socket to non-blocking so we can check shutdown flag
            listener.set_nonblocking(true)?;

            // Accept connections
            loop {
                // Check if we should exit
                {
                    let state_locked = state.lock().unwrap();
                    if state_locked.should_shutdown {
                        break;
                    }
                }

                match listener.accept() {
                    Ok((stream, _)) => {
                        // Process commands sequentially - no parallelism
                        if let Err(e) = handle_client(stream, state.clone()) {
                            eprintln!("Client handler error: {}", e);
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        // No connection available, sleep and try again
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(e) => {
                        eprintln!("Connection error: {}", e);
                    }
                }
            }

            // Give time for final requests to complete
            thread::sleep(Duration::from_millis(200));

            // Cleanup
            let state_locked = state.lock().unwrap();
            if state_locked.socket_was_auto_generated {
                let _ = fs::remove_file(&state_locked.socket_path);
                // Also remove the parent directory (the temp dir)
                if let Some(parent) = Path::new(&state_locked.socket_path).parent() {
                    let _ = fs::remove_dir(parent);
                }
            }

            Ok(())
        }
        Ok(Fork::Child) => {
            // Close master side in child by dropping it
            drop(pty.master);

            // Create new session - this makes the child a session leader
            // This is required for the PTY slave to become the controlling terminal
            setsid().context("Failed to create new session")?;

            // Redirect stdin/stdout/stderr to slave using nix
            use nix::unistd::dup2;
            let slave_fd = pty.slave.as_raw_fd();
            dup2(slave_fd, 0).context("Failed to dup2 stdin")?;
            dup2(slave_fd, 1).context("Failed to dup2 stdout")?;
            dup2(slave_fd, 2).context("Failed to dup2 stderr")?;

            // Make the PTY slave the controlling terminal for this session
            // TIOCSCTTY = "set controlling tty" - this must be done AFTER setsid()
            // and AFTER making stdin/stdout/stderr point to the slave
            if let Err(e) = rustix::process::ioctl_tiocsctty(&pty.slave) {
                // Non-fatal - continue anyway
                eprintln!("Warning: Failed to set controlling terminal: {}", e);
            }

            // Drop slave after dup2 (automatically closes it)
            drop(pty.slave);

            // Set TERM based on the terminal emulator backend
            // xterm (alacritty) supports full xterm-256color capabilities
            // custom uses basic ANSI escape sequences
            match emulator {
                Emulator::Xterm => std::env::set_var("TERM", "xterm-256color"),
                Emulator::Custom => std::env::set_var("TERM", "ansi"),
            }

            // Exec command
            let program = &command[0];
            let args = &command[1..];

            let _ = ProcessCommand::new(program)
                .args(args)
                .exec();

            std::process::exit(1);
        }
        Err(e) => bail!("Failed to fork for child: {}", e),
    }
}

fn handle_client(mut stream: UnixStream, state: Arc<Mutex<DaemonState>>) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();

    // Read line, checking for EOF (client disconnected before sending complete request)
    let bytes_read = reader.read_line(&mut line)?;
    if bytes_read == 0 {
        // EOF - client disconnected without sending complete request
        return Ok(());
    }

    let request: Request = match serde_json::from_str(&line) {
        Ok(req) => req,
        Err(e) => {
            let response = Response::error(format!("Invalid JSON: {}", e));
            write_response(&mut stream, &response)?;
            return Ok(());
        }
    };

    let response = match request.req_type.as_str() {
        "INPUT" => handle_input(request.data, &state),
        "OUTPUT" => handle_output(request.data, &state),
        "STATUS" => handle_running(request.data, &state),
        "WAIT" => handle_wait(request.data.clone(), &state, &stream),
        "KILL" => handle_kill(request.data, &state),
        "STOP" => handle_stop(&state),
        "RESIZE" => handle_resize(request.data, &state),
        "DEBUG" => handle_debug(request.data, &state),
        _ => Response::error(format!("Unknown command: {}", request.req_type)),
    };

    write_response(&mut stream, &response)?;

    Ok(())
}

fn write_response(stream: &mut UnixStream, response: &Response) -> Result<()> {
    let json = serde_json::to_string(response)?;
    stream.write_all(json.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

fn handle_input(data: serde_json::Value, state: &Arc<Mutex<DaemonState>>) -> Response {
    let input_data = match data.get("data").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return Response::error("Missing 'data' field".to_string()),
    };

    let state = state.lock().unwrap();

    match nix::unistd::write(state.master_fd.as_raw_fd(), input_data.as_bytes()) {
        Ok(_) => Response::ok(serde_json::json!({})),
        Err(e) => Response::error(format!("Failed to write to PTY: {}", e)),
    }
}

fn handle_output(data: serde_json::Value, state: &Arc<Mutex<DaemonState>>) -> Response {
    let format = data.get("format").and_then(|v| v.as_str()).unwrap_or("ascii");

    let mut state = state.lock().unwrap();
    state.read_pty_output();

    let screen_text = match format {
        "ansi" => state.terminal.get_screen_content_ansi(),
        _ => state.terminal.get_screen_content(),
    };
    let (cursor_row, cursor_col) = state.terminal.cursor_position();
    let (rows, cols) = state.terminal.dimensions();

    let data = serde_json::json!({
        "screen": screen_text,
        "cursor": {
            "row": cursor_row,
            "col": cursor_col
        },
        "size": {
            "rows": rows,
            "cols": cols
        }
    });

    Response::ok(data)
}

fn handle_running(data: serde_json::Value, state: &Arc<Mutex<DaemonState>>) -> Response {
    let activity_mode = data.get("activity").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut state = state.lock().unwrap();
    state.check_child_status();

    let running = state.exit_code.is_none();

    if activity_mode {
        let activity = state.activity;
        state.activity = false;  // Clear the flag after reading
        let mut response = serde_json::json!({
            "running": running,
            "activity": activity
        });
        if let Some(exit_code) = state.exit_code {
            response["exit_code"] = serde_json::json!(exit_code);
        }
        Response::ok(response)
    } else if let Some(exit_code) = state.exit_code {
        Response::ok(serde_json::json!({
            "running": false,
            "exit_code": exit_code
        }))
    } else {
        Response::ok(serde_json::json!({
            "running": true
        }))
    }
}

fn handle_wait(data: serde_json::Value, state: &Arc<Mutex<DaemonState>>, stream: &UnixStream) -> Response {
    use rustix::net::{recv, RecvFlags};

    let activity_mode = data.get("activity").and_then(|v| v.as_bool()).unwrap_or(false);

    loop {
        // Check if client disconnected using recv with MSG_PEEK | MSG_DONTWAIT
        let mut buf = [0u8; 1];
        let flags = RecvFlags::PEEK | RecvFlags::DONTWAIT;
        match recv(stream, &mut buf, flags) {
            Ok((_, 0)) => {
                // EOF - client disconnected
                return Response::error("Client disconnected".to_string());
            }
            Ok(_) => {
                // Unexpected data from client - ignore
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No data, client still connected - continue waiting
            }
            Err(_) => {
                // Real error - assume client disconnected
                return Response::error("Client disconnected".to_string());
            }
        }

        {
            let mut state = state.lock().unwrap();
            state.check_child_status();

            if activity_mode {
                // Activity mode: return as soon as activity or exit is detected
                // Get separate flags for PTY activity vs process exit
                let pty_activity = state.activity;
                let exited = state.exit_code.is_some();
                if pty_activity || exited {
                    // Clear the PTY activity flag
                    state.activity = false;
                    return Response::ok(serde_json::json!({
                        "activity": pty_activity,
                        "exited": exited
                    }));
                }
            } else {
                // Normal mode: wait for exit
                if let Some(exit_code) = state.exit_code {
                    return Response::ok(serde_json::json!({
                        "exit_code": exit_code
                    }));
                }
            }
        }

        thread::sleep(Duration::from_millis(100));
    }
}

fn handle_kill(data: serde_json::Value, state: &Arc<Mutex<DaemonState>>) -> Response {
    let signal_str = match data.get("signal").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return Response::error("Missing 'signal' field".to_string()),
    };

    let signal = match parse_signal(signal_str) {
        Ok(sig) => sig,
        Err(e) => return Response::error(format!("Invalid signal: {}", e)),
    };

    let state = state.lock().unwrap();

    match kill(state.child_pid, signal) {
        Ok(_) => Response::ok(serde_json::json!({
            "signal_sent": signal_str
        })),
        Err(e) => Response::error(format!("Failed to send signal: {}", e)),
    }
}

fn handle_stop(state: &Arc<Mutex<DaemonState>>) -> Response {
    let mut state = state.lock().unwrap();

    // Kill child if still running
    if state.exit_code.is_none() {
        let _ = kill(state.child_pid, Signal::SIGTERM);
    }

    // Set shutdown flag to exit daemon loop
    state.should_shutdown = true;

    Response::ok(serde_json::json!({
        "message": "Shutting down"
    }))
}

fn handle_resize(data: serde_json::Value, state: &Arc<Mutex<DaemonState>>) -> Response {
    let cols = match data.get("cols").and_then(|v| v.as_u64()) {
        Some(c) => c as u16,
        None => return Response::error("Missing 'cols' field".to_string()),
    };

    let rows = match data.get("rows").and_then(|v| v.as_u64()) {
        Some(r) => r as u16,
        None => return Response::error("Missing 'rows' field".to_string()),
    };

    let mut state = state.lock().unwrap();

    // Send TIOCSWINSZ to update terminal size using rustix's safe wrapper
    use rustix::termios::{tcsetwinsize, Winsize as RustixWinsize};

    let winsize = RustixWinsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    if let Err(_) = tcsetwinsize(&state.master_fd, winsize) {
        return Response::error("Failed to resize terminal".to_string());
    }

    // Update terminal emulator dimensions
    state.terminal.resize(rows as usize, cols as usize);

    Response::ok(serde_json::json!({
        "cols": cols,
        "rows": rows
    }))
}

fn handle_debug(data: serde_json::Value, state: &Arc<Mutex<DaemonState>>) -> Response {
    let clear = data.get("clear").and_then(|v| v.as_bool()).unwrap_or(false);

    let mut state = state.lock().unwrap();

    let entries = state.terminal.get_debug_entries();
    let dropped = state.terminal.get_debug_dropped();

    if clear {
        state.terminal.clear_debug_buffer();
    }

    // Get terminal mode info from PTY
    let termios_info = match tcgetattr(&state.master_fd) {
        Ok(termios) => {
            let iflags = termios.input_flags;
            let oflags = termios.output_flags;
            let lflags = termios.local_flags;
            let cflags = termios.control_flags;

            // Mode
            let is_canonical = lflags.contains(LocalFlags::ICANON);
            let mode = if is_canonical { "cooked" } else { "raw" };

            // Collect active flags
            let mut flags = Vec::new();
            if lflags.contains(LocalFlags::ECHO) { flags.push("ECHO"); }
            if lflags.contains(LocalFlags::ISIG) { flags.push("ISIG"); }
            if lflags.contains(LocalFlags::IEXTEN) { flags.push("IEXTEN"); }
            if iflags.contains(InputFlags::ICRNL) { flags.push("ICRNL"); }
            if iflags.contains(InputFlags::INLCR) { flags.push("INLCR"); }
            if iflags.contains(InputFlags::IGNCR) { flags.push("IGNCR"); }
            if iflags.contains(InputFlags::IXON) { flags.push("IXON"); }
            if iflags.contains(InputFlags::IXOFF) { flags.push("IXOFF"); }
            if oflags.contains(OutputFlags::OPOST) { flags.push("OPOST"); }
            if oflags.contains(OutputFlags::ONLCR) { flags.push("ONLCR"); }

            // Raw hex values
            let iflag_raw = iflags.bits();
            let oflag_raw = oflags.bits();
            let lflag_raw = lflags.bits();
            let cflag_raw = cflags.bits();

            // Control characters - decode to ^X format
            // 0 = _POSIX_VDISABLE on Linux (control character disabled)
            fn decode_cc(b: u8) -> String {
                match b {
                    0 => String::from("<disabled>"),
                    1..=26 => format!("^{}", (b'A' + b - 1) as char),
                    27 => String::from("^["),
                    28 => String::from("^\\"),
                    29 => String::from("^]"),
                    30 => String::from("^^"),
                    31 => String::from("^_"),
                    127 => String::from("^?"),
                    _ => format!("0x{:02x}", b),
                }
            }

            let c_cc = &termios.control_chars;
            let vintr = decode_cc(c_cc[SpecialCharacterIndices::VINTR as usize]);
            let veof = decode_cc(c_cc[SpecialCharacterIndices::VEOF as usize]);
            let verase = decode_cc(c_cc[SpecialCharacterIndices::VERASE as usize]);
            let vkill = decode_cc(c_cc[SpecialCharacterIndices::VKILL as usize]);
            let vsusp = decode_cc(c_cc[SpecialCharacterIndices::VSUSP as usize]);
            let vquit = decode_cc(c_cc[SpecialCharacterIndices::VQUIT as usize]);

            serde_json::json!({
                "mode": mode,
                "flags": flags,
                "hex": {
                    "iflag": format!("0x{:04x}", iflag_raw),
                    "oflag": format!("0x{:04x}", oflag_raw),
                    "lflag": format!("0x{:04x}", lflag_raw),
                    "cflag": format!("0x{:04x}", cflag_raw)
                },
                "c_cc": {
                    "VINTR": vintr,
                    "VEOF": veof,
                    "VERASE": verase,
                    "VKILL": vkill,
                    "VSUSP": vsusp,
                    "VQUIT": vquit
                }
            })
        }
        Err(e) => {
            serde_json::json!({
                "error": format!("Failed to get termios: {}", e)
            })
        }
    };

    Response::ok(serde_json::json!({
        "unhandled": entries,
        "dropped": dropped,
        "termios": termios_info
    }))
}

fn apply_cursor_inverse(screen: &str, cursor_row: usize, cursor_col: usize) -> String {
    let lines: Vec<&str> = screen.lines().collect();

    // Check if cursor_row is valid
    if cursor_row >= lines.len() {
        return screen.to_string();
    }

    let mut result = String::new();

    for (row_idx, line) in lines.iter().enumerate() {
        if row_idx == cursor_row {
            let chars: Vec<char> = line.chars().collect();

            // Check if cursor_col is valid
            if cursor_col >= chars.len() {
                result.push_str(line);
            } else {
                // Build the line with inverse video at cursor position
                for (col_idx, ch) in chars.iter().enumerate() {
                    if col_idx == cursor_col {
                        result.push_str("\x1b[7m"); // Start inverse video
                        result.push(*ch);
                        result.push_str("\x1b[27m"); // End inverse video
                    } else {
                        result.push(*ch);
                    }
                }
            }
        } else {
            result.push_str(line);
        }

        if row_idx < lines.len() - 1 {
            result.push('\n');
        }
    }

    result
}

fn send_request(socket_path: &str, request: serde_json::Value) -> Result<Response> {
    let mut stream = UnixStream::connect(socket_path)
        .context("Failed to connect to daemon socket")?;

    let json = serde_json::to_string(&request)?;
    stream.write_all(json.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;

    let response: Response = serde_json::from_str(&line)?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_cursor_inverse_basic() {
        let screen = "Hello World\nSecond Line";
        let result = apply_cursor_inverse(screen, 0, 6);

        // Should have inverse codes around character at position 6 (the 'W')
        assert!(result.contains("\x1b[7mW\x1b[27m"), "Should wrap 'W' with inverse codes");
        assert!(result.contains("Hello"));
        assert!(result.contains("orld")); // After the wrapped W
        assert!(result.contains("Second Line"));
    }

    #[test]
    fn test_apply_cursor_inverse_first_char() {
        let screen = "Test";
        let result = apply_cursor_inverse(screen, 0, 0);

        // Should start with inverse code
        assert!(result.starts_with("\x1b[7m"));
        assert!(result.contains("\x1b[27m"));
    }

    #[test]
    fn test_apply_cursor_inverse_multiline() {
        let screen = "Line 1\nLine 2\nLine 3";
        let result = apply_cursor_inverse(screen, 1, 5);

        // Should have all lines
        assert!(result.contains("Line 1"));
        assert!(result.contains("Line ")); // Before wrapped character
        assert!(result.contains("Line 3"));

        // Should have inverse codes wrapping character at position 5 of line 1 (the '2')
        assert!(result.contains("\x1b[7m2\x1b[27m"));
    }

    #[test]
    fn test_apply_cursor_inverse_invalid_row() {
        let screen = "Only one line";
        let result = apply_cursor_inverse(screen, 5, 0);

        // Should return original screen unchanged
        assert_eq!(result, screen);
    }

    #[test]
    fn test_apply_cursor_inverse_invalid_col() {
        let screen = "Short";
        let result = apply_cursor_inverse(screen, 0, 100);

        // Should return original line (no inverse codes)
        assert!(!result.contains("\x1b[7m"));
        assert!(result.contains("Short"));
    }

    #[test]
    fn test_apply_cursor_inverse_empty_screen() {
        let screen = "";
        let result = apply_cursor_inverse(screen, 0, 0);

        // Should handle gracefully
        assert_eq!(result, screen);
    }

    #[test]
    fn test_apply_cursor_inverse_preserves_all_chars() {
        let screen = "ABCDEFGHIJKLMNOP";
        let result = apply_cursor_inverse(screen, 0, 7);

        // Strip ANSI codes
        let stripped = result
            .replace("\x1b[7m", "")
            .replace("\x1b[27m", "");

        // All characters should be preserved
        assert_eq!(stripped, screen);
    }

    #[test]
    fn test_apply_cursor_inverse_last_char() {
        let screen = "Test";
        let result = apply_cursor_inverse(screen, 0, 3);

        // Should end with inverse codes and then the 't'
        assert!(result.contains("\x1b[7mt\x1b[27m"));
    }

    #[test]
    fn test_apply_cursor_inverse_special_chars() {
        let screen = "Hello\tWorld\nNext";
        let result = apply_cursor_inverse(screen, 0, 5);

        // Should handle tab character
        assert!(result.contains("\x1b[7m\t\x1b[27m"));
        assert!(result.contains("Hello"));
        assert!(result.contains("World"));
    }
}

fn main() -> Result<()> {
    // Ignore SIGPIPE to prevent panic when piping to commands like `head`
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Start { socket, size, emulator, no_daemon, pty_dump, command } => {
            cmd_start(socket, size, emulator, !no_daemon, pty_dump, command)?;
        }
        Commands::Input { socket, text, password } => {
            // Priority: --password, --text, stdin
            let input = if password {
                // Fetch current screen to show the password prompt from the application
                let output_request = serde_json::json!({
                    "type": "OUTPUT",
                    "format": "ascii"
                });
                let output_response = send_request(&socket, output_request)?;

                // Show generic guidance, then the cursor line and previous line for context
                eprintln!("Type your secret or password and press Enter.");
                if let Some(data) = output_response.data.as_ref() {
                    let cursor_row = data.get("cursor")
                        .and_then(|c| c.get("row"))
                        .and_then(|r| r.as_u64())
                        .unwrap_or(0) as usize;
                    if let Some(screen) = data.get("screen").and_then(|s| s.as_str()) {
                        let lines: Vec<&str> = screen.lines().collect();
                        // Show previous line if it exists and is non-empty
                        if cursor_row > 0 {
                            if let Some(prev_line) = lines.get(cursor_row - 1) {
                                if !prev_line.trim().is_empty() {
                                    eprintln!("{}", prev_line);
                                }
                            }
                        }
                        // Show cursor line
                        if let Some(prompt_line) = lines.get(cursor_row) {
                            if !prompt_line.trim().is_empty() {
                                eprint!("{} ", prompt_line);
                                std::io::stderr().flush().ok();
                            }
                        }
                    }
                }

                // Read password with echo disabled, append \r for Enter
                let password = rpassword::read_password()
                    .context("Failed to read password (is stdin a terminal?)")?;
                format!("{}\r", password)
            } else if let Some(text_arg) = text {
                unescape(&text_arg)?
            } else {
                let mut buf = String::new();
                std::io::stdin().read_to_string(&mut buf)?;
                buf
            };

            let request = serde_json::json!({
                "type": "INPUT",
                "data": input
            });

            let response = send_request(&socket, request)?;

            if response.status == "error" {
                eprintln!("Error: {}", response.error.unwrap_or_default());
                std::process::exit(1);
            }
        }
        Commands::Output { socket, color, no_color, cursor } => {
            // Default is color (ansi), --no-color disables it
            let format = if no_color { "ascii" } else { "ansi" };
            let _ = color; // --color is just for explicitness, default is already color

            let request = serde_json::json!({
                "type": "OUTPUT",
                "format": format
            });

            let response = send_request(&socket, request)?;

            if response.status == "error" {
                eprintln!("Error: {}", response.error.unwrap_or_default());
                std::process::exit(1);
            }

            if let Some(data) = response.data {
                let cursor_mode = cursor.as_str();

                // Print cursor info if requested (convert to 1-based for display)
                if cursor_mode == "print" || cursor_mode == "both" {
                    if let (Some(cursor_row), Some(cursor_col)) = (
                        data.get("cursor").and_then(|c| c.get("row")).and_then(|v| v.as_u64()),
                        data.get("cursor").and_then(|c| c.get("col")).and_then(|v| v.as_u64())
                    ) {
                        println!("Cursor: row {}, col {}", cursor_row + 1, cursor_col + 1);
                    }
                }

                if let Some(screen) = data.get("screen").and_then(|v| v.as_str()) {
                    // Apply inverse video if requested
                    if cursor_mode == "inverse" || cursor_mode == "both" {
                        if let (Some(cursor_row), Some(cursor_col)) = (
                            data.get("cursor").and_then(|c| c.get("row")).and_then(|v| v.as_u64()),
                            data.get("cursor").and_then(|c| c.get("col")).and_then(|v| v.as_u64())
                        ) {
                            print!("{}", apply_cursor_inverse(screen, cursor_row as usize, cursor_col as usize));
                        } else {
                            print!("{}", screen);
                        }
                    } else {
                        print!("{}", screen);
                    }
                }
            }
        }
        Commands::Status { socket, quiet } => {
            let request = serde_json::json!({
                "type": "STATUS",
                "activity": !quiet
            });

            let response = send_request(&socket, request)?;

            if response.status == "error" {
                eprintln!("Error: {}", response.error.unwrap_or_default());
                std::process::exit(1);
            }

            if let Some(data) = response.data {
                let running = data.get("running").and_then(|v| v.as_bool()).unwrap_or(false);

                if quiet {
                    // Quiet mode: just exit status
                    if running {
                        std::process::exit(0);
                    } else {
                        if let Some(exit_code) = data.get("exit_code") {
                            println!("{}", exit_code);
                        }
                        std::process::exit(1);
                    }
                } else {
                    // Default mode: print all status info
                    println!("Running: {}", running);
                    let has_activity = data.get("activity").and_then(|v| v.as_bool()).unwrap_or(false);
                    println!("Activity: {}", has_activity);
                    if !running {
                        if let Some(exit_code) = data.get("exit_code") {
                            println!("Exit code: {}", exit_code);
                        }
                    }
                }
            }
        }
        Commands::Wait { socket, quiet, line, not_contains, contains } => {
            if let Some(line_num) = line {
                // --line mode: wait until specified line matches condition
                if line_num == 0 {
                    eprintln!("Error: line number must be 1 or greater (1-based)");
                    std::process::exit(1);
                }

                // Helper to get a specific line from screen
                fn get_line(socket: &str, line_num: usize) -> Result<String> {
                    let request = serde_json::json!({
                        "type": "OUTPUT",
                        "format": "ascii"
                    });
                    let response = send_request(socket, request)?;
                    if response.status == "error" {
                        bail!("Failed to get output: {}", response.error.unwrap_or_default());
                    }
                    if let Some(data) = response.data {
                        if let Some(screen) = data.get("screen").and_then(|v| v.as_str()) {
                            let lines: Vec<&str> = screen.lines().collect();
                            // line_num is 1-based, convert to 0-based index
                            let idx = line_num - 1;
                            if idx < lines.len() {
                                return Ok(lines[idx].to_string());
                            } else {
                                return Ok(String::new()); // Line doesn't exist yet
                            }
                        }
                    }
                    Ok(String::new())
                }

                // Determine wait mode based on flags
                let wait_mode = match (&not_contains, &contains) {
                    (Some(pattern), None) => ("not_contains", pattern.clone()),
                    (None, Some(pattern)) => ("contains", pattern.clone()),
                    (Some(_), Some(_)) => {
                        eprintln!("Error: --not-contains and --contains are mutually exclusive");
                        std::process::exit(1);
                    }
                    (None, None) => ("change", String::new()),
                };

                // Get initial line content (only needed for "change" mode)
                let initial_line = if wait_mode.0 == "change" {
                    get_line(&socket, line_num)?
                } else {
                    String::new()
                };

                // Check condition on current line
                let check_condition = |line: &str| -> bool {
                    match wait_mode.0 {
                        "not_contains" => !line.contains(&wait_mode.1),
                        "contains" => line.contains(&wait_mode.1),
                        "change" => line != &initial_line,
                        _ => false,
                    }
                };

                // Check if condition is already met
                let current_line = get_line(&socket, line_num)?;
                if check_condition(&current_line) {
                    println!("Condition already met on line {}", line_num);
                } else {
                    // Wait loop
                    loop {
                        // Wait for activity
                        let wait_request = serde_json::json!({
                            "type": "WAIT",
                            "activity": true
                        });
                        let wait_response = send_request(&socket, wait_request)?;

                        if wait_response.status == "error" {
                            eprintln!("Error: {}", wait_response.error.unwrap_or_default());
                            std::process::exit(1);
                        }

                        // Check if process exited
                        if let Some(data) = &wait_response.data {
                            if data.get("exited").and_then(|v| v.as_bool()).unwrap_or(false) {
                                println!("Application exited");
                                break;
                            }
                        }

                        // Check condition
                        let current_line = get_line(&socket, line_num)?;
                        if check_condition(&current_line) {
                            println!("Condition met on line {}", line_num);
                            break;
                        }
                    }
                }
            } else {
                // Original behavior: single wait
                let request = serde_json::json!({
                    "type": "WAIT",
                    "activity": !quiet
                });

                let response = send_request(&socket, request)?;

                if response.status == "error" {
                    eprintln!("Error: {}", response.error.unwrap_or_default());
                    std::process::exit(1);
                }

                if let Some(data) = response.data {
                    if quiet {
                        // Quiet mode: just print exit code
                        if let Some(exit_code) = data.get("exit_code") {
                            println!("{}", exit_code);
                        }
                    } else {
                        // Default mode: report both terminal activity and exit status
                        let has_activity = data.get("activity").and_then(|v| v.as_bool()).unwrap_or(false);
                        let has_exited = data.get("exited").and_then(|v| v.as_bool()).unwrap_or(false);
                        println!("Terminal activity: {}", if has_activity { "true" } else { "false" });
                        println!("Application exited: {}", if has_exited { "true" } else { "false" });
                    }
                }
            }
        }
        Commands::Kill { socket, signal } => {
            let request = serde_json::json!({
                "type": "KILL",
                "signal": signal
            });

            let response = send_request(&socket, request)?;

            if response.status == "error" {
                eprintln!("Error: {}", response.error.unwrap_or_default());
                std::process::exit(1);
            }
        }
        Commands::Stop { socket } => {
            let request = serde_json::json!({
                "type": "STOP"
            });

            let response = send_request(&socket, request)?;

            if response.status == "error" {
                eprintln!("Error: {}", response.error.unwrap_or_default());
                std::process::exit(1);
            }
        }

        Commands::Resize { socket, size } => {
            // Parse and validate size
            let (cols, rows) = parse_terminal_size(&size)?;

            let request = serde_json::json!({
                "type": "RESIZE",
                "cols": cols,
                "rows": rows
            });

            let response = send_request(&socket, request)?;

            if response.status == "error" {
                eprintln!("Error: {}", response.error.unwrap_or_default());
                std::process::exit(1);
            }

            println!("Terminal resized to {}x{}", cols, rows);
        }

        Commands::Debug { socket, clear } => {
            let request = serde_json::json!({
                "type": "DEBUG",
                "clear": clear
            });

            let response = send_request(&socket, request)?;

            if response.status == "error" {
                eprintln!("Error: {}", response.error.unwrap_or_default());
                std::process::exit(1);
            }

            if let Some(data) = response.data {
                let unhandled = data.get("unhandled").and_then(|v| v.as_array());
                let dropped = data.get("dropped").and_then(|v| v.as_u64()).unwrap_or(0);

                if let Some(entries) = unhandled {
                    if entries.is_empty() {
                        println!("No unhandled escape sequences");
                    } else {
                        println!("Unhandled escape sequences:");
                        for entry in entries {
                            let seq = entry.get("sequence").and_then(|v| v.as_str()).unwrap_or("?");
                            let hex = entry.get("raw_hex").and_then(|v| v.as_str()).unwrap_or("?");
                            println!("  {} ({})", seq, hex);
                        }
                    }
                }

                if dropped > 0 {
                    println!("Dropped: {} (buffer overflow)", dropped);
                }

                // Display termios info
                if let Some(termios) = data.get("termios") {
                    if let Some(error) = termios.get("error").and_then(|v| v.as_str()) {
                        println!("Termios: {}", error);
                    } else {
                        println!("Termios:");

                        // Mode
                        let mode = termios.get("mode").and_then(|v| v.as_str()).unwrap_or("?");
                        println!("  Mode: {}", mode);

                        // Flags
                        if let Some(flags) = termios.get("flags").and_then(|v| v.as_array()) {
                            let flag_strs: Vec<&str> = flags.iter()
                                .filter_map(|v| v.as_str())
                                .collect();
                            if !flag_strs.is_empty() {
                                println!("  Flags: {}", flag_strs.join(" "));
                            }
                        }

                        // Hex values
                        if let Some(hex) = termios.get("hex") {
                            let iflag = hex.get("iflag").and_then(|v| v.as_str()).unwrap_or("?");
                            let oflag = hex.get("oflag").and_then(|v| v.as_str()).unwrap_or("?");
                            let lflag = hex.get("lflag").and_then(|v| v.as_str()).unwrap_or("?");
                            let cflag = hex.get("cflag").and_then(|v| v.as_str()).unwrap_or("?");
                            println!("  Hex: iflag={} oflag={} lflag={} cflag={}", iflag, oflag, lflag, cflag);
                        }

                        // Control characters
                        if let Some(c_cc) = termios.get("c_cc") {
                            let vintr = c_cc.get("VINTR").and_then(|v| v.as_str()).unwrap_or("?");
                            let veof = c_cc.get("VEOF").and_then(|v| v.as_str()).unwrap_or("?");
                            let verase = c_cc.get("VERASE").and_then(|v| v.as_str()).unwrap_or("?");
                            let vkill = c_cc.get("VKILL").and_then(|v| v.as_str()).unwrap_or("?");
                            let vsusp = c_cc.get("VSUSP").and_then(|v| v.as_str()).unwrap_or("?");
                            let vquit = c_cc.get("VQUIT").and_then(|v| v.as_str()).unwrap_or("?");
                            println!("  c_cc: VINTR={} VEOF={} VERASE={} VKILL={} VSUSP={} VQUIT={}",
                                vintr, veof, verase, vkill, vsusp, vquit);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
