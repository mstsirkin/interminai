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
    fn spawn_with_socket(socket: &str, args: &[&str]) -> Self {
        let mut cmd = std::process::Command::new(interminai_bin());
        cmd.arg("start")
            .args(emulator_args())
            .arg("--socket")
            .arg(socket)
            .arg("--");  // Important: separator before command

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

        // Parse output to verify daemon started
        let stdout = String::from_utf8_lossy(&output.stdout);
        println!("Daemon started:");
        for line in stdout.lines() {
            println!("  {}", line);
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
        // We'll track the daemon via socket communication instead
        Self {
            child: std::process::Command::new("true").spawn().unwrap(),  // Dummy process
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
    // Use --no-color because vim outputs syntax highlighting (ANSI color codes).
    // Tests check for exact text like "Line 3: Third line content [MODIFIED]"
    // which would fail if color codes are embedded in the output.
    let output = Command::new(interminai_bin())
        .arg("output")
        .arg("--socket")
        .arg(socket)
        .arg("--no-color")
        .timeout(Duration::from_secs(2))
        .output()
        .expect("Failed to get screen");

    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn test_comprehensive_vim_editing() {
    let env = TestEnv::new();
    let test_file = env._temp_dir.path().join("comprehensive.txt");

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["vim", test_file.to_str().unwrap()]);

    // Wait for socket to be created
    for _ in 0..20 {
        if std::path::Path::new(&env.socket()).exists() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    // Additional wait for vim to fully initialize
    thread::sleep(Duration::from_millis(1000));

    // Verify socket exists
    assert!(std::path::Path::new(&env.socket()).exists(), "Socket should exist before starting test");

    // === Phase 1: Enter insert mode and add initial content ===
    println!("=== Phase 1: Adding initial content ===");
    send_keys(&env.socket(), "i");
    send_keys(&env.socket(), "Line 1: This is the first line\n");
    send_keys(&env.socket(), "Line 2: Second line here\n");
    send_keys(&env.socket(), "Line 3: Third line content\n");
    send_keys(&env.socket(), "Line 4: Fourth line\n");
    send_keys(&env.socket(), "Line 5: Fifth line");

    // Exit insert mode
    send_keys(&env.socket(), "\x1b");
    thread::sleep(Duration::from_millis(200));

    let screen = get_screen(&env.socket());
    println!("Screen after initial insert:\n{}", screen);
    assert!(screen.contains("Line 1"), "Should show Line 1");
    assert!(screen.contains("Line 5"), "Should show Line 5");

    // === Phase 2: Move up and edit line 3 ===
    println!("\n=== Phase 2: Editing line 3 ===");

    // Move up 2 lines (currently on line 5, go to line 3)
    send_keys(&env.socket(), "kk");
    thread::sleep(Duration::from_millis(100));

    // Go to end of line and append
    send_keys(&env.socket(), "A");
    send_keys(&env.socket(), " [MODIFIED]");
    send_keys(&env.socket(), "\x1b");

    let screen = get_screen(&env.socket());
    println!("Screen after editing line 3:\n{}", screen);
    assert!(screen.contains("Line 3: Third line content [MODIFIED]"), "Line 3 should be modified");

    // === Phase 3: Move to line 1 and prepend ===
    println!("\n=== Phase 3: Modifying line 1 ===");

    // Go to top of file
    send_keys(&env.socket(), "gg");
    thread::sleep(Duration::from_millis(100));

    // Insert at beginning
    send_keys(&env.socket(), "I");
    send_keys(&env.socket(), ">>> ");
    send_keys(&env.socket(), "\x1b");

    let screen = get_screen(&env.socket());
    println!("Screen after prepending to line 1:\n{}", screen);
    assert!(screen.contains(">>> Line 1"), "Line 1 should have prefix");

    // === Phase 4: Add new lines in the middle ===
    println!("\n=== Phase 4: Adding lines in middle ===");

    // Go to line 2
    send_keys(&env.socket(), "2G");
    thread::sleep(Duration::from_millis(100));

    // Open new line below
    send_keys(&env.socket(), "o");
    send_keys(&env.socket(), "Line 2.5: Inserted between 2 and 3");
    send_keys(&env.socket(), "\x1b");

    let screen = get_screen(&env.socket());
    println!("Screen after inserting line 2.5:\n{}", screen);
    assert!(screen.contains("Line 2.5"), "Should have new line 2.5");

    // === Phase 5: Navigate with hjkl and make edits ===
    println!("\n=== Phase 5: Character-level navigation and editing ===");

    // Go to line 4 (which is now line 5 after insertion)
    send_keys(&env.socket(), "5G");
    thread::sleep(Duration::from_millis(100));

    // Move right to "Fourth" word
    send_keys(&env.socket(), "w");

    // Delete word and replace
    send_keys(&env.socket(), "cw");
    send_keys(&env.socket(), "FOURTH");
    send_keys(&env.socket(), "\x1b");

    let screen = get_screen(&env.socket());
    println!("Screen after word replacement:\n{}", screen);
    assert!(screen.contains("FOURTH"), "Should have FOURTH in caps");

    // === Phase 6: Move to end and add more lines ===
    println!("\n=== Phase 6: Adding more content at end ===");

    // Go to end of file
    send_keys(&env.socket(), "G");
    thread::sleep(Duration::from_millis(100));

    // Add new lines
    send_keys(&env.socket(), "o");
    send_keys(&env.socket(), "Line 6: Sixth line added at end\n");
    send_keys(&env.socket(), "Line 7: Seventh line\n");
    send_keys(&env.socket(), "Line 8: Final line with numbers: 1 2 3 4 5");
    send_keys(&env.socket(), "\x1b");

    let screen = get_screen(&env.socket());
    println!("Screen after adding end lines:\n{}", screen);
    assert!(screen.contains("Line 8"), "Should show Line 8");
    assert!(screen.contains("1 2 3 4 5"), "Should show numbers");

    // === Phase 7: Navigate back up and make more edits ===
    println!("\n=== Phase 7: More navigation and edits ===");

    // Go to line 1
    send_keys(&env.socket(), "gg");
    thread::sleep(Duration::from_millis(100));

    // Move down 3 lines
    send_keys(&env.socket(), "jjj");

    // Go to end of line and add
    send_keys(&env.socket(), "A");
    send_keys(&env.socket(), " <<<");
    send_keys(&env.socket(), "\x1b");

    let screen = get_screen(&env.socket());
    println!("Screen after adding arrows:\n{}", screen);
    assert!(screen.contains("<<<"), "Should show arrows");

    // === Phase 8: Delete a line ===
    println!("\n=== Phase 8: Deleting a line ===");

    // Go to line 2.5 (line 3 now)
    send_keys(&env.socket(), "3G");
    thread::sleep(Duration::from_millis(100));

    // Delete entire line
    send_keys(&env.socket(), "dd");

    let screen = get_screen(&env.socket());
    println!("Screen after deleting line:\n{}", screen);
    // Line 2.5 should be gone

    // === Phase 9: Save the file ===
    println!("\n=== Phase 9: Saving file ===");

    send_keys(&env.socket(), ":w\n");
    thread::sleep(Duration::from_millis(300));

    let screen = get_screen(&env.socket());
    println!("Screen after save:\n{}", screen);

    // === Phase 10: Verify file contents ===
    println!("\n=== Phase 10: Verifying saved content ===");

    // Quit vim
    send_keys(&env.socket(), ":q\n");
    thread::sleep(Duration::from_millis(300));

    daemon.stop();

    // Read and verify file
    assert!(test_file.exists(), "File should exist");
    let content = fs::read_to_string(&test_file).expect("Failed to read file");

    println!("\n=== Final file content ===\n{}", content);

    // Verify expected content
    assert!(content.contains(">>> Line 1"), "Should have modified line 1");
    assert!(content.contains("Line 2: Second line here"), "Should have line 2");
    assert!(!content.contains("Line 2.5"), "Line 2.5 should be deleted");
    assert!(content.contains("Line 3: Third line content [MODIFIED]"), "Should have modified line 3");
    assert!(content.contains("<<<"), "Should have arrows added");
    assert!(content.contains("Line FOURTH:") || content.contains("FOURTH line"), "Should have FOURTH in caps");
    assert!(content.contains("Line 5: Fifth line"), "Should have line 5");
    assert!(content.contains("Line 6: Sixth line added at end"), "Should have line 6");
    assert!(content.contains("Line 7: Seventh line"), "Should have line 7");
    assert!(content.contains("Line 8: Final line with numbers: 1 2 3 4 5"), "Should have line 8 with numbers");

    // Count lines (should be 8 lines total after deletion)
    let line_count = content.lines().count();
    assert_eq!(line_count, 8, "Should have exactly 8 lines after all edits, got {}", line_count);

    println!("\n✅ Comprehensive vim test passed! All edits verified.");
}

#[test]
fn test_vim_dd_screen_display_sync() {
    // Regression test for vim dd command bug where screen display didn't update
    // after deleting a line. This was caused by TERM=xterm-256color using
    // advanced escape sequences our emulator didn't support.
    println!("\n=== Testing vim dd screen/buffer sync ===");

    let env = TestEnv::new();
    let test_file = env._temp_dir.path().join("dd_test.txt");

    // Create file with distinct lines
    fs::write(&test_file, "FIRST_LINE\nSECOND_LINE\nTHIRD_LINE\nFOURTH_LINE\n")
        .expect("Failed to create test file");

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["vim", test_file.to_str().unwrap()]);
    thread::sleep(Duration::from_millis(1000));

    // Verify initial screen shows all lines
    let screen = get_screen(&env.socket());
    println!("Initial screen:\n{}", screen);
    assert!(screen.contains("FIRST_LINE"), "Initial screen should show FIRST_LINE");
    assert!(screen.contains("SECOND_LINE"), "Initial screen should show SECOND_LINE");
    assert!(screen.contains("THIRD_LINE"), "Initial screen should show THIRD_LINE");

    // Delete first line with dd
    send_keys(&env.socket(), "dd");
    thread::sleep(Duration::from_millis(300));

    // CRITICAL: Screen should immediately show updated content without FIRST_LINE
    let screen_after_dd = get_screen(&env.socket());
    println!("Screen after dd:\n{}", screen_after_dd);
    
    // The bug was: screen still showed FIRST_LINE even though buffer deleted it
    assert!(!screen_after_dd.contains("FIRST_LINE"), 
        "Screen should NOT show FIRST_LINE after dd (bug: screen didn't update)");
    assert!(screen_after_dd.contains("SECOND_LINE"), 
        "Screen should show SECOND_LINE as first line");
    assert!(screen_after_dd.contains("THIRD_LINE"), 
        "Screen should show THIRD_LINE");

    // Save and verify file contents match screen
    send_keys(&env.socket(), ":w\n");
    thread::sleep(Duration::from_millis(300));
    send_keys(&env.socket(), ":q\n");
    thread::sleep(Duration::from_millis(300));

    daemon.stop();

    // Verify file contents
    let content = fs::read_to_string(&test_file).expect("Failed to read file");
    println!("File contents after save:\n{}", content);
    
    assert!(!content.contains("FIRST_LINE"), "File should NOT contain FIRST_LINE");
    assert!(content.contains("SECOND_LINE"), "File should contain SECOND_LINE");
    assert!(content.contains("THIRD_LINE"), "File should contain THIRD_LINE");
    assert!(content.contains("FOURTH_LINE"), "File should contain FOURTH_LINE");

    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 3, "File should have exactly 3 lines after deleting one");
    assert_eq!(lines[0], "SECOND_LINE", "First line should be SECOND_LINE");

    println!("✅ vim dd screen/buffer sync test passed!");
}

#[test]
fn test_vim_dd_multiple_lines() {
    // Test deleting multiple lines in sequence
    println!("\n=== Testing vim multiple dd commands ===");

    let env = TestEnv::new();
    let test_file = env._temp_dir.path().join("multi_dd_test.txt");

    fs::write(&test_file, "Line1\nLine2\nLine3\nLine4\nLine5\n")
        .expect("Failed to create test file");

    let daemon = DaemonHandle::spawn_with_socket(&env.socket(), &["vim", test_file.to_str().unwrap()]);
    thread::sleep(Duration::from_millis(1000));

    // Delete first line
    send_keys(&env.socket(), "dd");
    thread::sleep(Duration::from_millis(200));

    let screen = get_screen(&env.socket());
    assert!(screen.contains("Line2"), "Should show Line2 first");
    assert!(!screen.contains("Line1"), "Should not show Line1");

    // Delete another line (now deleting Line2)
    send_keys(&env.socket(), "dd");
    thread::sleep(Duration::from_millis(200));

    let screen = get_screen(&env.socket());
    assert!(screen.contains("Line3"), "Should show Line3 first");
    assert!(!screen.contains("Line2"), "Should not show Line2");

    // Move down and delete Line4
    send_keys(&env.socket(), "j");
    send_keys(&env.socket(), "dd");
    thread::sleep(Duration::from_millis(200));

    let screen = get_screen(&env.socket());
    assert!(screen.contains("Line3"), "Should still show Line3");
    assert!(screen.contains("Line5"), "Should show Line5");
    assert!(!screen.contains("Line4"), "Should not show Line4");

    // Save and verify
    send_keys(&env.socket(), ":wq\n");
    thread::sleep(Duration::from_millis(300));

    daemon.stop();

    let content = fs::read_to_string(&test_file).expect("Failed to read file");
    let lines: Vec<&str> = content.lines().collect();
    
    assert_eq!(lines.len(), 2, "Should have 2 lines remaining");
    assert_eq!(lines[0], "Line3");
    assert_eq!(lines[1], "Line5");

    println!("✅ Multiple dd test passed!");
}
