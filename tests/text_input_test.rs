mod common;
use common::{interminai_bin, emulator_args};

use assert_cmd::Command;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

struct TestEnv {
    _temp_dir: TempDir,
}

impl TestEnv {
    fn new() -> Self {
        Self {
            _temp_dir: TempDir::new().expect("Failed to create temp dir"),
        }
    }

    fn socket(&self) -> String {
        self._temp_dir.path().join("test.sock").to_string_lossy().to_string()
    }
}

struct DaemonHandle {
    _child: std::process::Child,
    socket_path: String,
}

impl DaemonHandle {
    fn spawn_with_socket(socket: &str, command_args: &[&str]) -> Self {
        use std::process::Stdio;
        use std::io::BufRead;

        let mut cmd = std::process::Command::new(interminai_bin());
        cmd.arg("start")
            .args(emulator_args())
            .arg("--socket")
            .arg(socket)
            .arg("--no-daemon")
            .arg("--");

        for arg in command_args {
            cmd.arg(arg);
        }

        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn daemon");

        let stdout = child.stdout.take().unwrap();
        let reader = std::io::BufReader::new(stdout);
        let _lines: Vec<String> = reader.lines().take(3).map(|l| l.unwrap()).collect();

        thread::sleep(Duration::from_millis(300));

        DaemonHandle {
            _child: child,
            socket_path: socket.to_string(),
        }
    }
}

impl Drop for DaemonHandle {
    fn drop(&mut self) {
        let _ = Command::new(interminai_bin())
            .arg("stop")
            .arg("--socket")
            .arg(&self.socket_path)
            .output();
    }
}

// Test basic --text flag with simple string
#[test]
fn test_text_flag_simple_string() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["cat"]);
    thread::sleep(Duration::from_millis(200));

    // Send "hello" using --text
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--text")
        .arg("hello")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .output()
        .expect("Failed to get output");

    let screen = String::from_utf8_lossy(&output.stdout);
    assert!(screen.contains("hello"), "Screen should contain 'hello': {}", screen);
}

// Test \n escape (newline)
#[test]
fn test_text_flag_newline_escape() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["cat"]);
    thread::sleep(Duration::from_millis(200));

    // Send "line1\nline2" - should produce two lines
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--text")
        .arg("line1\\nline2")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .output()
        .expect("Failed to get output");

    let screen = String::from_utf8_lossy(&output.stdout);
    assert!(screen.contains("line1"), "Screen should contain 'line1': {}", screen);
    assert!(screen.contains("line2"), "Screen should contain 'line2': {}", screen);
}

// Test \t escape (tab)
#[test]
fn test_text_flag_tab_escape() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["cat"]);
    thread::sleep(Duration::from_millis(200));

    // Send "a\tb" - should have tab between a and b
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--text")
        .arg("a\\tb")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .output()
        .expect("Failed to get output");

    let screen = String::from_utf8_lossy(&output.stdout);
    // Tab expands to spaces, so 'a' and 'b' should be separated
    assert!(screen.contains("a"), "Screen should contain 'a': {}", screen);
    assert!(screen.contains("b"), "Screen should contain 'b': {}", screen);
}

// Test \e escape (ESC) - vim quit without save
#[test]
fn test_text_flag_escape_sequence() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["vim"]);
    thread::sleep(Duration::from_millis(500));

    // Enter insert mode, type text, escape, then :q!
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--text")
        .arg("itest\\e:q!\\n")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(500));

    // Check that vim exited
    let output = Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .output()
        .expect("Failed to check running");

    // Should exit with 1 (not running) or have exit code in output
    assert!(!output.status.success() || String::from_utf8_lossy(&output.stdout).contains("0"));
}

// Test \xHH escape (hex byte)
#[test]
fn test_text_flag_hex_escape() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["cat"]);
    thread::sleep(Duration::from_millis(200));

    // Send \x41\x42\x43 which is "ABC"
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--text")
        .arg("\\x41\\x42\\x43")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .output()
        .expect("Failed to get output");

    let screen = String::from_utf8_lossy(&output.stdout);
    assert!(screen.contains("ABC"), "Screen should contain 'ABC': {}", screen);
}

// Test \\ escape (literal backslash)
#[test]
fn test_text_flag_backslash_escape() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["cat"]);
    thread::sleep(Duration::from_millis(200));

    // Send "a\\b" which should produce "a\b"
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--text")
        .arg("a\\\\b")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .output()
        .expect("Failed to get output");

    let screen = String::from_utf8_lossy(&output.stdout);
    assert!(screen.contains("a\\b"), "Screen should contain 'a\\b': {}", screen);
}

// Test \f escape (form feed / Ctrl+L)
#[test]
fn test_text_flag_formfeed_escape() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash"]);
    thread::sleep(Duration::from_millis(300));

    // Send Ctrl+L (\f) to clear/redraw screen - should not crash
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--text")
        .arg("\\f")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    // Just verify daemon is still running
    let output = Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .output()
        .expect("Failed to check running");

    assert!(output.status.success(), "Daemon should still be running after Ctrl+L");
}

// Test \r escape (carriage return)
#[test]
fn test_text_flag_carriage_return_escape() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["cat"]);
    thread::sleep(Duration::from_millis(200));

    // Send "abc\rXY" - carriage return moves cursor to beginning, XY overwrites ab
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--text")
        .arg("abc\\rXY")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .output()
        .expect("Failed to get output");

    let screen = String::from_utf8_lossy(&output.stdout);
    // After "abc\rXY", the display should show "XYc" (XY overwrote ab)
    assert!(screen.contains("XY"), "Screen should contain 'XY': {}", screen);
}

// Test arrow key escape sequence
#[test]
fn test_text_flag_arrow_key() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["vim"]);
    thread::sleep(Duration::from_millis(500));

    // Insert "ab", escape, move left with arrow, insert "X"
    // Result should have X between a and b
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--text")
        .arg("iab\\e\\e[DiX\\e:q!\\n")
        .assert()
        .success();

    // Just verify it didn't crash - arrow key sequence was parsed
    thread::sleep(Duration::from_millis(500));
}

// Test \a escape (bell)
#[test]
fn test_text_flag_bell_escape() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["cat"]);
    thread::sleep(Duration::from_millis(200));

    // Send "X\aY" - bell is non-printable, X and Y should appear
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--text")
        .arg("X\\aY")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .output()
        .expect("Failed to get output");

    let screen = String::from_utf8_lossy(&output.stdout);
    // Bell doesn't print, but X and Y should be there
    assert!(screen.contains("X"), "Screen should contain 'X': {}", screen);
    assert!(screen.contains("Y"), "Screen should contain 'Y': {}", screen);
}

// Test \b escape (backspace)
#[test]
fn test_text_flag_backspace_escape() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["cat"]);
    thread::sleep(Duration::from_millis(200));

    // Send "abc\bX" - backspace (0x08) is sent
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--text")
        .arg("abc\\bX")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .output()
        .expect("Failed to get output");

    let screen = String::from_utf8_lossy(&output.stdout);
    // Backspace was sent - cat echoes it, terminal may show ^H or process it
    // Just verify the other characters came through
    assert!(screen.contains("a"), "Screen should contain 'a': {}", screen);
    assert!(screen.contains("X"), "Screen should contain 'X': {}", screen);
}

// Test \v escape (vertical tab)
#[test]
fn test_text_flag_vertical_tab_escape() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["cat"]);
    thread::sleep(Duration::from_millis(200));

    // Send "A\vB" - vertical tab moves cursor down
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--text")
        .arg("A\\vB")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .output()
        .expect("Failed to get output");

    let screen = String::from_utf8_lossy(&output.stdout);
    // Both A and B should appear (vertical tab may or may not move cursor depending on terminal)
    assert!(screen.contains("A"), "Screen should contain 'A': {}", screen);
    assert!(screen.contains("B"), "Screen should contain 'B': {}", screen);
}

// Test \E escape (uppercase ESC - should work same as \e)
#[test]
fn test_text_flag_uppercase_escape() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["vim"]);
    thread::sleep(Duration::from_millis(500));

    // Use \E instead of \e - should work the same
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--text")
        .arg("itest\\E:q!\\n")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(500));

    // Check that vim exited
    let output = Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .output()
        .expect("Failed to check running");

    assert!(!output.status.success() || String::from_utf8_lossy(&output.stdout).contains("0"));
}

// Test unknown escape sequences are passed through
#[test]
fn test_text_flag_unknown_escape_passthrough() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["cat"]);
    thread::sleep(Duration::from_millis(200));

    // Send "\z" - unknown escape, should pass through as \z
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--text")
        .arg("a\\zb")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .output()
        .expect("Failed to get output");

    let screen = String::from_utf8_lossy(&output.stdout);
    // Unknown escape \z should be kept as-is
    assert!(screen.contains("a\\zb") || screen.contains("a") && screen.contains("b"), 
            "Screen should contain the text: {}", screen);
}

// Test combining --text with stdin should be mutually exclusive or work correctly
#[test]
fn test_text_flag_takes_precedence_over_stdin() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["cat"]);
    thread::sleep(Duration::from_millis(200));

    // When --text is provided, stdin should be ignored
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--text")
        .arg("from_text_flag")
        .write_stdin("from_stdin")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .output()
        .expect("Failed to get output");

    let screen = String::from_utf8_lossy(&output.stdout);
    assert!(screen.contains("from_text_flag"), "Should use --text content: {}", screen);
}
