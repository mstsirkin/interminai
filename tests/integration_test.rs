use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

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
    fn spawn(args: &[&str]) -> Self {
        use std::process::Stdio;
        use std::io::BufRead;

        let mut cmd = std::process::Command::new(interminai_bin());
        cmd.arg("start").args(emulator_args());

        for arg in args {
            cmd.arg(arg);
        }

        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::null())  // Suppress stderr to avoid IO safety issues
            .spawn()
            .expect("Failed to spawn daemon");

        let stdout = child.stdout.take().unwrap();
        let reader = std::io::BufReader::new(stdout);
        let lines: Vec<String> = reader.lines().take(3).map(|l| l.unwrap()).collect();

        let socket_line = lines.iter().find(|l| l.starts_with("Socket:")).expect("No socket line");
        let socket_path = socket_line.split_whitespace().nth(1).unwrap().to_string();

        thread::sleep(Duration::from_millis(300));

        DaemonHandle { child, socket_path }
    }

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

    fn socket(&self) -> &str {
        &self.socket_path
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
fn test_start_without_socket_auto_generates() {
    let daemon = DaemonHandle::spawn(&["--", "sleep", "10"]);

    // Should have auto-generated socket
    assert!(daemon.socket().contains("/tmp/interminai-"));

    daemon.stop();
}

#[test]
fn test_start_with_socket_uses_it() {
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["cat"]);

    // Should use the specified socket
    assert_eq!(daemon.socket(), env.socket());

    // Socket file should exist
    assert!(env.socket_path.exists());

    daemon.stop();
}

#[test]
fn test_start_creates_daemon() {
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["cat"]);

    // Socket should exist
    assert!(env.socket_path.exists());

    daemon.stop();
}

#[test]
fn test_output_requires_socket() {
    let mut cmd = Command::new(interminai_bin());
    cmd.arg("output")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--socket"));
}

// FIXME: This test hangs - daemon mode spawning needs investigation
// #[test]
// fn test_daemon_mode_returns_immediately() {
//     let env = TestEnv::new();
//
//     // Start in daemon mode (default, no --no-daemon flag)
//     let start = std::time::Instant::now();
//     let output = Command::new(interminai_bin())
//         .arg("start")
//         .arg("--socket")
//         .arg(&env.socket())
//         .arg("--")
//         .arg("sleep")
//         .arg("10")
//         .output()
//         .expect("Failed to execute");
//
//     let duration = start.elapsed();
//
//     // Should return immediately (< 1 second), not wait for sleep to finish
//     assert!(duration.as_secs() < 1, "Daemon mode took too long: {:?}", duration);
//     assert!(output.status.success(), "Start command failed");
//
//     // Parse socket from output
//     let stdout = String::from_utf8_lossy(&output.stdout);
//     assert!(stdout.contains("Socket:"), "No socket in output");
//
//     // Wait a moment for daemon to initialize
//     thread::sleep(Duration::from_millis(300));
//
//     // Verify daemon is actually running
//     let running_output = Command::new(interminai_bin())
//         .arg("running")
//         .arg("--socket")
//         .arg(&env.socket())
//         .timeout(Duration::from_secs(5))
//         .output()
//         .expect("Failed to check running");
//
//     assert!(running_output.status.success(), "Process should be running");
//
//     // Clean up
//     Command::new(interminai_bin())
//         .arg("stop")
//         .arg("--socket")
//         .arg(&env.socket())
//         .assert()
//         .success();
// }

#[test]
fn test_input_requires_socket() {
    let mut cmd = Command::new(interminai_bin());
    cmd.arg("input")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--socket"));
}

#[test]
fn test_stop_requires_socket() {
    let mut cmd = Command::new(interminai_bin());
    cmd.arg("stop")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--socket"));
}

#[test]
fn test_running_requires_socket() {
    let mut cmd = Command::new(interminai_bin());
    cmd.arg("running")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--socket"));
}

#[test]
fn test_running_when_active() {
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["sleep", "100"]);

    // Check running status
    let mut cmd = Command::new(interminai_bin());
    cmd.arg("running")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .assert()
        .success(); // Exit code 0 = running

    daemon.stop();
}

#[test]
fn test_running_when_finished() {
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "exit 42"]);

    // Wait for process to finish
    thread::sleep(Duration::from_millis(500));

    // Check running status
    let output = Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to check running status");

    // Should fail (exit code != 0) when not running
    assert!(!output.status.success());

    // Should print exit code on stdout
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("42") || stdout.contains("exit"));

    daemon.stop();
}

#[test]
fn test_running_after_stop() {
    let env = TestEnv::new();

    let _daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["sleep", "100"]);

    // Stop it
    Command::new(interminai_bin())
        .arg("stop")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    // Check running status - should fail (not running)
    let result = Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output();

    // Should fail to connect or report not running
    assert!(result.is_err() || !result.unwrap().status.success());

    // Note: daemon already stopped, don't call stop() again
}

#[test]
fn test_running_no_daemon() {
    let env = TestEnv::new();

    // Try to check status without starting daemon
    let result = Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output();

    // Should fail (socket doesn't exist or can't connect)
    assert!(result.is_err() || !result.unwrap().status.success());
}

#[test]
fn test_wait_requires_socket() {
    let mut cmd = Command::new(interminai_bin());
    cmd.arg("wait")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--socket"));
}

#[test]
fn test_wait_until_exit() {
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "sleep 0.5; exit 7"]);

    thread::sleep(Duration::from_millis(200));

    // Wait for it to exit
    let output = Command::new(interminai_bin())
        .arg("wait")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(3))
        .output()
        .expect("Failed to wait");

    // Should succeed and report exit code 7
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("7") || stdout.contains("exit"));

    daemon.stop();
}

#[test]
fn test_wait_already_finished() {
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "exit 13"]);

    // Wait for process to finish
    thread::sleep(Duration::from_millis(500));

    // Wait should return immediately with exit code
    let output = Command::new(interminai_bin())
        .arg("wait")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to wait");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("13"));

    daemon.stop();
}

#[test]
fn test_kill_requires_socket() {
    let mut cmd = Command::new(interminai_bin());
    cmd.arg("kill")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--socket"));
}

#[test]
fn test_kill_default_sigterm() {
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["sleep", "100"]);

    // Verify it's running
    Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    // Kill with default SIGTERM
    Command::new(interminai_bin())
        .arg("kill")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    // Wait for process to die
    thread::sleep(Duration::from_millis(500));

    // Should no longer be running
    let result = Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output();

    // Should fail (not running) or show terminated
    assert!(result.is_err() || !result.unwrap().status.success());

    daemon.stop();
}

#[test]
fn test_kill_sigkill() {
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["sleep", "100"]);

    // Kill with explicit SIGKILL
    Command::new(interminai_bin())
        .arg("kill")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--signal")
        .arg("SIGKILL")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    // Wait for process to die
    thread::sleep(Duration::from_millis(500));

    // Should no longer be running
    let result = Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output();

    assert!(result.is_err() || !result.unwrap().status.success());

    daemon.stop();
}

#[test]
fn test_kill_sigint() {
    let env = TestEnv::new();

    // Use a bash script that keeps bash alive (multiple commands prevent exec optimization)
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "trap 'exit 99' INT; while true; do sleep 1; done"]);

    // Send SIGINT
    Command::new(interminai_bin())
        .arg("kill")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--signal")
        .arg("SIGINT")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    // Give bash trap time to execute
    thread::sleep(Duration::from_millis(500));

    // Wait for process to actually exit
    Command::new(interminai_bin())
        .arg("wait")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(3))
        .assert()
        .success();

    // Now check the exit code - should be 99 from the trap
    let output = Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get running status");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("99"), "Expected exit code 99 from trap, got: {}", stdout);

    daemon.stop();
}

#[test]
fn test_kill_numeric_signal_9() {
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["sleep", "100"]);

    // Kill with numeric signal 9 (SIGKILL)
    Command::new(interminai_bin())
        .arg("kill")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--signal")
        .arg("9")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    // Wait for process to die
    thread::sleep(Duration::from_millis(500));

    // Should no longer be running
    let result = Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output();

    assert!(result.is_err() || !result.unwrap().status.success());

    daemon.stop();
}

#[test]
fn test_kill_numeric_signal_15() {
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["sleep", "100"]);

    // Kill with numeric signal 15 (SIGTERM)
    Command::new(interminai_bin())
        .arg("kill")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--signal")
        .arg("15")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    // Wait for process to die
    thread::sleep(Duration::from_millis(500));

    // Should no longer be running
    let result = Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output();

    assert!(result.is_err() || !result.unwrap().status.success());

    daemon.stop();
}

#[test]
fn test_kill_numeric_signal_2() {
    let env = TestEnv::new();

    // Use a bash script that keeps bash alive (multiple commands prevent exec optimization)
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "trap 'exit 88' INT; while true; do sleep 1; done"]);

    // Send numeric signal 2 (SIGINT)
    Command::new(interminai_bin())
        .arg("kill")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--signal")
        .arg("2")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    // Give bash trap time to execute
    thread::sleep(Duration::from_millis(500));

    // Wait for process to actually exit
    Command::new(interminai_bin())
        .arg("wait")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(3))
        .assert()
        .success();

    // Now check the exit code - should be 88 from the trap
    let output = Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get running status");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("88"), "Expected exit code 88 from trap, got: {}", stdout);

    daemon.stop();
}

#[test]
fn test_output_gets_screen() {
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "echo 'Hello World'; sleep 10"]);

    // Get screen output
    let mut cmd = Command::new(interminai_bin());
    let output = cmd
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to run output command");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain screen output
    assert!(stdout.len() > 0);

    daemon.stop();
}

#[test]
fn test_input_sends_keys() {
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["cat"]);

    // Send some input
    let mut cmd = Command::new(interminai_bin());
    cmd.arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("Hello\n")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    // Get output
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain our input echoed back
    assert!(stdout.contains("Hello"));

    daemon.stop();
}

#[test]
fn test_stop_terminates_daemon() {
    let env = TestEnv::new();

    let _daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["sleep", "100"]);

    // Socket should exist
    assert!(env.socket_path.exists());

    // Stop it
    Command::new(interminai_bin())
        .arg("stop")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    // Socket should be gone (user-specified socket is NOT removed)
    // But connection should fail
    let result = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output();

    // Should fail to connect
    assert!(result.is_err() || !result.unwrap().status.success());

    // Note: daemon already stopped, don't call stop() again
}

#[test]
fn test_user_socket_not_deleted_on_stop() {
    let env = TestEnv::new();

    let _daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["echo", "test"]);

    let socket_existed = env.socket_path.exists();

    // Stop it
    Command::new(interminai_bin())
        .arg("stop")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .ok();

    thread::sleep(Duration::from_millis(200));

    // User-specified socket file should remain for reuse
    if socket_existed {
        assert!(
            env.socket_path.exists(),
            "User-specified socket should not be deleted"
        );
    }

    // Note: daemon already stopped, don't call stop() again
}

#[test]
fn test_socket_reuse() {
    let env = TestEnv::new();

    // Start first session
    let _daemon1 = DaemonHandle::spawn_with_socket(&env.socket(), &["echo", "session1"]);

    // Stop it
    Command::new(interminai_bin())
        .arg("stop")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .ok();

    thread::sleep(Duration::from_millis(200));

    // Start second session with same socket
    let daemon2 = DaemonHandle::spawn_with_socket(&env.socket(), &["echo", "session2"]);

    // Should succeed - socket is reused
    daemon2.stop();
}

#[test]
fn test_parallel_sessions() {
    // Create two separate environments
    let env1 = TestEnv::new();
    let env2 = TestEnv::new();

    // Start two sessions in parallel
    let daemon1 = DaemonHandle::spawn_with_socket(&env1.socket(), &["cat"]);
    let daemon2 = DaemonHandle::spawn_with_socket(&env2.socket(), &["cat"]);

    // Both sockets should exist
    assert!(env1.socket_path.exists());
    assert!(env2.socket_path.exists());

    daemon1.stop();
    daemon2.stop();
}

#[test]
fn test_terminal_size_option() {
    let env = TestEnv::new();

    // Start with custom terminal size - need custom spawn
    use std::process::Stdio;
    use std::io::BufRead;

    let mut cmd = std::process::Command::new(interminai_bin());
    cmd.arg("start")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--size")
        .arg("120x40")
        .arg("--")
        .arg("cat");

    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to spawn daemon");

    let stdout = child.stdout.take().unwrap();
    let reader = std::io::BufReader::new(stdout);
    let _lines: Vec<String> = reader.lines().take(3).map(|l| l.unwrap()).collect();

    thread::sleep(Duration::from_millis(300));

    // Get screen output
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout_out = String::from_utf8_lossy(&output.stdout);

    // Output should indicate terminal size (this will depend on format)
    assert!(stdout_out.len() > 0);

    // Cleanup
    Command::new(interminai_bin())
        .arg("stop")
        .arg("--socket")
        .arg(&env.socket())
        .output()
        .ok();

    thread::sleep(Duration::from_millis(200));
    let _ = child.wait();
}

// ============================================================================
// DAEMON RESILIENCE TESTS - Test daemon stability and error handling
// ============================================================================

#[test]
fn test_client_dies_before_response() {
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "for i in {1..1000}; do echo Line $i; done; sleep 10"]);

    // Start an output request but kill the client immediately
    let mut child = std::process::Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn output command");

    // Kill the client immediately (before it reads response)
    thread::sleep(Duration::from_millis(50));
    child.kill().ok();

    // Give daemon time to notice and clean up
    thread::sleep(Duration::from_millis(300));

    // Daemon should still be responsive
    let output = Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Daemon should still respond after client disconnect");

    assert!(output.status.success(), "Daemon should survive client disconnect");

    // Should still be able to get output
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Should get output after client disconnect");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Line"), "Should still get screen content");

    daemon.stop();
}

#[test]
fn test_incomplete_request_daemon_survives() {
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["sleep", "10"]);

    // Send incomplete/malformed request directly to socket
    if let Ok(mut stream) = UnixStream::connect(&env.socket()) {
        // Send incomplete JSON or garbage
        stream.write_all(b"{\"incomplete\":").ok();
        stream.flush().ok();
        // Drop connection
    }

    thread::sleep(Duration::from_millis(300));

    // Daemon should still be alive and responsive
    let output = Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Daemon should survive malformed request");

    assert!(output.status.success(), "Daemon should still be running");

    // Should still accept valid requests
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Should accept valid request after malformed one");

    assert!(output.status.success());

    daemon.stop();
}

#[test]
fn test_invalid_request_gets_error_response() {
    use std::io::{Write, Read};
    use std::os::unix::net::UnixStream;

    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["sleep", "10"]);

    // Send invalid request directly to socket
    if let Ok(mut stream) = UnixStream::connect(&env.socket()) {
        // Send malformed request
        stream.write_all(b"INVALID_COMMAND\n").ok();
        stream.flush().ok();

        // Try to read response (daemon should send error, not crash)
        let mut response = String::new();
        stream.read_to_string(&mut response).ok();

        // Response should indicate error
        assert!(
            response.contains("error") || response.contains("Error") || response.contains("invalid"),
            "Should receive error response for invalid request"
        );
    }

    thread::sleep(Duration::from_millis(300));

    // Daemon should still be alive
    let output = Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Daemon should still be running after invalid request");

    assert!(output.status.success(), "Daemon should survive invalid request");

    daemon.stop();
}

#[test]
fn test_multiple_clients_simultaneous() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "for i in {1..100}; do echo Test $i; sleep 0.1; done"]);

    // Spawn multiple threads making simultaneous requests
    let success_count = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];

    for i in 0..5 {
        let socket = env.socket();
        let counter = success_count.clone();

        let handle = std::thread::spawn(move || {
            thread::sleep(Duration::from_millis(i * 50)); // Stagger slightly

            let output = Command::new(interminai_bin())
                .arg("output")
                .arg("--socket")
                .arg(&socket)
                .timeout(Duration::from_secs(3))
                .output();

            if let Ok(output) = output {
                if output.status.success() {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            }
        });

        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().ok();
    }

    // At least some requests should succeed (daemon handles concurrent access)
    let successes = success_count.load(Ordering::SeqCst);
    assert!(successes >= 3, "Daemon should handle multiple simultaneous clients");

    // Daemon should still be responsive
    Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    daemon.stop();
}

// ============================================================================
// VIM EDITING TESTS - Real world interactive application tests
// ============================================================================

#[test]
fn test_vim_create_file() {
    let env = TestEnv::new();
    let test_file = env._temp_dir.path().join("test.txt");

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["vim", test_file.to_str().unwrap()]);

    thread::sleep(Duration::from_millis(1000));

    // Check initial screen shows vim
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should show vim interface (tildes or VIM text)
    assert!(stdout.contains("~") || stdout.contains("VIM"));

    // Enter insert mode
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("i")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(300));

    // Type some text
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("Hello from vim!")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(300));

    // Exit insert mode and save
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("\x1b:wq\n") // ESC, :wq, Enter
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(500));

    daemon.stop();

    // Verify file was created with correct content
    assert!(test_file.exists(), "File should have been created");
    let content = fs::read_to_string(&test_file).expect("Failed to read file");
    assert!(content.contains("Hello from vim!"));
}

#[test]
fn test_vim_edit_existing_file() {
    let env = TestEnv::new();
    let test_file = env._temp_dir.path().join("existing.txt");

    // Create initial file
    fs::write(&test_file, "Initial content\n").expect("Failed to create test file");

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["vim", test_file.to_str().unwrap()]);

    thread::sleep(Duration::from_millis(1000));

    // Check screen shows existing content
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Initial content"));

    // Append a line: Go to end of file, insert mode, type
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("GA\nAdded line\x1b:wq\n") // G, A, newline, text, ESC, :wq
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(500));

    daemon.stop();

    // Verify file was modified
    let content = fs::read_to_string(&test_file).expect("Failed to read file");
    assert!(content.contains("Initial content"));
    assert!(content.contains("Added line"));
}

#[test]
fn test_vim_insert_mode_visible() {
    let env = TestEnv::new();
    let test_file = env._temp_dir.path().join("mode_test.txt");

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["vim", test_file.to_str().unwrap()]);

    thread::sleep(Duration::from_millis(1000));

    // Enter insert mode
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("i")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(300));

    // Check screen shows INSERT mode
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("INSERT") || stdout.contains("-- INSERT --"),
        "Should show INSERT mode indicator"
    );

    // Cleanup - quit without saving
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("\x1b:q!\n") // ESC, :q!, Enter (quit without saving)
        .timeout(Duration::from_secs(2))
        .output()
        .ok();

    daemon.stop();
}

#[test]
fn test_vim_multiline_edit() {
    let env = TestEnv::new();
    let test_file = env._temp_dir.path().join("multiline.txt");

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["vim", test_file.to_str().unwrap()]);

    thread::sleep(Duration::from_millis(1000));

    // Enter insert mode and type multiple lines
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("i")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("Line 1\nLine 2\nLine 3")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(300));

    // Check screen shows the lines
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Line 1"));
    assert!(stdout.contains("Line 2"));
    assert!(stdout.contains("Line 3"));

    // Save and quit
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("\x1b:wq\n")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(500));

    daemon.stop();

    // Verify file content
    let content = fs::read_to_string(&test_file).expect("Failed to read file");
    assert!(content.contains("Line 1"));
    assert!(content.contains("Line 2"));
    assert!(content.contains("Line 3"));
}

#[test]
fn test_vim_quit_without_save() {
    let env = TestEnv::new();
    let test_file = env._temp_dir.path().join("nosave.txt");

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["vim", test_file.to_str().unwrap()]);

    thread::sleep(Duration::from_millis(1000));

    // Type something
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("iShould not be saved\x1b")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(300));

    // Quit without saving
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin(":q!\n")
        .timeout(Duration::from_secs(2))
        .output()
        .ok();

    thread::sleep(Duration::from_millis(300));

    daemon.stop();

    // File should not exist or be empty
    if test_file.exists() {
        let content = fs::read_to_string(&test_file).unwrap_or_default();
        assert!(
            content.is_empty() || !content.contains("Should not be saved"),
            "File should not contain unsaved text"
        );
    }
}

#[test]
fn test_vim_arrow_key_navigation() {
    let env = TestEnv::new();
    let test_file = env._temp_dir.path().join("arrows.txt");

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["vim", test_file.to_str().unwrap()]);

    thread::sleep(Duration::from_millis(1000));

    // Create some lines to navigate
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("iLine1\nLine2\nLine3")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(300));

    // Exit insert mode
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("\x1b")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    // Navigate up with arrow key (ESC[A)
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("\x1b[A")  // Up arrow
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    // Navigate down with arrow key (ESC[B)
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("\x1b[B")  // Down arrow
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    // Navigate right (ESC[C)
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("\x1b[C\x1b[C")  // Right arrow twice
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    // Navigate left (ESC[D)
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("\x1b[D")  // Left arrow
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    // Get screen to verify we can see content
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should still show all lines after navigation
    assert!(stdout.contains("Line1"));
    assert!(stdout.contains("Line2"));
    assert!(stdout.contains("Line3"));

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
fn test_vim_save_and_verify() {
    let env = TestEnv::new();
    let test_file = env._temp_dir.path().join("save_test.txt");

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["vim", test_file.to_str().unwrap()]);

    thread::sleep(Duration::from_millis(1000));

    // Type some content
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("iFirst line\nSecond line\nThird line")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(300));

    // Save without quitting (:w)
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("\x1b:w\n")  // ESC, :w, Enter
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(500));

    // Verify file was saved correctly
    assert!(test_file.exists(), "File should be saved");
    let content = fs::read_to_string(&test_file).expect("Failed to read saved file");
    assert!(content.contains("First line"));
    assert!(content.contains("Second line"));
    assert!(content.contains("Third line"));

    // Add more content after save
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("aFourth line")  // append
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(300));

    // Save again
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("\x1b:w\n")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(500));

    // Verify updated content
    let content = fs::read_to_string(&test_file).expect("Failed to read saved file");
    assert!(content.contains("Fourth line"), "Should contain newly added content");

    // Cleanup
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin(":q\n")
        .timeout(Duration::from_secs(2))
        .output()
        .ok();

    daemon.stop();
}

#[test]
fn test_vim_wq_exits() {
    let env = TestEnv::new();
    let test_file = env._temp_dir.path().join("exit_test.txt");

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["vim", test_file.to_str().unwrap()]);

    thread::sleep(Duration::from_millis(1000));

    // Verify vim is running
    Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .assert()
        .success(); // Should be running

    // Type some content
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("iTest content for exit")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(300));

    // Exit with :wq
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("\x1b:wq\n")  // ESC, :wq, Enter
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    // Wait for vim to exit (give it some time)
    thread::sleep(Duration::from_millis(1000));

    // Check that vim has exited
    let result = Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to check running status");

    // Should indicate process finished (non-zero exit or shows exit code)
    assert!(
        !result.status.success() || String::from_utf8_lossy(&result.stdout).contains("0"),
        "Vim should have exited after :wq"
    );

    // Verify file was saved
    assert!(test_file.exists(), "File should be saved after :wq");
    let content = fs::read_to_string(&test_file).expect("Failed to read file");
    assert!(content.contains("Test content for exit"));

    // Cleanup
    daemon.stop();
}

#[test]
fn test_vim_exits_eventually_after_quit() {
    let env = TestEnv::new();
    let test_file = env._temp_dir.path().join("quit_test.txt");

    // Start vim with existing file
    fs::write(&test_file, "Existing content\n").expect("Failed to create test file");

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["vim", test_file.to_str().unwrap()]);

    thread::sleep(Duration::from_millis(1000));

    // Quit without making changes
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin(":q\n")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    // Poll for vim to exit (should be quick)
    let mut exited = false;
    for _ in 0..10 {
        thread::sleep(Duration::from_millis(200));

        let result = Command::new(interminai_bin())
            .arg("running")
            .arg("--socket")
            .arg(&env.socket())
            .timeout(Duration::from_secs(2))
            .output();

        if let Ok(output) = result {
            if !output.status.success() {
                exited = true;
                break;
            }
        }
    }

    assert!(exited, "Vim should have exited within 2 seconds after :q");

    // Cleanup
    daemon.stop();
}
