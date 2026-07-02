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

    /// Get the screen content with ANSI color codes embedded.
    /// Default implementation returns plain text (same as get_screen_content).
    fn get_screen_content_ansi(&self) -> String {
        self.get_screen_content()
    }

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

    /// Get number of lines available in scrollback history
    fn scrollback_lines(&self) -> usize { 0 }

    /// Get scrollback buffer capacity (max lines)
    fn scrollback_capacity(&self) -> usize { 0 }

    /// Get scrollback content as plain text (most recent `lines` lines before visible screen)
    fn get_scrollback_content(&self, _lines: usize) -> String { String::new() }

    /// Get scrollback content with ANSI color codes
    fn get_scrollback_content_ansi(&self, lines: usize) -> String {
        self.get_scrollback_content(lines)
    }
}
