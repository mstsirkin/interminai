// Alacritty terminal emulator backend
//
// This module provides a terminal emulator implementation using alacritty_terminal.

use std::sync::{Arc, Mutex};
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::vte::ansi;
use alacritty_terminal::index::{Column, Line};

use crate::terminal::{TerminalEmulator, UnhandledSequence};

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
