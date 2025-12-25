use assert_cmd::Command;
use std::thread;
use std::time::Duration;

mod common;
use common::interminai_bin;

struct TestEnv {
    socket: String,
    _temp_dir: tempfile::TempDir,  // Keep temp dir alive
}

impl TestEnv {
    fn new() -> Self {
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
        let socket = temp_dir.path().join("test.sock").to_string_lossy().to_string();
        Self { 
            socket,
            _temp_dir: temp_dir,
        }
    }

    fn socket(&self) -> &str {
        &self.socket
    }
}

// Drop impl no longer needed - tempfile cleans up automatically

struct DaemonHandle {
    socket: String,
}

impl DaemonHandle {
    fn spawn_with_socket(socket: &str, command: &[&str]) -> Self {
        Command::new(interminai_bin())
            .arg("start")
            .arg("--socket")
            .arg(socket)
            .arg("--")
            .args(command)
            .assert()
            .success();

        thread::sleep(Duration::from_millis(500));

        Self {
            socket: socket.to_string(),
        }
    }

    fn stop(&self) {
        let _ = Command::new(interminai_bin())
            .arg("stop")
            .arg("--socket")
            .arg(&self.socket)
            .output();
    }
}

impl Drop for DaemonHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

fn get_output(socket: &str) -> String {
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(socket)
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");
    
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn test_dsr_cursor_position_response() {
    let env = TestEnv::new();
    
    // Start a simple cat process that echoes back what we send
    let daemon = DaemonHandle::spawn_with_socket(
        &env.socket(),
        &["cat"]
    );
    
    thread::sleep(Duration::from_millis(500));
    
    // Send DSR (Device Status Report) - request cursor position
    // This is ESC [ 6 n
    // Followed by newline to flush cat's output
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--text")
        .arg("\\e[6n\\n")
        .assert()
        .success();
    
    thread::sleep(Duration::from_millis(200));
    
    // Get output - should contain the CPR (Cursor Position Report) response
    // Format: ESC [ {row} ; {col} R
    let output = get_output(&env.socket());
    
    // The response should be echoed back by cat
    // It will appear as visible characters since cat echoes the bytes
    // After sending "\e[6n\n", cursor should be at row 2, col 1 (the newline moved it)
    assert!(output.contains("["), "Should contain bracket from escape sequence. Got: {}", output);
    assert!(output.contains("R"), "Should contain CPR terminator 'R'. Got: {}", output);
    assert!(output.contains("2;1R"), 
            "Should contain cursor position '2;1R' (row 2, col 1 after newline). Got: {}", output);
    
    daemon.stop();
}

#[test]
fn test_dsr_with_vim_text_insertion() {
    let env = TestEnv::new();
    
    // Start vim without a file (opens empty buffer)
    let daemon = DaemonHandle::spawn_with_socket(
        &env.socket(),
        &["vim", "-u", "NONE", "-c", "set nocp"]  // -u NONE = no vimrc, nocp = no compatible mode
    );
    
    thread::sleep(Duration::from_millis(1000));  // Give vim time to start
    
    // Insert some text: press 'i' to enter insert mode, type text, ESC to exit
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--text")
        .arg("iHello World\\nLine 2\\nLine 3\\x1b")  // i = insert mode, text, ESC
        .assert()
        .success();
    
    thread::sleep(Duration::from_millis(300));
    
    // Move to beginning and show position
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--text")
        .arg("gg\\x07")  // gg = go to top, Ctrl-G = show position
        .assert()
        .success();
    
    thread::sleep(Duration::from_millis(300));
    
    let output = get_output(&env.socket());
    
    // Should show the text we typed
    assert!(output.contains("Hello World"), "Should contain 'Hello World'. Got: {}", output);
    assert!(output.contains("Line 2"), "Should contain 'Line 2'. Got: {}", output);
    
    // Ctrl-G should show position info (line 1)
    assert!(output.contains("line 1") || output.contains("1,1"), 
            "Should show position at line 1. Got: {}", output);
    
    daemon.stop();
}

#[test]
fn test_dsr_at_specific_position() {
    let env = TestEnv::new();
    
    // Start cat which will echo everything back
    let daemon = DaemonHandle::spawn_with_socket(
        &env.socket(),
        &["cat"]
    );
    
    thread::sleep(Duration::from_millis(500));
    
    // Send: cursor position command to move to (5, 10), then DSR query, then newline to flush
    // ESC[5;10H = move to row 5, col 10
    // ESC[6n = query cursor position
    // Use stdin piping which we know works from manual testing
    let mut cmd = Command::new(interminai_bin());
    cmd.arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin(b"\x1b[5;10H\x1b[6n\n")
        .assert()
        .success();
    
    thread::sleep(Duration::from_millis(1000));  // Wait for cat to echo DSR response
    
    let output = get_output(&env.socket());
    
    // Should contain the CPR response showing position 5;10
    assert!(output.contains("5;10R"), 
            "Should contain cursor position report '5;10R'. Got: {}", output);
    
    daemon.stop();
}
