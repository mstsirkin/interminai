use assert_cmd::Command;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;
use std::path::PathBuf;

mod common;
use common::{interminai_bin, emulator_args, emulator};

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

    fn get_output_no_color(&self) -> String {
        let output = Command::new(interminai_bin())
            .arg("output")
            .arg("--socket")
            .arg(&self.socket_path)
            .arg("--no-color")
            .timeout(Duration::from_secs(2))
            .output()
            .expect("Failed to get output");
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn get_output_color(&self) -> String {
        let output = Command::new(interminai_bin())
            .arg("output")
            .arg("--socket")
            .arg(&self.socket_path)
            .arg("--color")
            .timeout(Duration::from_secs(2))
            .output()
            .expect("Failed to get output");
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn get_output_default(&self) -> String {
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

/// Test that --no-color returns plain text without ANSI codes
#[test]
fn test_no_color_strips_codes() {
    let env = TestEnv::new();
    // Print red "Hello" then reset
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "80x24", "\\033[31mHello\\033[0m");

    let output = daemon.get_output_no_color();
    // --no-color should NOT contain escape codes
    assert!(!output.contains("\x1b["), "--no-color should not contain ANSI codes");
    assert!(output.contains("Hello"), "Output should contain 'Hello'");

    daemon.stop();
}

/// Test that --color returns ANSI color codes for named colors
#[test]
fn test_color_named_color() {
    if emulator() == "custom" {
        // Custom backend doesn't support colors
        return;
    }

    let env = TestEnv::new();
    // Print red "Hello" (31 = red foreground)
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "80x24", "\\033[31mHello\\033[0m");

    let output = daemon.get_output_color();
    // --color should contain color code for red (31)
    assert!(output.contains("\x1b["), "--color should contain escape codes");
    assert!(output.contains("31"), "Output should contain red color code (31)");
    assert!(output.contains("Hello"), "Output should contain 'Hello'");

    daemon.stop();
}

/// Test that --color returns ANSI codes for bold text
#[test]
fn test_color_bold() {
    if emulator() == "custom" {
        return;
    }

    let env = TestEnv::new();
    // Print bold "Bold" (1 = bold)
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "80x24", "\\033[1mBold\\033[0m");

    let output = daemon.get_output_color();
    assert!(output.contains("\x1b["), "--color should contain escape codes");
    // Bold is code 1
    assert!(output.contains(";1") || output.contains("[1"), "Output should contain bold code (1)");
    assert!(output.contains("Bold"), "Output should contain 'Bold'");

    daemon.stop();
}

/// Test that --color returns ANSI codes for 256-color palette
#[test]
fn test_color_256_color() {
    if emulator() == "custom" {
        return;
    }

    let env = TestEnv::new();
    // Print with 256-color (38;5;202 = orange)
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "80x24", "\\033[38;5;202mOrange\\033[0m");

    let output = daemon.get_output_color();
    assert!(output.contains("\x1b["), "--color should contain escape codes");
    assert!(output.contains("38;5;202") || output.contains("38;5") || output.contains("38;2"),
        "Output should contain 256-color or RGB color code");
    assert!(output.contains("Orange"), "Output should contain 'Orange'");

    daemon.stop();
}

/// Test that --color returns ANSI codes for 24-bit RGB colors
#[test]
fn test_color_rgb_color() {
    if emulator() == "custom" {
        return;
    }

    let env = TestEnv::new();
    // Print with 24-bit RGB (38;2;255;128;0 = orange RGB)
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "80x24", "\\033[38;2;255;128;0mRGB\\033[0m");

    let output = daemon.get_output_color();
    assert!(output.contains("\x1b["), "--color should contain escape codes");
    assert!(output.contains("38;2;255;128;0") || output.contains("38;2"), "Output should contain RGB color code");
    assert!(output.contains("RGB"), "Output should contain 'RGB'");

    daemon.stop();
}

/// Test that --color includes background colors
#[test]
fn test_color_background_color() {
    if emulator() == "custom" {
        return;
    }

    let env = TestEnv::new();
    // Print with red background (41 = red background)
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "80x24", "\\033[41mBG\\033[0m");

    let output = daemon.get_output_color();
    assert!(output.contains("\x1b["), "--color should contain escape codes");
    assert!(output.contains("41") || output.contains("48"), "Output should contain background color code");
    assert!(output.contains("BG"), "Output should contain 'BG'");

    daemon.stop();
}

/// Test that --color works with multiple attributes
#[test]
fn test_color_multiple_attributes() {
    if emulator() == "custom" {
        return;
    }

    let env = TestEnv::new();
    // Print bold red text (1;31)
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "80x24", "\\033[1;31mBoldRed\\033[0m");

    let output = daemon.get_output_color();
    assert!(output.contains("\x1b["), "--color should contain escape codes");
    assert!(output.contains("1"), "Output should contain bold code");
    assert!(output.contains("31"), "Output should contain red code");
    assert!(output.contains("BoldRed"), "Output should contain 'BoldRed'");

    daemon.stop();
}

/// Test that custom emulator returns plain text even with --color
#[test]
fn test_custom_emulator_no_color() {
    if emulator() != "custom" {
        // This test only makes sense for custom emulator
        return;
    }

    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "80x24", "\\033[31mHello\\033[0m");

    let output = daemon.get_output_color();
    // Custom backend should NOT contain ANSI codes even with --color
    assert!(!output.contains("\x1b[31m"), "Custom backend should not emit color codes");
    assert!(output.contains("Hello"), "Output should contain 'Hello'");

    daemon.stop();
}

/// Test that plain text without colors works with both flags
#[test]
fn test_color_plain_text() {
    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "80x24", "Plain text");

    let no_color_output = daemon.get_output_no_color();
    let color_output = daemon.get_output_color();

    // Both should contain the text
    assert!(no_color_output.contains("Plain text"));
    assert!(color_output.contains("Plain text"));

    daemon.stop();
}

/// Test that default output includes color (--color is default)
#[test]
fn test_default_is_color() {
    if emulator() == "custom" {
        // Custom backend doesn't support colors
        return;
    }

    let env = TestEnv::new();
    // Print red "Hello" (31 = red foreground)
    let daemon = DaemonHandle::spawn_printf(&env.socket(), "80x24", "\\033[31mHello\\033[0m");

    let default_output = daemon.get_output_default();
    let color_output = daemon.get_output_color();

    // Default should be same as --color (both include escape codes)
    assert!(default_output.contains("\x1b["), "Default output should contain ANSI codes");
    assert!(default_output.contains("Hello"), "Default output should contain 'Hello'");
    // Both should have similar content
    assert!(color_output.contains("\x1b["), "--color output should contain ANSI codes");

    daemon.stop();
}

#[test]
fn test_color_carries_across_lines_without_numbering() {
    if emulator() == "custom" {
        return;
    }

    let env = TestEnv::new();
    let daemon = DaemonHandle::spawn_printf(
        &env.socket(),
        "80x10",
        "\\033[31mred1\nred2\n\\033[32mgreen3\ngreen4\\033[0m",
    );

    let output = daemon.get_output_color();
    let lines: Vec<&str> = output.lines().collect();

    let red1_line = lines.iter().find(|line| line.contains("red1")).expect("Missing red1 line");
    let red2_line = lines.iter().find(|line| line.contains("red2")).expect("Missing red2 line");
    let green3_line = lines.iter().find(|line| line.contains("green3")).expect("Missing green3 line");
    let green4_line = lines.iter().find(|line| line.contains("green4")).expect("Missing green4 line");

    assert_ansi_color(red1_line, "red1", "31");
    assert_ansi_color(red2_line, "red2", "31");
    assert_ansi_color(green3_line, "green3", "32");
    assert_ansi_color(green4_line, "green4", "32");

    daemon.stop();
}
