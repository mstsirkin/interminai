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

// Test wait --activity flag exists
#[test]
fn test_wait_activity_flag_exists() {
    let output = Command::new(interminai_bin())
        .arg("wait")
        .arg("--help")
        .output()
        .expect("Failed to run wait help");

    // Should show --activity in help
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    assert!(
        combined.contains("--activity"),
        "Wait help should mention --activity: {}",
        combined
    );
}

// Test wait --activity reports activity when output is produced
#[test]
fn test_wait_activity_reports_activity_on_output() {
    let env = TestEnv::new();

    // Start bash that outputs something
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "echo hello; sleep 5"]);
    thread::sleep(Duration::from_millis(500));

    // Wait for activity - should return immediately since output was produced
    let output = Command::new(interminai_bin())
        .arg("wait")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--activity")
        .output()
        .expect("Failed to wait for activity");

    assert!(output.status.success(), "Wait --activity should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(stdout.contains("Terminal activity: true"), "Should report activity: got '{}'", stdout);
    assert!(stdout.contains("Application exited: false"), "Should report not exited: got '{}'", stdout);
}

// Test wait --activity reports both activity and exit when command exits
#[test]
fn test_wait_activity_on_command_exit() {
    let env = TestEnv::new();

    // Start a command that exits quickly
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "echo done"]);

    // Wait for command to exit
    thread::sleep(Duration::from_millis(500));

    // Activity should be set because command exited (and output was produced)
    let output = Command::new(interminai_bin())
        .arg("wait")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--activity")
        .output()
        .expect("Failed to wait for activity");

    assert!(output.status.success(), "Wait --activity should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(stdout.contains("Terminal activity: true"), "Should report activity: got '{}'", stdout);
    assert!(stdout.contains("Application exited: true"), "Should report exited: got '{}'", stdout);
}

// Test activity clears after read - second call should block
#[test]
fn test_wait_activity_clears_after_read() {
    let env = TestEnv::new();

    // Start bash that outputs something and then waits
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "echo hello; sleep 10"]);
    thread::sleep(Duration::from_millis(500));

    // First read - should report activity
    let output = Command::new(interminai_bin())
        .arg("wait")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--activity")
        .output()
        .expect("Failed to wait for activity");

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(stdout.contains("Terminal activity: true"), "First read should report activity");

    // Second call should block (timeout because no new activity)
    let output = Command::new(interminai_bin())
        .arg("wait")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--activity")
        .timeout(Duration::from_millis(500))
        .output();

    // Should timeout (not complete)
    assert!(output.is_err() || !output.unwrap().status.success(),
            "Second wait --activity should block/timeout when no new activity");
}

// Test multiple events coalesce
#[test]
fn test_wait_activity_multiple_events_coalesce() {
    let env = TestEnv::new();

    // Output multiple times quickly
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "echo a; echo b; echo c; sleep 5"]);
    thread::sleep(Duration::from_millis(500));

    // First read - should report activity (all events coalesced into one activity flag)
    let output = Command::new(interminai_bin())
        .arg("wait")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--activity")
        .output()
        .expect("Failed to wait for activity");

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(stdout.contains("Terminal activity: true"), "Should report activity for multiple events: got '{}'", stdout);
}

// Test status --activity clears flag after read
#[test]
fn test_status_activity_clears_after_read() {
    let env = TestEnv::new();

    // Start bash that outputs something and then waits for input
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "echo hello; read line; echo got $line"]);
    thread::sleep(Duration::from_millis(500));

    // First status --activity - should report activity: true
    let output = Command::new(interminai_bin())
        .arg("status")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--activity")
        .output()
        .expect("Failed to get status");

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(stdout.contains("Activity: true"), "First status --activity should report true: got '{}'", stdout);

    // Second status --activity - should report activity: false (flag was cleared)
    let output = Command::new(interminai_bin())
        .arg("status")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--activity")
        .output()
        .expect("Failed to get status");

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(stdout.contains("Activity: false"), "Second status --activity should report false after clear: got '{}'", stdout);

    // Send input to trigger more activity
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--text")
        .arg("test\n")
        .output()
        .expect("Failed to send input");

    thread::sleep(Duration::from_millis(300));

    // Third status --activity - should report activity: true again
    let output = Command::new(interminai_bin())
        .arg("status")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--activity")
        .output()
        .expect("Failed to get status");

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(stdout.contains("Activity: true"), "Third status --activity should report true after new output: got '{}'", stdout);
}
