// Terminal emulator abstraction trait
//
// This module defines the TerminalEmulator trait that abstracts terminal
// emulation implementations, allowing different backends (custom, alacritty).

/// Entry in the unhandled escape sequence debug buffer
#[derive(Clone, serde::Serialize)]
pub struct UnhandledSequence {
    pub sequence: String,
    pub raw_hex: String,
}

/// Trait abstracting terminal emulator implementations
///
/// This trait allows swapping between different terminal emulation backends
/// while keeping the PTY management and daemon architecture unchanged.
pub trait TerminalEmulator: Send {
    /// Feed bytes from PTY output to the terminal.
    /// This is the primary method for processing terminal data.
    fn process_bytes(&mut self, bytes: &[u8]);

    /// Get the screen content as a string (lines separated by newlines).
    /// Trailing whitespace on each line is trimmed.
    fn get_screen_content(&self) -> String;

    /// Get cursor position (row, col) - 0-indexed
    fn cursor_position(&self) -> (usize, usize);

    /// Get terminal dimensions (rows, cols)
    fn dimensions(&self) -> (usize, usize);

    /// Resize the terminal to new dimensions
    fn resize(&mut self, rows: usize, cols: usize);

    /// Get pending responses to send back to PTY (e.g., cursor position reports, device attributes)
    fn take_pending_responses(&mut self) -> Vec<Vec<u8>>;

    /// Get debug buffer entries (unhandled escape sequences)
    fn get_debug_entries(&self) -> Vec<UnhandledSequence>;

    /// Clear debug buffer
    fn clear_debug_buffer(&mut self);

    /// Get count of dropped debug entries (due to buffer overflow)
    fn get_debug_dropped(&self) -> usize;
}
