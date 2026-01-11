use assert_cmd::Command;
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

        Self {
            socket_path: socket.to_string(),
        }
    }
}

impl Drop for DaemonHandle {
    fn drop(&mut self) {
        // Ensure daemon is stopped even if stop() wasn't explicitly called
        let _ = std::process::Command::new(interminai_bin())
            .arg("stop")
            .arg("--socket")
            .arg(&self.socket_path)
            .output();

        // Give daemon time to clean up
        thread::sleep(Duration::from_millis(100));

        // Remove socket file if it still exists
        let socket_path = std::path::Path::new(&self.socket_path);
        if socket_path.exists() {
            let _ = std::fs::remove_file(socket_path);
        }

        // Remove parent directory if it's empty
        if let Some(parent) = socket_path.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }
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

#[test]
fn test_screen_scrolls_when_exceeding_height() {
    let env = TestEnv::new();

    // Start with small terminal (10 rows)
    // Output 15 lines, which should cause scrolling
    let _daemon = DaemonHandle::spawn_with_socket_and_size(
        &env.socket(),
        "80x10",
        &["bash", "-c", "for i in {1..15}; do echo 'Line '$i; done; sleep 10"]
    );

    thread::sleep(Duration::from_millis(800));

    let screen = get_screen(&env.socket());
    println!("=== Screen (10 rows, 15 lines of output) ===\n{}", screen);

    // The screen should show the LAST 10 lines (lines 6-15) due to scrolling
    // Lines 1-5 should have scrolled off the top

    // Note: bash prompt is also included in the output
    // Should see later lines (the ones visible after scrolling)
    assert!(screen.contains("Line 7") || screen.contains("Line 6"),
            "Should see line 6 or 7 (early lines scrolled off)");
    assert!(screen.contains("Line 10"), "Line 10 should be visible");
    assert!(screen.contains("Line 15"), "Line 15 should be visible");

    // Verify we have the right number of lines (should be 10 rows)
    let non_empty_lines: Vec<&str> = screen.lines()
        .filter(|line| !line.trim().is_empty())
        .collect();

    println!("Non-empty lines count: {}", non_empty_lines.len());
}

#[test]
fn test_screen_scrolls_incrementally() {
    let env = TestEnv::new();

    // Start with tiny terminal (5 rows)
    // This makes it easier to test scrolling behavior
    let _daemon = DaemonHandle::spawn_with_socket_and_size(
        &env.socket(),
        "80x5",
        &["bash", "-c", "sleep 0.2; echo '1'; sleep 0.2; echo '2'; sleep 0.2; echo '3'; sleep 0.2; echo '4'; sleep 0.2; echo '5'; sleep 0.2; echo '6'; sleep 0.2; echo '7'; sleep 10"]
    );

    // Wait for all output
    thread::sleep(Duration::from_millis(2000));

    let screen = get_screen(&env.socket());
    println!("=== Screen (5 rows, 7 lines printed) ===\n{}", screen);

    // With 5 rows and 7 lines, the first 2 lines should have scrolled off
    // We should see lines 3-7 (or possibly the prompt after line 7)

    // Should NOT see early output
    assert!(!screen.contains("1\n") && !screen.contains("1 "), "Line 1 should have scrolled off");
    assert!(!screen.contains("2\n") && !screen.contains("2 "), "Line 2 should have scrolled off");

    // Should see later output
    assert!(screen.contains("6") || screen.contains("7"), "Recent lines should be visible");
}

#[test]
fn test_no_scroll_when_content_fits() {
    let env = TestEnv::new();

    // Start with terminal that has more rows than content
    // 10 rows, only 5 lines of output
    let _daemon = DaemonHandle::spawn_with_socket_and_size(
        &env.socket(),
        "80x10",
        &["bash", "-c", "for i in {1..5}; do echo 'Line '$i; done; sleep 10"]
    );

    thread::sleep(Duration::from_millis(500));

    let screen = get_screen(&env.socket());
    println!("=== Screen (10 rows, 5 lines of output) ===\n{}", screen);

    // All lines should be visible since content fits
    assert!(screen.contains("Line 1"), "Line 1 should be visible");
    assert!(screen.contains("Line 2"), "Line 2 should be visible");
    assert!(screen.contains("Line 3"), "Line 3 should be visible");
    assert!(screen.contains("Line 4"), "Line 4 should be visible");
    assert!(screen.contains("Line 5"), "Line 5 should be visible");
}

#[test]
fn test_vim_scrolls_to_show_last_lines() {
    let env = TestEnv::new();

    // Create a file with 30 lines
    let test_file = env._temp_dir.path().join("scroll_test.txt");
    let mut content = String::new();
    for i in 1..=30 {
        content.push_str(&format!("Line {:02}\n", i));
    }
    std::fs::write(&test_file, content).unwrap();

    // Start vim in a 10-row terminal with a 30-line file
    let _daemon = DaemonHandle::spawn_with_socket_and_size(
        &env.socket(),
        "80x10",
        &["vim", test_file.to_str().unwrap()]
    );

    thread::sleep(Duration::from_millis(1000));

    // At start, should see lines 1-9 (first screen)
    let screen = get_screen(&env.socket());
    println!("=== Vim initial screen (10 rows, 30 line file) ===\n{}", screen);

    assert!(screen.contains("Line 01"), "Should see line 1 initially");
    assert!(!screen.contains("Line 25"), "Should not see line 25 initially");

    // Now go to end of file with 'G'
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("G")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(500));

    let screen = get_screen(&env.socket());
    println!("\n=== Vim after 'G' (should scroll to end) ===\n{}", screen);

    // After scrolling to end, should see last lines
    // Line 30 should be visible, early lines should NOT be visible
    assert!(screen.contains("Line 30"), "Should see line 30 at end");
    assert!(screen.contains("Line 25") || screen.contains("Line 26"),
            "Should see lines near the end");
    assert!(!screen.contains("Line 01"), "Line 1 should have scrolled off");
    assert!(!screen.contains("Line 02"), "Line 2 should have scrolled off");

    // Quit vim
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin(":q!\n")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(300));
}

#[test]
fn test_vim_pagedown_causes_scroll() {
    let env = TestEnv::new();

    // Create file with many lines
    let test_file = env._temp_dir.path().join("long_file.txt");
    let mut content = String::new();
    for i in 1..=50 {
        content.push_str(&format!("Content line {:03}\n", i));
    }
    std::fs::write(&test_file, content).unwrap();

    // Open in 15-row terminal
    let _daemon = DaemonHandle::spawn_with_socket_and_size(
        &env.socket(),
        "80x15",
        &["vim", test_file.to_str().unwrap()]
    );

    thread::sleep(Duration::from_millis(1000));

    let screen_initial = get_screen(&env.socket());
    println!("=== Initial vim screen ===\n{}", screen_initial);
    assert!(screen_initial.contains("Content line 001"), "Should start at top");

    // Page down with Ctrl+F
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin("\x06") // Ctrl+F
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(500));

    let screen_after = get_screen(&env.socket());
    println!("\n=== After Ctrl+F (page down) ===\n{}", screen_after);

    // After paging down, first line should have scrolled off
    assert!(!screen_after.contains("Content line 001"),
            "First line should have scrolled off after page down");

    // Should see later lines now
    let has_later_lines = (10..30).any(|i|
        screen_after.contains(&format!("Content line {:03}", i))
    );
    assert!(has_later_lines, "Should see later lines after page down");

    // Quit
    Command::new(interminai_bin())
        .arg("input")
        .arg("--socket")
        .arg(&env.socket())
        .write_stdin(":q!\n")
        .timeout(Duration::from_secs(2))
        .assert()
        .success();

    thread::sleep(Duration::from_millis(300));
}
