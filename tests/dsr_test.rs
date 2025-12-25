use assert_cmd::Command;
use std::thread;
use std::time::Duration;

mod common;
use common::interminai_bin;

struct TestEnv {
    socket: String,
}

impl TestEnv {
    fn new() -> Self {
        let socket = format!("/tmp/interminai-dsr-test-{}.sock", std::process::id());
        Self { socket }
    }

    fn socket(&self) -> &str {
        &self.socket
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket);
    }
}

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
    // The escape character \x1b appears as "^[" in some contexts or is shown as-is
    assert!(output.contains("["), "Should contain bracket from escape sequence. Got: {}", output);
    assert!(output.contains("R"), "Should contain CPR terminator 'R'. Got: {}", output);
    assert!(output.contains("2;1R") || output.contains("1;1R"), 
            "Should contain cursor position like '2;1R' or '1;1R'. Got: {}", output);
    
    daemon.stop();
}
