mod common;
use common::{interminai_client_bin, interminai_server_bin, emulator_args, emulator};

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
        Self::spawn_with_size(socket, "80x24", command_args)
    }

    fn spawn_with_size(socket: &str, size: &str, command_args: &[&str]) -> Self {
        use std::process::Stdio;
        use std::io::BufRead;

        let mut cmd = std::process::Command::new(interminai_server_bin());
        cmd.arg("start")
            .args(emulator_args())
            .arg("--socket")
            .arg(socket)
            .arg("--size")
            .arg(size)
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
        let _ = Command::new(interminai_client_bin())
            .arg("stop")
            .arg("--socket")
            .arg(&self.socket_path)
            .output();
        let _ = self._child.wait();
    }
}

// Inspect the prompt and cursor without entering any password. The target's
// row is 80 columns, while the user's terminal can have a different width.
fn assert_password_layout(script: &str, size: &str, rows: &[&str], row: usize, col: usize) {
    let env = TestEnv::new();
    let wrapper_env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", script]);
    let client = interminai_client_bin();
    let wrapper = DaemonHandle::spawn_with_size(&wrapper_env.socket(), size, &[
        &client, "input", "--socket", &daemon.socket_path, "--password",
    ]);

    let output = Command::new(client)
        .args(["output", "--socket", &wrapper.socket_path, "--cursor", "print", "--no-color"])
        .timeout(Duration::from_secs(3))
        .output()
        .expect("Failed to inspect password prompt");
    assert!(output.status.success());
    let screen = String::from_utf8_lossy(&output.stdout);
    let mut lines = screen.lines();
    let expected_cursor = format!("Cursor: row {}, col {}", row, col);
    assert_eq!(lines.next(), Some(expected_cursor.as_str()), "{}", screen);
    assert_eq!(lines.next().map(str::trim_end), Some("Type your secret or password and press Enter."));
    for expected in rows {
        assert_eq!(lines.next().map(str::trim_end), Some(*expected), "{}", screen);
    }
    assert!(lines.all(|line| line.trim().is_empty()), "Unexpected extra prompt rows: {}", screen);
}

#[test]
fn test_password_layout_sudo() {
    assert_password_layout(
        "printf 'sudo true\n'; read -s -p '[sudo] password for user: ' pass",
        "80x24", &["sudo true", "[sudo] password for user:"], 3, 27,
    );
    assert_password_layout(
        "printf 'sudo true\n'; read -s -p '[sudo] password for user: ' pass",
        "50x24", &["sudo true", "[sudo] password for user:"], 3, 27,
    );
}

#[test]
fn test_password_layout_preserves_spacing() {
    assert_password_layout(
        "read -s -p '  Password:   ' pass",
        "80x24", &["  Password:"], 2, 15,
    );
    assert_password_layout(
        "read -s -p 'Password:' pass",
        "80x24", &["Password:"], 2, 10,
    );
}

#[test]
fn test_password_layout_blank_input_line() {
    assert_password_layout(
        "printf 'Password:\n'; read -s -p '    ' pass",
        "80x24", &["Password:", ""], 3, 5,
    );
}

#[test]
fn test_password_layout_hint() {
    assert_password_layout(
        "printf 'Password:      (account password)\r\x1b[10C'; read -s pass",
        "80x24", &["Password:      (account password)"], 2, 11,
    );
    assert_password_layout(
        "printf '(account password)\r'; read -s pass",
        "80x24", &["(account password)"], 2, 1,
    );
}

#[test]
fn test_password_layout_wrapped_hint() {
    assert_password_layout(
        "printf 'Password:                                           hint\r\x1b[10C'; read -s pass",
        "50x24", &["Password:", "  hint"], 2, 11,
    );
    // Text ending exactly at the local right edge has a pending wrap, not a new row.
    assert_password_layout(
        "printf 'Password:                                     hint\r\x1b[10C'; read -s pass",
        "50x24", &["Password:                                     hint"], 2, 11,
    );
}

#[test]
fn test_password_layout_unicode() {
    if emulator() == "custom" {
        return; // The basic emulator counts every character as one cell.
    }
    assert_password_layout(
        "read -s -p '密码:   ' pass",
        "80x24", &["密码:"], 2, 9,
    );
}

// Test --password flag exists in help
#[test]
fn test_password_flag_in_help() {
    let output = Command::new(interminai_client_bin())
        .arg("input")
        .arg("--help")
        .output()
        .expect("Failed to run help");

    let help_text = String::from_utf8_lossy(&output.stdout);
    assert!(help_text.contains("--password"), "Help should mention --password flag: {}", help_text);
    assert!(help_text.contains("echo disabled"), "Help should mention echo disabled: {}", help_text);
}

// Test --password fails gracefully when stdin is not a terminal
#[test]
fn test_password_requires_terminal() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["cat"]);
    thread::sleep(Duration::from_millis(200));

    // Running --password without a TTY should fail
    let output = Command::new(interminai_client_bin())
        .arg("input")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .arg("--password")
        .output()
        .expect("Failed to run input");

    // Should fail because stdin is not a terminal
    assert!(!output.status.success(), "Should fail without terminal");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("terminal") || stderr.contains("password"),
            "Error should mention terminal or password: {}", stderr);
}

// Test --password using interminai to provide PTY for the input command
#[test]
fn test_password_with_interminai_pty() {
    let env = TestEnv::new();
    let env2 = TestEnv::new();  // Second socket for the input --password wrapper

    // Start a session with bash that reads a password using read -s
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c",
        "read -s -p 'Password: ' pass; echo; echo \"Got: $pass\""]);
    thread::sleep(Duration::from_millis(300));

    // Check we see the password prompt
    let output = Command::new(interminai_client_bin())
        .arg("output")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .output()
        .expect("Failed to get output");

    let screen = String::from_utf8_lossy(&output.stdout);
    assert!(screen.contains("Password:"), "Should show password prompt: {}", screen);

    // Start another interminai session to run `input --password` with a real PTY
    let input_cmd = format!("{} input --socket {} --password",
                            interminai_client_bin(), daemon.socket_path);
    let wrapper = DaemonHandle::spawn_with_socket(&env2.socket(), &["bash", "-c", &input_cmd]);
    thread::sleep(Duration::from_millis(300));

    // Check wrapper shows the generic guidance AND the actual application prompt
    let output = Command::new(interminai_client_bin())
        .arg("output")
        .arg("--socket")
        .arg(&wrapper.socket_path)
        .output()
        .expect("Failed to get wrapper output");

    let screen = String::from_utf8_lossy(&output.stdout);
    assert!(screen.contains("Type your secret or password"),
            "Wrapper should show generic guidance: {}", screen);
    assert!(screen.contains("Password:"),
            "Wrapper should show the application's password prompt: {}", screen);

    // Send the password to the wrapper (which forwards to the original session)
    Command::new(interminai_client_bin())
        .arg("input")
        .arg("--socket")
        .arg(&wrapper.socket_path)
        .arg("--text")
        .arg("secret123\\r")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(500));

    // Verify that the password was NOT echoed on the wrapper screen
    let output = Command::new(interminai_client_bin())
        .arg("output")
        .arg("--socket")
        .arg(&wrapper.socket_path)
        .output()
        .expect("Failed to get wrapper output");

    let wrapper_screen = String::from_utf8_lossy(&output.stdout);
    assert!(!wrapper_screen.contains("secret123"),
            "Password should NOT be echoed on wrapper screen: {}", wrapper_screen);

    // Check that the original session received the password
    let output = Command::new(interminai_client_bin())
        .arg("output")
        .arg("--socket")
        .arg(&daemon.socket_path)
        .output()
        .expect("Failed to get output");

    let screen = String::from_utf8_lossy(&output.stdout);
    assert!(screen.contains("Got: secret123"),
            "Should show received password: {}", screen);
}
