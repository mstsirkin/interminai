use assert_cmd::Command;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;
use std::path::PathBuf;

mod common;
use common::{interminai_bin, emulator_args};

/// Helper to create a test environment with temporary directory and socket
struct TestEnv {
    _temp_dir: TempDir,
    socket_path: PathBuf,
}

impl TestEnv {
    fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let socket_path = temp_dir.path().join("test.sock");

        Self {
            _temp_dir: temp_dir,
            socket_path,
        }
    }

    fn socket(&self) -> String {
        self.socket_path.to_str().unwrap().to_string()
    }
}

// Helper to spawn daemon in foreground and manage its lifecycle
struct DaemonHandle {
    child: std::process::Child,
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
            child,
            socket_path: socket.to_string()
        }
    }

    fn stop(mut self) {
        let _ = std::process::Command::new(interminai_bin())
            .arg("stop")
            .arg("--socket")
            .arg(&self.socket_path)
            .output();

        thread::sleep(Duration::from_millis(200));
        let _ = self.child.wait();
    }
}

#[test]
fn test_cursor_flag_none() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "echo 'Test'; sleep 10"]);

    thread::sleep(Duration::from_millis(500));

    // Get output with --cursor none
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--cursor")
        .arg("none")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain screen content
    assert!(stdout.contains("Test"));

    // Should NOT contain cursor info line
    assert!(!stdout.contains("Cursor:"));

    // Should NOT contain ANSI inverse codes
    assert!(!stdout.contains("\x1b[7m"));

    daemon.stop();
}

#[test]
fn test_cursor_flag_default_is_none() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "echo 'Default'; sleep 10"]);

    thread::sleep(Duration::from_millis(500));

    // Get output without --cursor flag (should default to none)
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain screen content
    assert!(stdout.contains("Default"));

    // Should NOT contain cursor info line (default is none)
    assert!(!stdout.contains("Cursor:"));

    // Should NOT contain ANSI inverse codes
    assert!(!stdout.contains("\x1b[7m"));

    daemon.stop();
}

#[test]
fn test_cursor_flag_print() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "echo 'Hello World'; sleep 10"]);

    thread::sleep(Duration::from_millis(500));

    // Get output with --cursor print
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--cursor")
        .arg("print")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain cursor info line
    assert!(stdout.contains("Cursor:"));
    assert!(stdout.contains("row"));
    assert!(stdout.contains("col"));

    // Should contain screen content
    assert!(stdout.contains("Hello World"));

    // Should NOT contain ANSI inverse codes (print mode only)
    assert!(!stdout.contains("\x1b[7m"));

    daemon.stop();
}

#[test]
fn test_cursor_flag_inverse() {
    let env = TestEnv::new();
    let test_file = env._temp_dir.path().join("test.txt");
    
    // Create file with known content
    std::fs::write(&test_file, "ABCDEFGH\n").expect("Failed to create test file");
    
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["vim", test_file.to_str().unwrap()]);

    thread::sleep(Duration::from_millis(1000));

    // Move cursor to column 3 (0-indexed, so char 'D')
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("lll") // Move right 3 times
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(300));

    // First verify cursor position
    let check_output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--cursor")
        .arg("print")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");
    
    let check_stdout = String::from_utf8_lossy(&check_output.stdout);
    assert!(check_stdout.contains("row 1, col 4"), "Cursor should be at row 1, col 4. Got: {}", check_stdout.lines().next().unwrap_or(""));

    // Get output with --cursor inverse
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--cursor")
        .arg("inverse")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain screen content (split by inverse codes)
    assert!(stdout.contains("ABC"));
    assert!(stdout.contains("EFGH"));

    // Should contain ANSI inverse codes around character 'D' at position 3
    assert!(stdout.contains("\x1b[7mD\x1b[27m"), "Should contain inverse video around 'D'. Got: {:?}", stdout);

    // Should NOT contain cursor info line (inverse mode only)
    assert!(!stdout.contains("Cursor:"));

    // Cleanup
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin(":q!\n")
        .timeout(Duration::from_secs(2))
        .output()
        .ok();

    daemon.stop();
}

#[test]
fn test_cursor_flag_both() {
    let env = TestEnv::new();
    let test_file = env._temp_dir.path().join("both.txt");
    
    // Create file with known content
    std::fs::write(&test_file, "Both modes test\n").expect("Failed to create test file");
    
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["vim", test_file.to_str().unwrap()]);

    thread::sleep(Duration::from_millis(1000));

    // Move cursor to column 5 (char 'm')
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("lllll") // Move right 5 times
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(300));

    // Get output with --cursor both
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--cursor")
        .arg("both")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain screen content (split by inverse codes)
    assert!(stdout.contains("Both "));
    assert!(stdout.contains("odes test"));

    // Should contain cursor info line
    assert!(stdout.contains("Cursor:"));
    assert!(stdout.contains("row 1"));
    assert!(stdout.contains("col 6"));

    // Should contain ANSI inverse codes around character 'm' at position 5
    assert!(stdout.contains("\x1b[7m"), "Should contain inverse video start code");
    assert!(stdout.contains("\x1b[27m"), "Should contain inverse video end code");

    // Cleanup
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin(":q!\n")
        .timeout(Duration::from_secs(2))
        .output()
        .ok();

    daemon.stop();
}

#[test]
fn test_cursor_position_reported() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "echo -n 'Test'; sleep 10"]);

    thread::sleep(Duration::from_millis(500));

    // Get output with --cursor print to see position
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--cursor")
        .arg("print")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should report cursor position with row and col numbers
    assert!(stdout.contains("Cursor: row"));
    
    // Extract and verify the cursor line format
    let cursor_line = stdout.lines().find(|l| l.contains("Cursor:")).expect("Should have cursor line");
    assert!(cursor_line.contains("row"), "Cursor line should contain 'row'");
    assert!(cursor_line.contains("col"), "Cursor line should contain 'col'");

    daemon.stop();
}

#[test]
fn test_cursor_with_interactive_session() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["cat"]);

    // Send some input
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("Hello")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(300));

    // Get output with cursor info
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--cursor")
        .arg("print")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should show cursor position
    assert!(stdout.contains("Cursor:"));
    
    // Should show the echoed input
    assert!(stdout.contains("Hello"));

    daemon.stop();
}

#[test]
fn test_cursor_inverse_does_not_obscure_text() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "echo 'ABCDEFGH'; sleep 10"]);

    thread::sleep(Duration::from_millis(500));

    // Get output with inverse cursor
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--cursor")
        .arg("inverse")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Strip ANSI codes to verify all characters are present
    let stripped: String = stdout
        .chars()
        .filter(|c| !c.is_ascii_control() || *c == '\n' || *c == ' ')
        .collect::<String>()
        .replace("\x1b[7m", "")
        .replace("\x1b[27m", "");

    // All original characters should still be present
    assert!(stripped.contains("ABCDEFGH"), "All characters should be preserved with inverse cursor");

    daemon.stop();
}

#[test]
fn test_cursor_print_appears_before_screen() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "echo 'Screen Content'; sleep 10"]);

    thread::sleep(Duration::from_millis(500));

    // Get output with --cursor print
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--cursor")
        .arg("print")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    // First line should be the cursor info
    assert!(lines.len() >= 2, "Should have at least cursor line and content");
    assert!(lines[0].contains("Cursor:"), "First line should be cursor info");
    
    // Screen content should come after
    let has_screen_content = lines.iter().skip(1).any(|line| line.contains("Screen Content"));
    assert!(has_screen_content, "Screen content should appear after cursor line");

    daemon.stop();
}

#[test]
fn test_cursor_with_multiline_output() {
    let env = TestEnv::new();
    let test_file = env._temp_dir.path().join("multiline.txt");
    
    // Create file with multiple lines
    std::fs::write(&test_file, "Line 1\nLine 2\nLine 3\n").expect("Failed to create test file");
    
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["vim", test_file.to_str().unwrap()]);

    thread::sleep(Duration::from_millis(1000));

    // Move to line 2, column 5 (char 'e' in "Line 2")
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("jlllll") // j=down, then 5x right
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(300));

    // Verify cursor position first
    let check_output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--cursor")
        .arg("print")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");
    
    let check_stdout = String::from_utf8_lossy(&check_output.stdout);
    assert!(check_stdout.contains("row 2, col 6"), "Cursor should be at row 2, col 6. Got: {}", check_stdout.lines().next().unwrap_or(""));

    // Get output with cursor both
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--cursor")
        .arg("both")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should have cursor info
    assert!(stdout.contains("Cursor:"));
    assert!(stdout.contains("row 2, col 6"));

    // Should have all lines (Line 2 will be split by inverse codes)
    assert!(stdout.contains("Line 1"));
    assert!(stdout.contains("Line "));  // Before inverse code
    assert!(stdout.contains("Line 3"));

    // Should have inverse codes around the character at cursor position
    assert!(stdout.contains("\x1b[7m"));
    assert!(stdout.contains("\x1b[27m"));

    // Cleanup
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin(":q!\n")
        .timeout(Duration::from_secs(2))
        .output()
        .ok();

    daemon.stop();
}

#[test]
fn test_dsr_cursor_position_query_responds() {
    // Test that ESC[6n (Device Status Report - cursor position query) gets a proper response
    // This is critical for programs like codex that query cursor position on startup
    let env = TestEnv::new();

    // Use bash with a script that:
    // 1. Sends ESC[6n to query cursor position
    // 2. Reads the response with a timeout
    // 3. Prints whether it got a valid response
    // The response format is ESC[row;colR
    let daemon = DaemonHandle::spawn_with_socket(
        &env.socket(),
        &["bash", "-c", r#"
            # Query cursor position by sending ESC[6n
            printf '\033[6n'
            # Read response with 1 second timeout
            # The response is ESC[row;colR
            if read -r -t 1 -d 'R' response; then
                echo "GOT_RESPONSE:$response"
            else
                echo "NO_RESPONSE"
            fi
            sleep 5
        "#]
    );

    // Wait for the script to execute
    thread::sleep(Duration::from_millis(1500));

    // Get output
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should have received a response (not NO_RESPONSE)
    assert!(stdout.contains("GOT_RESPONSE"),
        "Should receive DSR response. Got: {}", stdout);

    // Should NOT show NO_RESPONSE
    assert!(!stdout.contains("NO_RESPONSE"),
        "DSR query should not timeout. Got: {}", stdout);

    daemon.stop();
}

#[test]
fn test_dsr_device_status_query_responds() {
    // Test that ESC[5n (Device Status Report - device status) gets a proper response
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(
        &env.socket(),
        &["bash", "-c", r#"
            # Query device status by sending ESC[5n
            printf '\033[5n'
            # Read response with 1 second timeout
            # The response is ESC[0n (ready, no malfunction)
            if read -r -t 1 -d 'n' response; then
                echo "GOT_DSR5:$response"
            else
                echo "NO_RESPONSE"
            fi
            sleep 5
        "#]
    );

    thread::sleep(Duration::from_millis(1500));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("GOT_DSR5"),
        "Should receive DSR 5 response. Got: {}", stdout);
    assert!(!stdout.contains("NO_RESPONSE"),
        "DSR 5 query should not timeout. Got: {}", stdout);

    daemon.stop();
}

#[test]
fn test_primary_device_attributes_responds() {
    // Test that ESC[c (Primary Device Attributes) gets a proper response
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(
        &env.socket(),
        &["bash", "-c", r#"
            # Query device attributes by sending ESC[c
            printf '\033[c'
            # Read response with 1 second timeout
            # The response is ESC[?1;2c (VT100 with AVO)
            if read -r -t 1 -d 'c' response; then
                echo "GOT_DA:$response"
            else
                echo "NO_RESPONSE"
            fi
            sleep 5
        "#]
    );

    thread::sleep(Duration::from_millis(1500));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("GOT_DA"),
        "Should receive DA response. Got: {}", stdout);
    assert!(!stdout.contains("NO_RESPONSE"),
        "DA query should not timeout. Got: {}", stdout);

    daemon.stop();
}
