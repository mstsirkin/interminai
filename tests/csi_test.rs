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

    fn stop(self) {
        let _ = std::process::Command::new(interminai_bin())
            .arg("stop")
            .arg("--socket")
            .arg(&self.socket_path)
            .output();
    }
}

/// Test CSI G - horizontal position absolute (hpa)
#[test]
fn test_csi_hpa_horizontal_position() {
    let env = TestEnv::new();
    // Move to column 10, print X
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "40x10", "\\e[10GX");

    let output = daemon.get_output();
    assert!(output.lines().any(|l| l.starts_with("         X")),
        "X should be at column 10 (9 spaces before). Output:\n{}", output);

    daemon.stop();
}

/// Test CSI d - vertical position absolute (vpa)
#[test]
fn test_csi_vpa_vertical_position() {
    let env = TestEnv::new();
    // Move to row 3, print Y
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "40x10", "\\e[3dY");

    let output = daemon.get_output();
    let lines: Vec<&str> = output.lines().collect();
    assert!(lines.len() > 2 && lines[2].starts_with("Y"),
        "Y should be at row 3 (index 2). Lines:\n{:?}", lines);

    daemon.stop();
}

/// Test CSI K mode 1 - erase from beginning of line to cursor (el1)
#[test]
fn test_csi_el1_erase_to_beginning() {
    let env = TestEnv::new();
    // Print ABCDEFGH, move to col 5, erase to beginning
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "40x10", "ABCDEFGH\\e[5G\\e[1K");

    let output = daemon.get_output();
    // Cols 1-5 erased, FGH remains
    let first_line = output.lines().next().unwrap_or("");
    assert!(first_line.contains("FGH") && !first_line.contains("ABCDE"),
        "ABCDE should be erased, FGH remains. Line: '{}'", first_line);

    daemon.stop();
}

/// Test CSI X - erase character (ech)
#[test]
fn test_csi_ech_erase_character() {
    let env = TestEnv::new();
    // Print ABCDEFGH, move to col 3, erase 3 chars
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "40x10", "ABCDEFGH\\e[3G\\e[3X");

    let output = daemon.get_output();
    let first_line = output.lines().next().unwrap_or("");
    // Cols 3-5 erased: "AB   FGH"
    assert!(first_line.starts_with("AB") && first_line.contains("FGH"),
        "Should have AB___FGH. Line: '{}'", first_line);

    daemon.stop();
}

/// Test CSI P - delete character (dch)
#[test]
fn test_csi_dch_delete_character() {
    let env = TestEnv::new();
    // Print ABCDEFGH, move to col 3, delete 2 chars
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "40x10", "ABCDEFGH\\e[3G\\e[2P");

    let output = daemon.get_output();
    let first_line = output.lines().next().unwrap_or("");
    // CD deleted, rest shifts left: ABEFGH
    assert!(first_line.starts_with("ABEFGH"),
        "Should have ABEFGH. Line: '{}'", first_line);

    daemon.stop();
}

/// Test CSI @ - insert character (ich)
#[test]
fn test_csi_ich_insert_character() {
    let env = TestEnv::new();
    // Print ABCDEF, move to col 3, insert 2 blanks
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "40x10", "ABCDEF\\e[3G\\e[2@");

    let output = daemon.get_output();
    let first_line = output.lines().next().unwrap_or("");
    // 2 blanks inserted at col 3: "AB  CDEF"
    assert!(first_line.starts_with("AB  CD"),
        "Should have AB__CD. Line: '{}'", first_line);

    daemon.stop();
}

/// Test CSI S - scroll up
#[test]
fn test_csi_scroll_up() {
    let env = TestEnv::new();
    // Print 3 lines, then scroll up 2
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "40x10",
        "LINE1\\nLINE2\\nLINE3\\e[2S");

    let output = daemon.get_output();
    // LINE1 and LINE2 scrolled off, LINE3 remains
    assert!(output.contains("LINE3"), "LINE3 should remain");
    assert!(!output.contains("LINE1"), "LINE1 should be scrolled off. Output:\n{}", output);

    daemon.stop();
}

/// Test CSI T - scroll down
#[test]
fn test_csi_scroll_down() {
    let env = TestEnv::new();
    // Print TOPLINE at row 1, scroll down 2
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "40x10", "TOPLINE\\e[2T");

    let output = daemon.get_output();
    let lines: Vec<&str> = output.lines().collect();
    // TOPLINE pushed to row 3 (index 2)
    assert!(lines.len() > 2 && lines[2].contains("TOPLINE"),
        "TOPLINE should be at row 3. Lines:\n{:?}", lines);

    daemon.stop();
}

/// Test CSI I - cursor horizontal tab forward (cht)
#[test]
fn test_csi_horizontal_tab() {
    let env = TestEnv::new();
    // Move to col 3, forward tab (to col 9), print X
    // Tab stops: 1, 9, 17, 25... (1-based) = 0, 8, 16, 24... (0-indexed)
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "40x10", "\\e[3G\\e[IX");

    let output = daemon.get_output();
    let first_line = output.lines().next().unwrap_or("");
    let x_pos = first_line.find('X');
    // Forward tab from col 3 (index 2) goes to col 9 (index 8)
    assert_eq!(x_pos, Some(8),
        "X should be at index 8. Line: '{}'", first_line);

    daemon.stop();
}

/// Test CSI I with count - multiple forward tabs
#[test]
fn test_csi_horizontal_tab_count() {
    let env = TestEnv::new();
    // Move to col 1, forward tab 2 times (to col 17), print X
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "40x10", "\\e[1G\\e[2IX");

    let output = daemon.get_output();
    let first_line = output.lines().next().unwrap_or("");
    let x_pos = first_line.find('X');
    // 2 forward tabs from col 1 (index 0): 0->8->16
    assert_eq!(x_pos, Some(16),
        "X should be at index 16 after 2 tabs. Line: '{}'", first_line);

    daemon.stop();
}

/// Test CSI Z - back tab (cbt)
#[test]
fn test_csi_back_tab() {
    let env = TestEnv::new();
    // Move to col 20, back tab (to col 17), print X
    // Tab stops: 1, 9, 17, 25... (1-based) = 0, 8, 16, 24... (0-indexed)
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "40x10", "\\e[20G\\e[ZX");

    let output = daemon.get_output();
    let first_line = output.lines().next().unwrap_or("");
    let x_pos = first_line.find('X');
    // Back tab from col 20 (index 19) goes to col 17 (index 16)
    assert_eq!(x_pos, Some(16),
        "X should be at index 16. Line: '{}'", first_line);

    daemon.stop();
}

/// Test CSI b - repeat character
#[test]
fn test_csi_repeat_character() {
    let env = TestEnv::new();
    // Print A, repeat 5 times
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "40x10", "A\\e[5b");

    let output = daemon.get_output();
    // 1 original + 5 repeats = AAAAAA
    assert!(output.contains("AAAAAA"),
        "Should have 6 A's. Output:\n{}", output);

    daemon.stop();
}

/// Test repeat after SGR (escape shouldn't reset last_char)
#[test]
fn test_csi_repeat_after_sgr() {
    let env = TestEnv::new();
    // Print X, SGR bold (ignored), repeat 3 times
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "40x10", "X\\e[1m\\e[3b");

    let output = daemon.get_output();
    // SGR doesn't affect last_char, so should have XXXX
    assert!(output.contains("XXXX"),
        "Should have 4 X's. Output:\n{}", output);

    daemon.stop();
}

/// Test Unicode support
#[test]
fn test_unicode_support() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "40x10", "日本語 ├── test");

    let output = daemon.get_output();
    assert!(output.contains("日本語"), "Should display Japanese");
    assert!(output.contains("├──"), "Should display box-drawing");

    daemon.stop();
}
