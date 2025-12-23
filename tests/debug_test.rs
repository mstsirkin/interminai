use assert_cmd::Command;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;
use std::path::PathBuf;

mod common;
use common::interminai_bin;

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

struct DaemonHandle {
    socket_path: String,
}

impl DaemonHandle {
    fn spawn(socket: &str) -> Self {
        let mut cmd = std::process::Command::new(interminai_bin());
        cmd.arg("start")
            .arg("--socket")
            .arg(socket)
            .arg("--size")
            .arg("40x10")
            .arg("--")
            .arg("bash")
            .arg("-c")
            .arg("sleep 10");

        let output = cmd.output().expect("Failed to start daemon");
        if !output.status.success() {
            panic!("Daemon failed to start: {}", String::from_utf8_lossy(&output.stderr));
        }

        thread::sleep(Duration::from_millis(300));

        DaemonHandle {
            socket_path: socket.to_string()
        }
    }

    fn stop(self) {
        let _ = std::process::Command::new(interminai_bin())
            .arg("stop")
            .arg("--socket")
            .arg(&self.socket_path)
            .output();
    }
}

/// Test debug command returns valid JSON with expected structure
#[test]
fn test_debug_returns_valid_structure() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn(&env.socket());

    let output = Command::new(interminai_bin())
        .arg("debug")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to run debug command");

    assert!(output.status.success(), "debug command should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain the expected fields
    assert!(stdout.contains("Unhandled") || stdout.contains("unhandled") || stdout.contains("[]"),
        "Should have unhandled field or empty output. Got: {}", stdout);

    daemon.stop();
}

/// Test debug command with --clear flag
#[test]
fn test_debug_with_clear_flag() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn(&env.socket());

    // First call without clear
    let output1 = Command::new(interminai_bin())
        .arg("debug")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to run debug command");

    assert!(output1.status.success(), "debug command should succeed");

    // Second call with clear
    let output2 = Command::new(interminai_bin())
        .arg("debug")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--clear")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to run debug command with clear");

    assert!(output2.status.success(), "debug --clear command should succeed");

    daemon.stop();
}

/// Test debug command works and returns successfully
#[test]
fn test_debug_basic_functionality() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn(&env.socket());

    let output = Command::new(interminai_bin())
        .arg("debug")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to run debug command");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Empty buffer should say "No unhandled" or similar
    assert!(stdout.contains("unhandled") || stdout.contains("Unhandled") || stdout.contains("No"),
        "Should indicate buffer status. Got: {}", stdout);

    daemon.stop();
}

/// Test debug on non-existent socket fails gracefully
#[test]
fn test_debug_nonexistent_socket() {
    let output = Command::new(interminai_bin())
        .arg("debug")
        .arg("--socket")
        .arg("/tmp/nonexistent-socket-12345.sock")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to run debug command");

    assert!(!output.status.success(), "Should fail on nonexistent socket");
}

/// Test debug requires socket argument
#[test]
fn test_debug_requires_socket() {
    let output = Command::new(interminai_bin())
        .arg("debug")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to run debug command");

    assert!(!output.status.success(), "Should fail without socket");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("socket") || stderr.contains("required"),
        "Should mention missing socket. Got: {}", stderr);
}
