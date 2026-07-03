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
        Self::spawn_with_socket_and_size(socket, "80x24", command_args)
    }

    fn spawn_with_socket_and_size(socket: &str, size: &str, command_args: &[&str]) -> Self {
        use std::process::Stdio;
        use std::io::BufRead;

        let mut cmd = std::process::Command::new(interminai_bin());
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

fn assert_ansi_color(line: &str, text: &str, color_code: &str) {
    let direct = format!("\x1b[{color_code}m{text}");
    let reset_prefixed = format!("\x1b[0;{color_code}m{text}");
    assert!(
        line.contains(&direct) || line.contains(&reset_prefixed),
        "Expected {:?} to contain {:?} or {:?}",
        line,
        direct,
        reset_prefixed
    );
}

#[test]
fn test_number_flag_adds_line_numbers() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "echo 'Hello'; sleep 10"]);

    thread::sleep(Duration::from_millis(500));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .arg("-n")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    // Default 80x24 terminal has 24 lines, so width should be 2
    // First line should start with " 01\t"
    assert!(lines[0].starts_with(" 01\t"), "First line should start with '01\\t', got: {:?}", lines[0]);

    // Line containing "Hello" should be numbered
    let hello_line = lines.iter().find(|l| l.contains("Hello")).expect("Should find Hello line");
    assert!(hello_line.starts_with(" 01\t") || hello_line.starts_with(" 02\t"),
        "Hello line should be numbered, got: {:?}", hello_line);

    // Last line should be " 24\t"
    assert!(lines.last().unwrap().starts_with(" 24\t"),
        "Last line should start with '24\\t', got: {:?}", lines.last());

    daemon.stop();
}

#[test]
fn test_number_flag_long_form() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "echo 'Test'; sleep 10"]);

    thread::sleep(Duration::from_millis(500));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--number")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    // Should have numbered lines
    assert!(lines[0].starts_with(" 01\t"), "First line should start with '01\\t', got: {:?}", lines[0]);

    daemon.stop();
}

#[test]
fn test_number_flag_zero_padding_large_terminal() {
    let env = TestEnv::new();
    // Use a terminal with 100+ rows to test 3-digit padding
    let daemon = {
        use std::process::Stdio;
        use std::io::BufRead;

        let mut cmd = std::process::Command::new(interminai_bin());
        cmd.arg("start")
            .args(emulator_args())
            .arg("--socket")
            .arg(&env.socket())
            .arg("--size")
            .arg("80x100")
            .arg("--no-daemon")
            .arg("--")
            .arg("bash")
            .arg("-c")
            .arg("echo 'Big terminal'; sleep 10");

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
            socket_path: env.socket()
        }
    };

    thread::sleep(Duration::from_millis(500));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .arg("-n")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    // 100 lines -> 3-digit padding
    assert!(lines[0].starts_with(" 001\t"), "First line should be '001\\t', got: {:?}", lines[0]);
    assert!(lines[9].starts_with(" 010\t"), "Line 10 should be '010\\t', got: {:?}", lines[9]);
    assert!(lines[99].starts_with(" 100\t"), "Last line should be '100\\t', got: {:?}", lines[99]);

    daemon.stop();
}

#[test]
fn test_number_flag_without_disables_numbering() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "echo 'NoNum'; sleep 10"]);

    thread::sleep(Duration::from_millis(500));

    // Without -n, output should NOT have line numbers
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().unwrap_or("");

    // Should NOT start with a number and tab
    assert!(!first_line.starts_with(" 01\t"), "Without -n, should not have line numbers, got: {:?}", first_line);

    daemon.stop();
}

#[test]
fn test_number_flag_with_cursor_print() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "echo 'Combined'; sleep 10"]);

    thread::sleep(Duration::from_millis(500));

    // Combine -n with --cursor print
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .arg("-n")
        .arg("--cursor")
        .arg("print")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    // First line should be cursor info (not numbered)
    assert!(lines[0].starts_with("Cursor:"), "First line should be cursor info, got: {:?}", lines[0]);

    // Subsequent lines should be numbered
    assert!(lines[1].starts_with(" 01\t"), "Screen lines should be numbered, got: {:?}", lines[1]);

    daemon.stop();
}

#[test]
fn test_number_flag_preserves_color() {
    if common::emulator() == "custom" {
        return;
    }
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["bash", "-c", "printf '\\033[31mRed\\033[0m'; sleep 10"]);

    thread::sleep(Duration::from_millis(500));

    // -n without --no-color should preserve ANSI color codes
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .arg("-n")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should have numbered lines with color codes preserved
    assert!(stdout.contains("\x1b["), "Color codes should be preserved with -n flag");

    daemon.stop();
}

#[test]
fn test_number_flag_keeps_prefix_uncolored_when_color_carries_across_lines() {
    if common::emulator() == "custom" {
        return;
    }

    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_with_socket_and_size(
        &env.socket(),
        "80x10",
        &[
            "bash",
            "-c",
            "printf '\\033[31mred1\nred2\n\\033[32mgreen3\ngreen4\\033[0m'; sleep 10",
        ],
    );

    thread::sleep(Duration::from_millis(500));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .arg("-n")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    let red1_line = lines.iter().find(|line| line.contains("red1")).expect("Missing red1 line");
    let red2_line = lines.iter().find(|line| line.contains("red2")).expect("Missing red2 line");
    let green3_line = lines.iter().find(|line| line.contains("green3")).expect("Missing green3 line");
    let green4_line = lines.iter().find(|line| line.contains("green4")).expect("Missing green4 line");

    for (line, prefix, text, color_code) in [
        (*red1_line, " 01\t", "red1", "31"),
        (*red2_line, " 02\t", "red2", "31"),
        (*green3_line, " 03\t", "green3", "32"),
        (*green4_line, " 04\t", "green4", "32"),
    ] {
        assert!(line.starts_with(prefix), "Expected {:?} to start with {:?}", line, prefix);
        assert!(!line[..prefix.len()].contains("\x1b["), "Number prefix should be plain: {:?}", line);
        assert_ansi_color(&line[prefix.len()..], text, color_code);
    }

    daemon.stop();
}
