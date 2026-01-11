/// Get the interminai binary path to use for testing.
///
/// This allows testing alternative implementations (e.g., Python) by setting
/// the OVERRIDE_CARGO_BIN_EXE_interminai environment variable.
///
/// For more granular testing, you can override server and client separately:
/// - OVERRIDE_CARGO_BIN_EXE_interminai_SERVER - for the daemon (start command)
/// - OVERRIDE_CARGO_BIN_EXE_interminai_CLIENT - for client commands (output, input, etc.)
///
/// # Example
///
/// ```bash
/// # Test Rust implementation (default)
/// cargo test
///
/// # Test Python implementation (both server and client)
/// OVERRIDE_CARGO_BIN_EXE_interminai=/path/to/interminai.py cargo test
///
/// # Test Python server with Rust client (isolate server bugs)
/// OVERRIDE_CARGO_BIN_EXE_interminai_SERVER=/path/to/interminai.py cargo test
///
/// # Test Rust server with Python client (isolate client bugs)
/// OVERRIDE_CARGO_BIN_EXE_interminai_CLIENT=/path/to/interminai.py cargo test
/// ```
pub fn interminai_bin() -> String {
    std::env::var("OVERRIDE_CARGO_BIN_EXE_interminai")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_interminai").to_string())
}

/// Get the interminai server binary (for start command).
/// Checks OVERRIDE_CARGO_BIN_EXE_interminai_SERVER first, then falls back to interminai_bin().
// Used in daemon_test.rs - false positive from Rust's dead code detection in test modules
#[allow(dead_code)]
pub fn interminai_server_bin() -> String {
    std::env::var("OVERRIDE_CARGO_BIN_EXE_interminai_SERVER")
        .unwrap_or_else(|_| interminai_bin())
}

/// Get the interminai client binary (for output, input, stop, etc.).
/// Checks OVERRIDE_CARGO_BIN_EXE_interminai_CLIENT first, then falls back to interminai_bin().
// Used in daemon_test.rs - false positive from Rust's dead code detection in test modules
#[allow(dead_code)]
pub fn interminai_client_bin() -> String {
    std::env::var("OVERRIDE_CARGO_BIN_EXE_interminai_CLIENT")
        .unwrap_or_else(|_| interminai_bin())
}

/// Get the terminal emulator to use for testing.
///
/// Set INTERMINAI_EMULATOR environment variable to test different backends:
/// - "xterm" (default) - full xterm-256color emulation
/// - "custom" - basic ANSI emulation
///
/// # Example
///
/// ```bash
/// # Test with xterm emulator (default)
/// cargo test
///
/// # Test with custom emulator
/// INTERMINAI_EMULATOR=custom cargo test
///
/// # Test both (run twice)
/// cargo test && INTERMINAI_EMULATOR=custom cargo test
/// ```
#[allow(dead_code)]
pub fn emulator() -> String {
    std::env::var("INTERMINAI_EMULATOR").unwrap_or_else(|_| "xterm".to_string())
}

/// Get emulator arguments to pass to the start command.
/// Returns ["--emulator", "<value>"] based on INTERMINAI_EMULATOR env var.
#[allow(dead_code)]
pub fn emulator_args() -> Vec<String> {
    vec!["--emulator".to_string(), emulator()]
}
