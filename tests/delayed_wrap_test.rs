use assert_cmd::Command;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;
use std::path::PathBuf;

mod common;
use common::{interminai_bin, emulator_args};

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
    socket_path: String,
}

impl DaemonHandle {
    /// Spawn a one-shot bash command that outputs escape sequences
    fn spawn_printf(socket: &str, size: &str, printf_arg: &str) -> Self {
        let cmd_str = format!("printf '{}'; sleep 5", printf_arg);

        let mut cmd = std::process::Command::new(interminai_bin());
        cmd.arg("start")
            .args(emulator_args())
            .arg("--socket")
            .arg(socket)
            .arg("--size")
            .arg(size)
            .arg("--")
            .arg("bash")
            .arg("-c")
            .arg(&cmd_str);

        let output = cmd.output().expect("Failed to start daemon");
        if !output.status.success() {
            panic!("Daemon failed to start: {}", String::from_utf8_lossy(&output.stderr));
        }

        thread::sleep(Duration::from_millis(500));

        DaemonHandle {
            socket_path: socket.to_string()
        }
    }

    fn get_output(&self) -> String {
        let output = Command::new(interminai_bin())
            .arg("output")
            .arg("--socket")
            .arg(&self.socket_path)
            .timeout(Duration::from_secs(2))
            .output()
            .expect("Failed to get output");
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn get_cursor(&self) -> (usize, usize) {
        let output = Command::new(interminai_bin())
            .arg("output")
            .arg("--socket")
            .arg(&self.socket_path)
            .arg("--cursor")
            .arg("print")
            .timeout(Duration::from_secs(2))
            .output()
            .expect("Failed to get output");
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Parse "Cursor: row N, col M"
        for line in stdout.lines() {
            if line.starts_with("Cursor:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    let row: usize = parts[2].trim_end_matches(',').parse().unwrap_or(0);
                    let col: usize = parts[4].parse().unwrap_or(0);
                    return (row, col);
                }
            }
        }
        (0, 0)
    }

    fn stop(self) {
        let _ = std::process::Command::new(interminai_bin())
            .arg("stop")
            .arg("--socket")
            .arg(&self.socket_path)
            .output();
    }
}

/// Test that printing exactly terminal-width characters keeps cursor at last column
/// (delayed wrap - cursor doesn't wrap until next character)
#[test]
fn test_delayed_wrap_cursor_stays_at_last_column() {
    let env = TestEnv::new();
    // 10-column terminal, print exactly 10 characters
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "10x5", "ABCDEFGHIJ");

    let (row, col) = daemon.get_cursor();
    // Cursor should be at row 1, col 10 (1-based) - last column, not wrapped
    assert_eq!(row, 1, "Cursor should stay on row 1");
    assert_eq!(col, 10, "Cursor should be at last column (10)");

    let output = daemon.get_output();
    let first_line = output.lines().next().unwrap_or("");
    assert_eq!(first_line, "ABCDEFGHIJ", "All 10 chars should be on first line");

    daemon.stop();
}

/// Test that the next character after filling the line causes wrap
#[test]
fn test_delayed_wrap_next_char_wraps() {
    let env = TestEnv::new();
    // 10-column terminal, print 11 characters
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "10x5", "ABCDEFGHIJK");

    let (row, col) = daemon.get_cursor();
    // K should wrap to row 2, col 2 (after printing at col 1)
    assert_eq!(row, 2, "Cursor should be on row 2 after wrap");
    assert_eq!(col, 2, "Cursor should be at col 2 after printing K");

    let output = daemon.get_output();
    let lines: Vec<&str> = output.lines().collect();
    assert!(lines.len() >= 2, "Should have at least 2 lines");
    assert_eq!(lines[0], "ABCDEFGHIJ", "First line should have 10 chars");
    assert!(lines[1].starts_with("K"), "Second line should start with K");

    daemon.stop();
}

/// Test that control character (CR) cancels pending wrap
#[test]
fn test_delayed_wrap_cancelled_by_cr() {
    let env = TestEnv::new();
    // 10-column terminal, print 10 chars, then CR, then X
    // CR should cancel pending wrap and move to column 1 of same row
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "10x5", "ABCDEFGHIJ\\rX");

    let output = daemon.get_output();
    let first_line = output.lines().next().unwrap_or("");
    // X should overwrite A at the start of line 1
    assert!(first_line.starts_with("X"), "X should be at start of line 1");
    assert!(first_line.contains("BCDEFGHIJ"), "Rest of line should remain");

    daemon.stop();
}

/// Test that CSI cursor movement cancels pending wrap
#[test]
fn test_delayed_wrap_cancelled_by_csi() {
    let env = TestEnv::new();
    // 10-column terminal, print 10 chars, then cursor up, then move to col 1, then X
    // CSI A (cursor up) should cancel pending wrap
    // After cursor up, we're at line 2 col 10, so move to col 1 to write X at start
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "10x5", "\\n\\nABCDEFGHIJ\\e[1A\\e[1GX");

    let output = daemon.get_output();
    let lines: Vec<&str> = output.lines().collect();
    // Line 3 (index 2) has ABCDEFGHIJ, cursor up moves to line 2 (index 1)
    // X should be on line 2 col 1, not cause a wrap
    assert!(lines.len() >= 2, "Should have at least 2 lines");
    assert!(lines[1].starts_with("X"), "X should be at start of line 2 after cursor up");

    daemon.stop();
}

/// Test that newline after full line works correctly
#[test]
fn test_delayed_wrap_with_newline() {
    let env = TestEnv::new();
    // 10-column terminal, print 10 chars, newline, then Y
    // Newline should cancel pending wrap and move to next line
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "10x5", "ABCDEFGHIJ\\nY");

    let output = daemon.get_output();
    let lines: Vec<&str> = output.lines().collect();
    assert!(lines.len() >= 2, "Should have at least 2 lines");
    assert_eq!(lines[0], "ABCDEFGHIJ", "First line should have 10 chars");
    assert!(lines[1].starts_with("Y"), "Y should be at start of line 2");

    daemon.stop();
}

/// Test spinner-like pattern: print full line, cursor up, erase, reprint
/// This simulates what Claude Code's spinner does
#[test]
fn test_spinner_pattern() {
    let env = TestEnv::new();
    // 20-column terminal
    // Print a full 20-char line, then cursor up, erase line, print new content
    // This should result in just the new content on line 1
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "20x5",
        "12345678901234567890\\e[1A\\e[2K\\e[1GNew content here");

    let output = daemon.get_output();
    let first_line = output.lines().next().unwrap_or("");
    // Should have "New content here", not the original line
    assert!(first_line.contains("New content here"),
        "Should show new content. Line: '{}'", first_line);
    assert!(!first_line.contains("1234567890"),
        "Old content should be erased. Line: '{}'", first_line);

    daemon.stop();
}

/// Test multiple full lines followed by cursor-up operations
/// Simulates updating multiple spinner lines in place
#[test]
fn test_multi_line_update_in_place() {
    let env = TestEnv::new();
    // 10-column terminal
    // Print 2 lines, move up 1 to line 1, erase and rewrite, move down, erase and rewrite
    // Line 1: AAAAAAAAAA, Line 2: BBBBBBBBBB
    // After \n\n we're on line 3, move up 2 to line 1, erase and write LINE1
    // Then move down 1 to line 2, erase and write LINE2
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "10x5",
        "AAAAAAAAAA\\nBBBBBBBBBB\\n\\e[2A\\e[2K\\e[1GLINE1\\e[1B\\e[2K\\e[1GLINE2");

    let output = daemon.get_output();
    let lines: Vec<&str> = output.lines().collect();
    assert!(lines.len() >= 2, "Should have at least 2 lines. Output:\n{}", output);
    assert!(lines[0].starts_with("LINE1"), "First line should be LINE1. Got: '{}'", lines[0]);
    assert!(lines[1].starts_with("LINE2"), "Second line should be LINE2. Got: '{}'", lines[1]);

    daemon.stop();
}

/// Test backspace behavior after filling line exactly (pending wrap state).
///
/// Terminals differ in how they handle backspace at the right margin:
///
/// 1. DEC-compliant (XTerm, Alacritty, real VT100/220/420/510):
///    - Backspace cancels pending wrap AND moves cursor back one column
///    - Result: "ABCDEFGHIJ" + BS + "X" -> "ABCDEFGHXJ" (X overwrites I)
///
/// 2. Non-DEC (rxvt, PuTTY, Konsole):
///    - Backspace cancels pending wrap but does NOT move cursor (BS is "absorbed")
///    - Result: "ABCDEFGHIJ" + BS + "X" -> "ABCDEFGHIX" (X overwrites J)
///
/// We follow DEC/XTerm behavior as verified by wraptest (github.com/mattiase/wraptest).
#[test]
fn test_backspace_after_exact_fill() {
    let env = TestEnv::new();
    // 10-column terminal, print 10 chars, backspace, X
    // DEC-compliant: backspace cancels pending wrap and moves cursor to col 9 (0-indexed: 8)
    // X should overwrite I, resulting in ABCDEFGHXJ
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "10x5", "ABCDEFGHIJ\\bX");

    let output = daemon.get_output();
    let first_line = output.lines().next().unwrap_or("");
    assert_eq!(first_line, "ABCDEFGHXJ",
        "Backspace should cancel pending wrap and X should overwrite 'I'. Got: '{}'", first_line);

    let (row, col) = daemon.get_cursor();
    assert_eq!(row, 1, "Cursor should stay on row 1");
    assert_eq!(col, 10, "Cursor should be at col 10 after writing X");

    daemon.stop();
}

/// Test that exactly 80 characters on 80-column terminal works correctly
/// This is the specific case that was broken before the delayed wrap fix
#[test]
fn test_80_char_line_on_80_col_terminal() {
    let env = TestEnv::new();
    // Create exactly 80 'X' characters
    let line_80 = "X".repeat(80);
    let printf_arg = format!("{}\\e[1A\\e[2K\\e[1GUpdated", line_80);

    let daemon = DaemonHandle::spawn_printf(&env.socket(), "80x5", &printf_arg);

    let output = daemon.get_output();
    let first_line = output.lines().next().unwrap_or("");
    // Should have "Updated" on line 1, not the 80 X's
    assert!(first_line.starts_with("Updated"),
        "Should show 'Updated' after cursor-up and erase. Line: '{}'", first_line);
    // Should NOT have wrapped to create an extra empty line
    assert!(!first_line.contains("XXXX"),
        "Old content should be gone. Line: '{}'", first_line);

    daemon.stop();
}
