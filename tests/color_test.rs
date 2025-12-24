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
    child: std::process::Child,
    socket_path: String,
}

impl DaemonHandle {
    fn spawn_with_socket(socket: &str, color_flag: bool, command_args: &[&str]) -> Self {
        use std::process::Stdio;
        use std::io::BufRead;

        let mut cmd = std::process::Command::new(interminai_bin());
        cmd.arg("start")
            .arg("--socket")
            .arg(socket)
            .arg("--no-daemon");
        
        if color_flag {
            cmd.arg("--color");
        }
        
        cmd.arg("--");

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
fn test_color_flag_enables_sgr() {
    let env = TestEnv::new();

    // Start a session that outputs SGR color sequences
    let daemon = DaemonHandle::spawn_with_socket(
        &env.socket(),
        true, // --color flag
        &["bash", "-c", "echo -e '\\e[31mRed\\e[0m \\e[32mGreen\\e[0m \\e[34mBlue\\e[0m'"]
    );

    thread::sleep(Duration::from_millis(500));

    // Get output
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // With --color, SGR sequences should be preserved in output
    assert!(stdout.contains("\x1b[31m"), "Should contain red color escape code");
    assert!(stdout.contains("Red"));
    assert!(stdout.contains("\x1b[32m"), "Should contain green color escape code");
    assert!(stdout.contains("Green"));
    assert!(stdout.contains("\x1b[34m"), "Should contain blue color escape code");
    assert!(stdout.contains("Blue"));
    assert!(stdout.contains("\x1b[0m"), "Should contain reset escape code");

    daemon.stop();
}

#[test]
fn test_without_color_flag_strips_sgr() {
    let env = TestEnv::new();

    // Start a session WITHOUT --color flag
    let daemon = DaemonHandle::spawn_with_socket(
        &env.socket(),
        false, // no --color flag
        &["bash", "-c", "echo -e '\\e[31mRed\\e[0m \\e[32mGreen\\e[0m \\e[34mBlue\\e[0m'"]
    );

    thread::sleep(Duration::from_millis(500));

    // Get output
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Without --color, text should appear but SGR codes should be stripped
    assert!(stdout.contains("Red"));
    assert!(stdout.contains("Green"));
    assert!(stdout.contains("Blue"));
    assert!(!stdout.contains("\x1b["), "Should not contain any escape codes");

    daemon.stop();
}

#[test]
fn test_color_flag_preserves_bold() {
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(
        &env.socket(),
        true,
        &["bash", "-c", "echo -e '\\e[1mBold\\e[0m Normal'"]
    );

    thread::sleep(Duration::from_millis(500));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain the text and bold escape codes
    assert!(stdout.contains("\x1b[1m"), "Should contain bold escape code");
    assert!(stdout.contains("Bold"));
    assert!(stdout.contains("Normal"));
    assert!(stdout.contains("\x1b[0m"), "Should contain reset escape code");

    daemon.stop();
}

#[test]
fn test_color_flag_with_background_colors() {
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket(
        &env.socket(),
        true,
        &["bash", "-c", "echo -e '\\e[41mRed BG\\e[0m \\e[42mGreen BG\\e[0m'"]
    );

    thread::sleep(Duration::from_millis(500));

    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(&env.socket())
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get output");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain text and background color codes
    assert!(stdout.contains("\x1b[41m"), "Should contain red background escape code");
    assert!(stdout.contains("Red BG"));
    assert!(stdout.contains("\x1b[42m"), "Should contain green background escape code");
    assert!(stdout.contains("Green BG"));
    assert!(stdout.contains("\x1b[0m"), "Should contain reset escape code");

    daemon.stop();
}
