mod common;
use common::{interminai_bin, emulator_args};

use assert_cmd::Command;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn test_wait_interrupted_by_client_disconnect() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let socket_path = temp_dir.path().join("wait.sock");

    // Start daemon with long-running command
    let output = Command::new(interminai_bin())
        .arg("start")
        .args(emulator_args())
        .arg("--socket")
        .arg(socket_path.to_str().unwrap())
        .arg("--")
        .arg("sleep")
        .arg("1000")
        .output()
        .expect("Failed to start daemon");

    assert!(output.status.success(), "Failed to start daemon");

    // Give daemon time to start
    thread::sleep(Duration::from_millis(500));

    // Start WAIT command in background thread with timeout
    let socket_path_clone = socket_path.to_str().unwrap().to_string();
    let interminai_bin_path = interminai_bin();
    let wait_thread = thread::spawn(move || {
        // Use timeout command to kill WAIT after 2 seconds
        let output = std::process::Command::new("timeout")
            .arg("2")
            .arg(&interminai_bin_path)
            .arg("wait")
            .arg("--socket")
            .arg(&socket_path_clone)
            .output()
            .expect("Failed to run wait command");

        // Timeout command returns 124 when it times out
        assert_eq!(output.status.code(), Some(124), "Wait should have timed out");
    });

    // Wait for the WAIT command to start and then timeout
    thread::sleep(Duration::from_secs(3));

    // Now send KILL command - this should succeed
    // The server should have detected the client disconnect and stopped blocking
    let kill_output = Command::new(interminai_bin())
        .arg("kill")
        .arg("--socket")
        .arg(socket_path.to_str().unwrap())
        .arg("--signal")
        .arg("SIGTERM")
        .timeout(Duration::from_secs(5))
        .output()
        .expect("Failed to run kill command");

    assert!(
        kill_output.status.success(),
        "Kill command should succeed after client disconnect. Exit code: {:?}, stderr: {}",
        kill_output.status.code(),
        String::from_utf8_lossy(&kill_output.stderr)
    );

    // Wait for the background thread to finish
    wait_thread.join().expect("Wait thread panicked");

    // Verify daemon stopped
    thread::sleep(Duration::from_millis(500));

    let running_output = Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(socket_path.to_str().unwrap())
        .output()
        .expect("Failed to check running status");

    // Should report not running
    let stdout = String::from_utf8_lossy(&running_output.stdout);
    assert!(
        !stdout.contains("true") || stdout.contains("false") || stdout.contains("null"),
        "Daemon should have stopped after kill"
    );
}
