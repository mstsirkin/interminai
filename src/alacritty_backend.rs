// Alacritty terminal emulator backend
//
// This module provides a terminal emulator implementation using alacritty_terminal.

use std::sync::{Arc, Mutex};
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::vte::ansi::{self, Color, NamedColor};
use alacritty_terminal::index::{Column, Line};

use crate::terminal::{TerminalEmulator, UnhandledSequence};

/// Display-related flags that affect ANSI output (excludes internal flags like WRAPLINE)
fn display_flags(flags: Flags) -> Flags {
    flags & (Flags::BOLD | Flags::DIM | Flags::ITALIC | Flags::UNDERLINE
           | Flags::INVERSE | Flags::HIDDEN | Flags::STRIKEOUT)
}

/// Simple dimensions struct for creating the terminal
struct TermDimensions {
    columns: usize,
    screen_lines: usize,
}

impl Dimensions for TermDimensions {
    fn total_lines(&self) -> usize {
        self.screen_lines
    }

    fn screen_lines(&self) -> usize {
        self.screen_lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// Event listener that captures PtyWrite events for responses
pub struct ResponseCapturingListener {
    responses: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl ResponseCapturingListener {
    fn new() -> (Self, Arc<Mutex<Vec<Vec<u8>>>>) {
        let responses = Arc::new(Mutex::new(Vec::new()));
        (Self { responses: responses.clone() }, responses)
    }
}

impl EventListener for ResponseCapturingListener {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(data) = event {
            if let Ok(mut responses) = self.responses.lock() {
                responses.push(data.into_bytes());
            }
        }
    }
}

/// Alacritty-based terminal emulator implementation
pub struct AlacrittyTerminal {
    term: Term<ResponseCapturingListener>,
    parser: ansi::Processor,
    responses: Arc<Mutex<Vec<Vec<u8>>>>,
    rows: usize,
    cols: usize,
}

impl AlacrittyTerminal {
    pub fn new(rows: usize, cols: usize) -> Self {
        let config = Config::default();
        let dimensions = TermDimensions {
            columns: cols,
            screen_lines: rows,
        };

        let (listener, responses) = ResponseCapturingListener::new();
        let term = Term::new(config, &dimensions, listener);
        let parser = ansi::Processor::new();

        AlacrittyTerminal {
            term,
            parser,
            responses,
            rows,
            cols,
        }
    }
}

/// Build ANSI SGR escape sequence from color and flags
fn build_sgr_sequence(fg: &Color, bg: &Color, flags: Flags) -> String {
    let mut codes: Vec<String> = Vec::new();

    // Reset first, then apply attributes
    codes.push("0".to_string());

    // Text attributes from flags
    if flags.contains(Flags::BOLD) {
        codes.push("1".to_string());
    }
    if flags.contains(Flags::DIM) {
        codes.push("2".to_string());
    }
    if flags.contains(Flags::ITALIC) {
        codes.push("3".to_string());
    }
    if flags.contains(Flags::UNDERLINE) {
        codes.push("4".to_string());
    }
    if flags.contains(Flags::INVERSE) {
        codes.push("7".to_string());
    }
    if flags.contains(Flags::HIDDEN) {
        codes.push("8".to_string());
    }
    if flags.contains(Flags::STRIKEOUT) {
        codes.push("9".to_string());
    }

    // Foreground color
    if let Some(fg_code) = color_to_ansi(fg, true) {
        codes.push(fg_code);
    }

    // Background color
    if let Some(bg_code) = color_to_ansi(bg, false) {
        codes.push(bg_code);
    }

    if codes.len() == 1 && codes[0] == "0" {
        // Just reset, no other attributes - return empty to avoid unnecessary codes
        return String::new();
    }

    format!("\x1b[{}m", codes.join(";"))
}

/// Convert Color to ANSI code string
fn color_to_ansi(color: &Color, is_foreground: bool) -> Option<String> {
    match color {
        Color::Named(named) => named_color_to_ansi(*named, is_foreground),
        Color::Indexed(idx) => {
            let prefix = if is_foreground { "38;5" } else { "48;5" };
            Some(format!("{};{}", prefix, idx))
        }
        Color::Spec(rgb) => {
            let prefix = if is_foreground { "38;2" } else { "48;2" };
            Some(format!("{};{};{};{}", prefix, rgb.r, rgb.g, rgb.b))
        }
    }
}

/// Convert NamedColor to ANSI code
fn named_color_to_ansi(color: NamedColor, is_foreground: bool) -> Option<String> {
    let code = match color {
        // Standard colors (30-37 fg, 40-47 bg)
        NamedColor::Black => Some(if is_foreground { 30 } else { 40 }),
        NamedColor::Red => Some(if is_foreground { 31 } else { 41 }),
        NamedColor::Green => Some(if is_foreground { 32 } else { 42 }),
        NamedColor::Yellow => Some(if is_foreground { 33 } else { 43 }),
        NamedColor::Blue => Some(if is_foreground { 34 } else { 44 }),
        NamedColor::Magenta => Some(if is_foreground { 35 } else { 45 }),
        NamedColor::Cyan => Some(if is_foreground { 36 } else { 46 }),
        NamedColor::White => Some(if is_foreground { 37 } else { 47 }),
        // Bright colors (90-97 fg, 100-107 bg)
        NamedColor::BrightBlack => Some(if is_foreground { 90 } else { 100 }),
        NamedColor::BrightRed => Some(if is_foreground { 91 } else { 101 }),
        NamedColor::BrightGreen => Some(if is_foreground { 92 } else { 102 }),
        NamedColor::BrightYellow => Some(if is_foreground { 93 } else { 103 }),
        NamedColor::BrightBlue => Some(if is_foreground { 94 } else { 104 }),
        NamedColor::BrightMagenta => Some(if is_foreground { 95 } else { 105 }),
        NamedColor::BrightCyan => Some(if is_foreground { 96 } else { 106 }),
        NamedColor::BrightWhite => Some(if is_foreground { 97 } else { 107 }),
        // Default/special colors - don't emit codes
        NamedColor::Foreground | NamedColor::Background | NamedColor::Cursor => None,
        // Dim colors map to standard + dim attribute (handled by flags)
        NamedColor::DimBlack => Some(if is_foreground { 30 } else { 40 }),
        NamedColor::DimRed => Some(if is_foreground { 31 } else { 41 }),
        NamedColor::DimGreen => Some(if is_foreground { 32 } else { 42 }),
        NamedColor::DimYellow => Some(if is_foreground { 33 } else { 43 }),
        NamedColor::DimBlue => Some(if is_foreground { 34 } else { 44 }),
        NamedColor::DimMagenta => Some(if is_foreground { 35 } else { 45 }),
        NamedColor::DimCyan => Some(if is_foreground { 36 } else { 46 }),
        NamedColor::DimWhite => Some(if is_foreground { 37 } else { 47 }),
        // Fallback for any other named colors
        _ => None,
    };
    code.map(|c| c.to_string())
}

/// Trim trailing spaces from a line while preserving ANSI escape codes at the end
fn trim_end_preserve_ansi(s: &str) -> &str {
    // Find last non-space, non-escape-sequence character
    let bytes = s.as_bytes();
    let mut end = bytes.len();

    // Walk backwards, skipping trailing spaces
    while end > 0 {
        if bytes[end - 1] == b' ' {
            end -= 1;
        } else if end >= 4 && bytes[end - 1] == b'm' {
            // Might be end of ANSI sequence, look for ESC[
            let mut seq_start = end - 2;
            while seq_start > 0 && bytes[seq_start] != 0x1b {
                seq_start -= 1;
            }
            if seq_start > 0 && bytes[seq_start] == 0x1b && bytes[seq_start + 1] == b'[' {
                // This is an ANSI sequence at the end, keep it and continue trimming before it
                end = seq_start;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // Now find actual content end (include any ANSI codes after last content)
    let content_end = end;
    end = s.len();

    // Keep ANSI reset codes at the very end
    if content_end == 0 {
        return "";
    }

    // Return up to and including any trailing ANSI codes
    &s[..end]
}

impl TerminalEmulator for AlacrittyTerminal {
    fn process_bytes(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    fn get_screen_content(&self) -> String {
        let grid = self.term.grid();
        let mut result = String::new();

        for line_idx in 0..grid.screen_lines() {
            let line = &grid[Line(line_idx as i32)];
            let line_str: String = (0..grid.columns())
                .filter_map(|col| {
                    let cell = &line[Column(col)];
                    // Skip wide char spacer cells (placeholder after wide char)
                    if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                        None
                    } else {
                        Some(cell.c)
                    }
                })
                .collect();
            result.push_str(line_str.trim_end());
            result.push('\n');
        }

        result
    }

    fn get_screen_content_ansi(&self) -> String {
        let grid = self.term.grid();
        let mut result = String::new();

        // Default colors for comparison
        let default_fg = Color::Named(NamedColor::Foreground);
        let default_bg = Color::Named(NamedColor::Background);
        let empty_flags = Flags::empty();

        for line_idx in 0..grid.screen_lines() {
            let line = &grid[Line(line_idx as i32)];
            let mut line_content = String::new();
            let mut current_fg = default_fg;
            let mut current_bg = default_bg;
            let mut current_flags = empty_flags;

            for col in 0..grid.columns() {
                let cell = &line[Column(col)];

                // Skip wide char spacer cells
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }

                // Check if we need to emit SGR codes (only compare display-related flags)
                let cell_display_flags = display_flags(cell.flags);
                let need_sgr = cell.fg != current_fg
                    || cell.bg != current_bg
                    || cell_display_flags != current_flags;

                if need_sgr {
                    let sgr = build_sgr_sequence(&cell.fg, &cell.bg, cell.flags);
                    if !sgr.is_empty() {
                        line_content.push_str(&sgr);
                    }
                    current_fg = cell.fg;
                    current_bg = cell.bg;
                    current_flags = cell_display_flags;
                }

                line_content.push(cell.c);
            }

            // Reset at end of line if we changed any attributes
            if current_fg != default_fg || current_bg != default_bg || current_flags != empty_flags {
                line_content.push_str("\x1b[0m");
            }

            // Trim trailing spaces but preserve ANSI codes
            let trimmed = trim_end_preserve_ansi(&line_content);
            result.push_str(trimmed);
            result.push('\n');
        }

        result
    }

    fn cursor_position(&self) -> (usize, usize) {
        let cursor = self.term.grid().cursor.point;
        (cursor.line.0 as usize, cursor.column.0)
    }

    fn dimensions(&self) -> (usize, usize) {
        (self.rows, self.cols)
    }

    fn resize(&mut self, rows: usize, cols: usize) {
        let dimensions = TermDimensions {
            columns: cols,
            screen_lines: rows,
        };
        self.term.resize(dimensions);
        self.rows = rows;
        self.cols = cols;
    }

    fn take_pending_responses(&mut self) -> Vec<Vec<u8>> {
        if let Ok(mut responses) = self.responses.lock() {
            std::mem::take(&mut *responses)
        } else {
            Vec::new()
        }
    }

    fn get_debug_entries(&self) -> Vec<UnhandledSequence> {
        // alacritty_terminal handles most sequences, so we don't track unhandled ones
        Vec::new()
    }

    fn clear_debug_buffer(&mut self) {
        // No-op for alacritty backend
    }

    fn get_debug_dropped(&self) -> usize {
        0
    }
}
