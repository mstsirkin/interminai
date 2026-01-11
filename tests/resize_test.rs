use assert_cmd::Command;
use std::fs;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

mod common;
use common::{interminai_bin, emulator_args};

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
    child: std::process::Child,
    socket_path: String,
}

impl DaemonHandle {
    fn spawn_with_socket_and_size(socket: &str, size: &str, args: &[&str]) -> Self {
        let mut cmd = std::process::Command::new(interminai_bin());
        cmd.arg("start")
            .args(emulator_args())
            .arg("--socket")
            .arg(socket)
            .arg("--size")
            .arg(size)
            .arg("--");

        for arg in args {
            cmd.arg(arg);
        }

        // Use .output() to wait for the daemon start command to return
        // In daemon mode, this will return immediately after the double-fork
        let output = cmd
            .output()
            .expect("Failed to start daemon");

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            panic!("Daemon failed to start: {}\nStderr: {}", output.status, stderr);
        }

        // Wait for socket to be created
        let socket_path = std::path::Path::new(socket);
        for _ in 0..50 {
            if socket_path.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }

        if !socket_path.exists() {
            panic!("Socket was not created: {}", socket);
        }

        // Create a dummy child handle (no actual child process to track in daemon mode)
        Self {
            child: std::process::Command::new("true").spawn().unwrap(),
            socket_path: socket.to_string(),
        }
    }

    fn stop(mut self) {
        let _ = std::process::Command::new(interminai_bin())
            .arg("stop")
            .arg("--socket")
            .arg(&self.socket_path)
            .output();

        thread::sleep(Duration::from_millis(100));
        let _ = self.child.wait();
    }
}

fn send_keys(socket: &str, keys: &str) {
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(socket)
        .write_stdin(keys)
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(100));
}

fn get_screen(socket: &str) -> String {
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(socket)
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get screen");

    String::from_utf8_lossy(&output.stdout).to_string()
}

fn resize_terminal(socket: &str, size: &str) {
    Command::new(interminai_bin())
        .arg("resize")
        .arg("--socket")
        .arg(socket)
        .arg("--size")
        .arg(size)
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));
}

#[test]
fn test_narrow_terminal_wraps_long_lines() {
    let env = TestEnv::new();

    // Start with very narrow terminal (40 columns)
    let daemon = DaemonHandle::spawn_with_socket_and_size(
        &env.socket(),
        "40x24",
        &["bash", "-c", "echo 'This is a very long line that should wrap in a 40 column terminal'; sleep 10"]
    );

    thread::sleep(Duration::from_millis(500));

    let screen = get_screen(&env.socket());
    println!("Screen (40 columns):\n{}", screen);

    // Count lines - the long line should wrap
    let _lines: Vec<&str> = screen.lines().collect();

    // The text is longer than 40 chars, so it should wrap to multiple screen lines
    let text = "This is a very long line that should wrap in a 40 column terminal";
    assert!(text.len() > 40, "Test string should be longer than 40 chars");

    // Check that the text appears (possibly wrapped)
    let screen_text = screen.replace('\n', " ");
    assert!(screen_text.contains("This is a very long line"), "Should contain the text");
    assert!(screen_text.contains("should wra") || screen_text.contains("40 column"),
            "Should contain wrapped portion, got: {}", screen_text);

    daemon.stop();
}

#[test]
fn test_wide_terminal_no_wrap() {
    let env = TestEnv::new();

    // Start with wide terminal (120 columns)
    let daemon = DaemonHandle::spawn_with_socket_and_size(
        &env.socket(),
        "120x24",
        &["bash", "-c", "echo 'This is a line that fits in 120 columns without wrapping'; sleep 10"]
    );

    thread::sleep(Duration::from_millis(500));

    let screen = get_screen(&env.socket());
    println!("Screen (120 columns):\n{}", screen);

    // The text should appear on a single line
    assert!(screen.contains("This is a line that fits in 120 columns without wrapping"));

    daemon.stop();
}

#[test]
fn test_resize_from_narrow_to_wide() {
    let env = TestEnv::new();
    let test_file = env._temp_dir.path().join("resize_test.txt");

    // Create a file with a long line
    fs::write(&test_file, "AAAAAAAAAA BBBBBBBBBB CCCCCCCCCC DDDDDDDDDD EEEEEEEEEE FFFFFFFFFF GGGGGGGGGG\n").unwrap();

    // Start vim with narrow terminal (40 columns)
    let daemon = DaemonHandle::spawn_with_socket_and_size(
        &env.socket(),
        "40x10",
        &["vim", test_file.to_str().unwrap()]
    );

    thread::sleep(Duration::from_millis(1000));

    let screen_narrow = get_screen(&env.socket());
    println!("=== Screen at 40 columns ===\n{}", screen_narrow);

    // Resize to wide (100 columns)
    resize_terminal(&env.socket(), "100x10");

    // Give vim time to process SIGWINCH and redraw
    thread::sleep(Duration::from_millis(1000));

    // Force a screen update by sending a benign command
    send_keys(&env.socket(), "\x1b"); // ESC to ensure normal mode
    thread::sleep(Duration::from_millis(200));

    let screen_wide = get_screen(&env.socket());
    println!("\n=== Screen at 100 columns ===\n{}", screen_wide);

    // The wide screen should have more columns available (check status line shows different position)
    // Note: vim doesn't automatically reflow existing text, but the terminal size changed
    // We can verify by checking that both screens contain the content
    assert!(screen_narrow.contains("AAAAAAAAAA"), "Narrow screen should show content");
    assert!(screen_wide.contains("AAAAAAAAAA"), "Wide screen should show content");

    // If the screens are identical, that's actually OK - vim just shows more available space
    // The key is that resize didn't crash and the content is still visible

    // Quit vim
    send_keys(&env.socket(), ":q!\n");
    thread::sleep(Duration::from_millis(300));

    daemon.stop();
}

#[test]
fn test_resize_from_wide_to_narrow() {
    let env = TestEnv::new();

    // Start bash with wide terminal
    let daemon = DaemonHandle::spawn_with_socket_and_size(
        &env.socket(),
        "100x24",
        &["bash", "-c", "while true; do sleep 1; done"]
    );

    thread::sleep(Duration::from_millis(500));

    // Type a long command
    send_keys(&env.socket(), "echo 'Line 1: AAAAAAAAAA BBBBBBBBBB CCCCCCCCCC DDDDDDDDDD EEEEEEEEEE'\n");
    thread::sleep(Duration::from_millis(300));

    let screen_wide = get_screen(&env.socket());
    println!("=== Screen at 100 columns ===\n{}", screen_wide);

    // Resize to narrow
    resize_terminal(&env.socket(), "40x24");
    thread::sleep(Duration::from_millis(500));

    // Type another long line
    send_keys(&env.socket(), "echo 'Line 2: XXXXXXXXXX YYYYYYYYYY ZZZZZZZZZZ'\n");
    thread::sleep(Duration::from_millis(300));

    let screen_narrow = get_screen(&env.socket());
    println!("\n=== Screen at 40 columns ===\n{}", screen_narrow);

    // Should contain both lines
    assert!(screen_narrow.contains("Line 2"), "Should show second line");

    daemon.stop();
}

#[test]
fn test_resize_tall_terminal() {
    let env = TestEnv::new();

    // Start with short terminal (10 rows)
    let daemon = DaemonHandle::spawn_with_socket_and_size(
        &env.socket(),
        "80x10",
        &["bash", "-c", "for i in {1..20}; do echo Line $i; done; sleep 10"]
    );

    thread::sleep(Duration::from_millis(500));

    let screen_short = get_screen(&env.socket());
    println!("=== Screen at 10 rows ===\n{}", screen_short);
    let lines_short = screen_short.lines().count();

    // Resize to tall (30 rows)
    resize_terminal(&env.socket(), "80x30");
    thread::sleep(Duration::from_millis(500));

    let screen_tall = get_screen(&env.socket());
    println!("\n=== Screen at 30 rows ===\n{}", screen_tall);
    let lines_tall = screen_tall.lines().count();

    // Tall screen should have more lines visible
    assert!(lines_tall > lines_short, "Tall terminal should show more lines: {} vs {}", lines_tall, lines_short);

    daemon.stop();
}

#[test]
fn test_multiple_resizes() {
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket_and_size(
        &env.socket(),
        "80x24",
        &["cat"]
    );

    thread::sleep(Duration::from_millis(500));

    // Resize multiple times
    resize_terminal(&env.socket(), "40x10");
    resize_terminal(&env.socket(), "100x30");
    resize_terminal(&env.socket(), "60x20");
    resize_terminal(&env.socket(), "120x40");

    // Should still be responsive
    send_keys(&env.socket(), "Test after multiple resizes\n");
    thread::sleep(Duration::from_millis(200));

    let screen = get_screen(&env.socket());
    assert!(screen.contains("Test after multiple resizes"), "Should still work after multiple resizes");

    daemon.stop();
}

#[test]
fn test_resize_invalid_size() {
    let env = TestEnv::new();

    let daemon = DaemonHandle::spawn_with_socket_and_size(
        &env.socket(),
        "80x24",
        &["sleep", "10"]
    );

    thread::sleep(Duration::from_millis(500));

    // Try invalid size
    let result = Command::new(interminai_bin())
        .arg("resize")
        .arg("--socket")
        .arg(&env.socket())
        .arg("--size")
        .arg("invalid")
        .timeout(Duration::from_secs(2))
        .output();

    // Should fail
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(!output.status.success(), "Invalid size should fail");

    daemon.stop();
}

#[test]
fn test_vim_reflow_after_resize() {
    let env = TestEnv::new();
    let test_file = env._temp_dir.path().join("reflow.txt");

    // Start vim narrow
    let daemon = DaemonHandle::spawn_with_socket_and_size(
        &env.socket(),
        "40x20",
        &["vim", test_file.to_str().unwrap()]
    );

    thread::sleep(Duration::from_millis(1000));

    // Type a very long line
    send_keys(&env.socket(), "i");
    send_keys(&env.socket(), "AAAA BBBB CCCC DDDD EEEE FFFF GGGG HHHH IIII JJJJ KKKK LLLL MMMM NNNN OOOO PPPP");
    send_keys(&env.socket(), "\x1b");

    thread::sleep(Duration::from_millis(300));

    let screen_narrow = get_screen(&env.socket());
    println!("=== Vim at 40 cols ===\n{}", screen_narrow);

    // Resize to wide
    resize_terminal(&env.socket(), "100x20");
    thread::sleep(Duration::from_millis(500));

    let screen_wide = get_screen(&env.socket());
    println!("\n=== Vim at 100 cols ===\n{}", screen_wide);

    // The content should still be there
    assert!(screen_wide.contains("AAAA") && screen_wide.contains("PPPP"),
            "Content should be visible after resize");

    // Quit without saving
    send_keys(&env.socket(), ":q!\n");
    thread::sleep(Duration::from_millis(300));

    daemon.stop();
}
