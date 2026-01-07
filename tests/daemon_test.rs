use assert_cmd::Command;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

mod common;
use common::{interminai_bin, interminai_server_bin, interminai_client_bin};

#[test]
fn test_daemon_mode_returns_immediately() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let socket_path = temp_dir.path().join("daemon.sock");

    // Start in daemon mode (default, no --no-daemon flag)
    let start = std::time::Instant::now();

    let output = Command::new(interminai_server_bin())
        .arg("start")
        .arg("--socket")
        .arg(socket_path.to_str().unwrap())
        .arg("--")
        .arg("sleep")
        .arg("10")
        .output()
        .expect("Failed to execute interminai");

    let elapsed = start.elapsed();

    // Should return immediately (under 2 seconds)
    assert!(elapsed < Duration::from_secs(2), "Daemon mode took too long: {:?}", elapsed);
    assert!(output.status.success(), "Command failed: {}", String::from_utf8_lossy(&output.stderr));

    // Parse socket path from output
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Socket:"), "Output missing socket line");
    assert!(stdout.contains("PID:"), "Output missing PID line");

    // Give daemon time to initialize
    thread::sleep(Duration::from_millis(500));

    // Verify daemon is running
    Command::new(interminai_client_bin())
        .arg("running")
        .arg("--socket")
        .arg(socket_path.to_str().unwrap())
        .assert()
        .success();

    // Stop daemon
    Command::new(interminai_client_bin())
        .arg("stop")
        .arg("--socket")
        .arg(socket_path.to_str().unwrap())
        .assert()
        .success();
}

#[test]
fn test_no_daemon_flag_runs_foreground() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let socket_path = temp_dir.path().join("foreground.sock");

    // Start with --no-daemon flag (should block)
    let socket = socket_path.to_str().unwrap().to_string();

    let handle = std::thread::spawn(move || {
        let output = Command::new(interminai_bin())
            .arg("start")
            .arg("--socket")
            .arg(&socket)
            .arg("--no-daemon")
            .arg("--")
            .arg("bash")
            .arg("-c")
            .arg("echo ready; sleep 0.5")
            .output()
            .expect("Failed to execute interminai");
        output
    });

    // Wait for daemon to start
    thread::sleep(Duration::from_millis(500));

    // Verify it's running
    Command::new(interminai_client_bin())
        .arg("output")
        .arg("--socket")
        .arg(socket_path.to_str().unwrap())
        .assert()
        .success();

    // Stop it
    Command::new(interminai_client_bin())
        .arg("stop")
        .arg("--socket")
        .arg(socket_path.to_str().unwrap())
        .assert()
        .success();

    // The thread should complete now
    let result = handle.join().expect("Thread panicked");
    assert!(result.status.success());
}

#[test]
fn test_daemon_mode_survives_parent_exit() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let socket_path = temp_dir.path().join("survive.sock");

    // Start daemon and let parent exit
    {
        let output = Command::new(interminai_bin())
            .arg("start")
            .arg("--socket")
            .arg(socket_path.to_str().unwrap())
            .arg("--")
            .arg("sleep")
            .arg("10")
            .output()
            .expect("Failed to execute interminai");

        assert!(output.status.success());
    }
    // Parent process exited, but daemon should still be running

    thread::sleep(Duration::from_millis(500));

    // Daemon should still be accessible
    Command::new(interminai_client_bin())
        .arg("running")
        .arg("--socket")
        .arg(socket_path.to_str().unwrap())
        .assert()
        .success();

    // Clean up
    Command::new(interminai_client_bin())
        .arg("stop")
        .arg("--socket")
        .arg(socket_path.to_str().unwrap())
        .assert()
        .success();
}

#[test]
fn test_auto_generated_socket_from_output() {
    // Start without --socket and parse socket path from output
    let output = Command::new(interminai_server_bin())
        .arg("start")
        .arg("--")
        .arg("bash")
        .arg("-c")
        .arg("echo ready; sleep 10")
        .output()
        .expect("Failed to execute interminai");

    assert!(output.status.success(), "Command failed: {}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse socket path from "Socket: /path/to/socket" line
    let socket_path = stdout
        .lines()
        .find(|line| line.starts_with("Socket: "))
        .expect("Output missing 'Socket:' line")
        .strip_prefix("Socket: ")
        .expect("Failed to parse socket path")
        .trim();

    assert!(!socket_path.is_empty(), "Socket path is empty");

    // Give daemon time to initialize
    thread::sleep(Duration::from_millis(500));

    // Verify daemon is running using the parsed socket
    Command::new(interminai_client_bin())
        .arg("running")
        .arg("--socket")
        .arg(socket_path)
        .assert()
        .success();

    // Get output to verify it's working
    let output_result = Command::new(interminai_client_bin())
        .arg("output")
        .arg("--socket")
        .arg(socket_path)
        .output()
        .expect("Failed to get output");

    assert!(output_result.status.success());
    let screen = String::from_utf8_lossy(&output_result.stdout);
    assert!(screen.contains("ready"), "Screen should show 'ready' from bash command");

    // Stop daemon
    Command::new(interminai_client_bin())
        .arg("stop")
        .arg("--socket")
        .arg(socket_path)
        .assert()
        .success();

    // Verify socket and its directory were cleaned up
    // (daemon has 200ms delay before cleanup, so wait longer)
    thread::sleep(Duration::from_millis(500));
    let socket = std::path::Path::new(socket_path);
    assert!(!socket.exists(),
        "Auto-generated socket should be removed after stop");

    let socket_dir = socket.parent().expect("Socket should have parent directory");
    assert!(!socket_dir.exists(),
        "Auto-generated socket directory should be removed after stop");
}
