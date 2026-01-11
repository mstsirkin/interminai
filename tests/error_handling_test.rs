use assert_cmd::Command;
use std::time::Duration;

mod common;
use common::{interminai_bin, emulator_args};

#[test]
fn test_nonexistent_socket_output() {
    Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg("/tmp/this-socket-does-not-exist.sock")
        .timeout(Duration::from_secs(2))
        .assert()
        .failure()
        .stderr(predicates::str::contains("No such file"));
}

#[test]
fn test_nonexistent_socket_input() {
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg("/tmp/this-socket-does-not-exist.sock")
        .write_stdin("test")
        .timeout(Duration::from_secs(2))
        .assert()
        .failure()
        .stderr(predicates::str::contains("No such file"));
}

#[test]
fn test_nonexistent_socket_running() {
    Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg("/tmp/this-socket-does-not-exist.sock")
        .timeout(Duration::from_secs(2))
        .assert()
        .failure()
        .stderr(predicates::str::contains("No such file"));
}

#[test]
fn test_nonexistent_socket_stop() {
    Command::new(interminai_bin())
        .arg("stop")
        .arg("--socket")
        .arg("/tmp/this-socket-does-not-exist.sock")
        .timeout(Duration::from_secs(2))
        .assert()
        .failure()
        .stderr(predicates::str::contains("No such file"));
}

#[test]
fn test_invalid_terminal_size() {
    Command::new(interminai_bin())
        .arg("start")
        .args(emulator_args())
        .arg("--socket")
        .arg("/tmp/test-invalid-size.sock")
        .arg("--size")
        .arg("notasize")
        .arg("--")
        .arg("sleep")
        .arg("1")
        .timeout(Duration::from_secs(2))
        .assert()
        .failure()
        .stderr(predicates::str::contains("Invalid size"));
}

#[test]
fn test_invalid_signal_name() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let socket = temp_dir.path().join("test.sock");

    // Start daemon
    let mut daemon = std::process::Command::new(interminai_bin())
        .arg("start")
        .args(emulator_args())
        .arg("--socket")
        .arg(socket.to_str().unwrap())
        .arg("--")
        .arg("sleep")
        .arg("100")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("Failed to spawn daemon");

    std::thread::sleep(Duration::from_millis(500));

    // Try invalid signal
    Command::new(interminai_bin())
        .arg("kill")
        .arg("--socket")
        .arg(socket.to_str().unwrap())
        .arg("--signal")
        .arg("NOTASIGNAL")
        .timeout(Duration::from_secs(2))
        .assert()
        .failure()
        .stderr(predicates::str::contains("Invalid signal"));

    // Cleanup
    Command::new(interminai_bin())
        .arg("stop")
        .arg("--socket")
        .arg(socket.to_str().unwrap())
        .output()
        .ok();

    let _ = daemon.wait();
}

#[test]
fn test_invalid_signal_number() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let socket = temp_dir.path().join("test.sock");

    // Start daemon
    let mut daemon = std::process::Command::new(interminai_bin())
        .arg("start")
        .args(emulator_args())
        .arg("--socket")
        .arg(socket.to_str().unwrap())
        .arg("--")
        .arg("sleep")
        .arg("100")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("Failed to spawn daemon");

    std::thread::sleep(Duration::from_millis(500));

    // Try invalid signal number
    Command::new(interminai_bin())
        .arg("kill")
        .arg("--socket")
        .arg(socket.to_str().unwrap())
        .arg("--signal")
        .arg("9999")
        .timeout(Duration::from_secs(2))
        .assert()
        .failure()
        .stderr(predicates::str::contains("Invalid signal"));

    // Cleanup
    Command::new(interminai_bin())
        .arg("stop")
        .arg("--socket")
        .arg(socket.to_str().unwrap())
        .output()
        .ok();

    let _ = daemon.wait();
}

#[test]
fn test_missing_command_argument() {
    let output = Command::new(interminai_bin())
        .arg("start")
        .timeout(Duration::from_secs(2))
        .assert()
        .failure();

    // Accept either Rust or Python error message format
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(
        stderr.contains("required arguments were not provided") ||
        stderr.contains("arguments are required"),
        "Expected error about required arguments, got: {}", stderr
    );
}

#[test]
fn test_nonexistent_command_exits_gracefully() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let socket = temp_dir.path().join("test.sock");

    // Start daemon with nonexistent command
    let mut daemon = std::process::Command::new(interminai_bin())
        .arg("start")
        .args(emulator_args())
        .arg("--socket")
        .arg(socket.to_str().unwrap())
        .arg("--")
        .arg("/this/command/does/not/exist")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("Failed to spawn daemon");

    std::thread::sleep(Duration::from_millis(500));

    // Check that child process exited
    let output = Command::new(interminai_bin())
        .arg("running")
        .arg("--socket")
        .arg(socket.to_str().unwrap())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to check running");

    // Should report not running (exit 1)
    assert!(!output.status.success());

    // Cleanup
    Command::new(interminai_bin())
        .arg("stop")
        .arg("--socket")
        .arg(socket.to_str().unwrap())
        .output()
        .ok();

    let _ = daemon.wait();
}
