// Custom terminal emulator implementation (legacy)
//
// This is the original terminal emulator that was extracted from main.rs.
// It uses the vte crate for parsing ANSI escape sequences.

use vte::Perform;
use crate::terminal::{TerminalEmulator, UnhandledSequence};

/// Ring buffer for tracking unhandled escape sequences
struct DebugBuffer {
    entries: Vec<UnhandledSequence>,
    capacity: usize,
    dropped: usize,
}

impl DebugBuffer {
    fn new(capacity: usize) -> Self {
        DebugBuffer {
            entries: Vec::with_capacity(capacity),
            capacity,
            dropped: 0,
        }
    }

    fn push(&mut self, sequence: String, raw_bytes: &[u8]) {
        let raw_hex = raw_bytes.iter().map(|b| format!("{:02x}", b)).collect();
        let entry = UnhandledSequence { sequence, raw_hex };

        if self.entries.len() >= self.capacity {
            self.entries.remove(0);
            self.dropped += 1;
        }
        self.entries.push(entry);
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.dropped = 0;
    }

    fn get_entries(&self) -> &[UnhandledSequence] {
        &self.entries
    }

    fn get_dropped(&self) -> usize {
        self.dropped
    }
}

/// Custom terminal screen buffer implementation
pub struct CustomScreen {
    rows: usize,
    cols: usize,
    cells: Vec<Vec<char>>,
    cursor_row: usize,
    cursor_col: usize,
    last_char: char,
    debug_buffer: DebugBuffer,
    pending_responses: Vec<Vec<u8>>,
    parser: vte::Parser,
}

impl CustomScreen {
    pub fn new(rows: usize, cols: usize) -> Self {
        Self::with_debug_buffer(rows, cols, 10)
    }

    pub fn with_debug_buffer(rows: usize, cols: usize, debug_buffer_size: usize) -> Self {
        CustomScreen {
            rows,
            cols,
            cells: vec![vec![' '; cols]; rows],
            cursor_row: 0,
            cursor_col: 0,
            last_char: ' ',
            debug_buffer: DebugBuffer::new(debug_buffer_size),
            pending_responses: Vec::new(),
            parser: vte::Parser::new(),
        }
    }

    fn to_ascii(&self) -> String {
        let mut result = String::new();
        for row in &self.cells {
            let line: String = row.iter().collect();
            result.push_str(&line.trim_end());
            result.push('\n');
        }
        result
    }

    fn scroll_up(&mut self) {
        self.cells.remove(0);
        self.cells.push(vec![' '; self.cols]);
    }
}

impl TerminalEmulator for CustomScreen {
    fn process_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            // We need to use a trick here since vte::Parser::advance takes &mut self
            // and we need to pass self as the Perform implementor
            let mut parser = std::mem::take(&mut self.parser);
            parser.advance(self, *byte);
            self.parser = parser;
        }
    }

    fn get_screen_content(&self) -> String {
        self.to_ascii()
    }

    fn cursor_position(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    fn dimensions(&self) -> (usize, usize) {
        (self.rows, self.cols)
    }

    fn resize(&mut self, rows: usize, cols: usize) {
        let mut new_cells = vec![vec![' '; cols]; rows];
        for row in 0..self.rows.min(rows) {
            for col in 0..self.cols.min(cols) {
                new_cells[row][col] = self.cells[row][col];
            }
        }
        self.cells = new_cells;
        self.rows = rows;
        self.cols = cols;
        self.cursor_row = self.cursor_row.min(rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(cols.saturating_sub(1));
    }

    fn take_pending_responses(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.pending_responses)
    }

    fn get_debug_entries(&self) -> Vec<UnhandledSequence> {
        self.debug_buffer.get_entries().to_vec()
    }

    fn clear_debug_buffer(&mut self) {
        self.debug_buffer.clear();
    }

    fn get_debug_dropped(&self) -> usize {
        self.debug_buffer.get_dropped()
    }
}

impl Perform for CustomScreen {
    fn print(&mut self, c: char) {
        self.last_char = c;
        if self.cursor_row < self.rows && self.cursor_col < self.cols {
            self.cells[self.cursor_row][self.cursor_col] = c;
            self.cursor_col += 1;
            if self.cursor_col >= self.cols {
                self.cursor_col = 0;
                self.cursor_row += 1;
                if self.cursor_row >= self.rows {
                    self.scroll_up();
                    self.cursor_row = self.rows - 1;
                }
            }
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => {
                self.cursor_row += 1;
                if self.cursor_row >= self.rows {
                    self.scroll_up();
                    self.cursor_row = self.rows - 1;
                }
                self.cursor_col = 0;
            }
            b'\r' => {
                self.cursor_col = 0;
            }
            b'\t' => {
                self.cursor_col = ((self.cursor_col / 8) + 1) * 8;
                if self.cursor_col >= self.cols {
                    self.cursor_col = self.cols - 1;
                }
            }
            b'\x08' => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
            }
            _ => {}
        }
    }

    fn hook(&mut self, _: &vte::Params, _: &[u8], _: bool, _: char) {}
    fn put(&mut self, _: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _: &[&[u8]], _: bool) {}

    fn csi_dispatch(&mut self, params: &vte::Params, intermediates: &[u8], _ignore: bool, action: char) {
        match action {
            'H' | 'f' => {
                let row = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(1).saturating_sub(1) as usize;
                let col = params.iter().nth(1).and_then(|p| p.first()).copied().unwrap_or(1).saturating_sub(1) as usize;
                self.cursor_row = row.min(self.rows - 1);
                self.cursor_col = col.min(self.cols - 1);
            }
            'A' => {
                let n = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(1).max(1) as usize;
                self.cursor_row = self.cursor_row.saturating_sub(n);
            }
            'B' => {
                let n = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(1).max(1) as usize;
                self.cursor_row = (self.cursor_row + n).min(self.rows - 1);
            }
            'C' => {
                let n = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(1).max(1) as usize;
                self.cursor_col = (self.cursor_col + n).min(self.cols - 1);
            }
            'D' => {
                let n = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(1).max(1) as usize;
                self.cursor_col = self.cursor_col.saturating_sub(n);
            }
            'G' => {
                let col = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(1).saturating_sub(1) as usize;
                self.cursor_col = col.min(self.cols - 1);
            }
            'd' => {
                let row = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(1).saturating_sub(1) as usize;
                self.cursor_row = row.min(self.rows - 1);
            }
            'J' => {
                let mode = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(0);
                match mode {
                    0 => {
                        for col in self.cursor_col..self.cols {
                            self.cells[self.cursor_row][col] = ' ';
                        }
                        for row in (self.cursor_row + 1)..self.rows {
                            for col in 0..self.cols {
                                self.cells[row][col] = ' ';
                            }
                        }
                    }
                    2 => {
                        for row in 0..self.rows {
                            for col in 0..self.cols {
                                self.cells[row][col] = ' ';
                            }
                        }
                        self.cursor_row = 0;
                        self.cursor_col = 0;
                    }
                    _ => {}
                }
            }
            'K' => {
                let mode = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(0);
                match mode {
                    0 => {
                        for col in self.cursor_col..self.cols {
                            self.cells[self.cursor_row][col] = ' ';
                        }
                    }
                    1 => {
                        for col in 0..=self.cursor_col {
                            self.cells[self.cursor_row][col] = ' ';
                        }
                    }
                    2 => {
                        for col in 0..self.cols {
                            self.cells[self.cursor_row][col] = ' ';
                        }
                    }
                    _ => {}
                }
            }
            'M' => {
                let n = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(1).max(1) as usize;
                for _ in 0..n {
                    if self.cursor_row < self.rows {
                        self.cells.remove(self.cursor_row);
                        self.cells.push(vec![' '; self.cols]);
                    }
                }
            }
            'L' => {
                let n = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(1).max(1) as usize;
                for _ in 0..n {
                    if self.cursor_row < self.rows {
                        self.cells.pop();
                        self.cells.insert(self.cursor_row, vec![' '; self.cols]);
                    }
                }
            }
            'P' => {
                let n = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(1).max(1) as usize;
                let row = self.cursor_row;
                for _ in 0..n {
                    if self.cursor_col < self.cols {
                        self.cells[row].remove(self.cursor_col);
                        self.cells[row].push(' ');
                    }
                }
            }
            '@' => {
                let n = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(1).max(1) as usize;
                let row = self.cursor_row;
                for _ in 0..n {
                    if self.cursor_col < self.cols {
                        self.cells[row].pop();
                        self.cells[row].insert(self.cursor_col, ' ');
                    }
                }
            }
            'X' => {
                let n = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(1).max(1) as usize;
                for i in 0..n {
                    let col = self.cursor_col + i;
                    if col < self.cols {
                        self.cells[self.cursor_row][col] = ' ';
                    }
                }
            }
            'S' => {
                let n = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(1).max(1) as usize;
                for _ in 0..n {
                    self.scroll_up();
                }
            }
            'T' => {
                let n = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(1).max(1) as usize;
                for _ in 0..n {
                    self.cells.pop();
                    self.cells.insert(0, vec![' '; self.cols]);
                }
            }
            'I' => {
                let n = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(1).max(1) as usize;
                for _ in 0..n {
                    self.cursor_col = ((self.cursor_col / 8) + 1) * 8;
                    if self.cursor_col >= self.cols {
                        self.cursor_col = self.cols - 1;
                        break;
                    }
                }
            }
            'Z' => {
                if self.cursor_col > 0 {
                    self.cursor_col = ((self.cursor_col - 1) / 8) * 8;
                }
            }
            'b' => {
                let n = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(1).max(1) as usize;
                let c = self.last_char;
                for _ in 0..n {
                    self.print(c);
                }
            }
            'g' => {
                // Clear Tab Stop - we use fixed 8-column tabs, so ignore
            }
            'm' => {
                // SGR - ignore (colors/attributes)
            }
            'n' => {
                let mode = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(0);
                match mode {
                    5 => {
                        self.pending_responses.push(b"\x1b[0n".to_vec());
                    }
                    6 => {
                        let response = format!("\x1b[{};{}R", self.cursor_row + 1, self.cursor_col + 1);
                        self.pending_responses.push(response.into_bytes());
                    }
                    _ => {}
                }
            }
            'c' => {
                let mode = params.iter().nth(0).and_then(|p| p.first()).copied().unwrap_or(0);
                if mode == 0 {
                    self.pending_responses.push(b"\x1b[?1;2c".to_vec());
                }
            }
            _ => {
                let mut seq = String::from("\\e[");
                for intermediate in intermediates {
                    seq.push(*intermediate as char);
                }
                let param_strs: Vec<String> = params.iter()
                    .map(|p| p.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(":"))
                    .collect();
                seq.push_str(&param_strs.join(";"));
                seq.push(action);

                let mut raw = vec![0x1b, b'['];
                raw.extend_from_slice(intermediates);
                for (i, p) in params.iter().enumerate() {
                    if i > 0 { raw.push(b';'); }
                    for (j, v) in p.iter().enumerate() {
                        if j > 0 { raw.push(b':'); }
                        raw.extend_from_slice(v.to_string().as_bytes());
                    }
                }
                raw.push(action as u8);

                self.debug_buffer.push(seq, &raw);
            }
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        match byte {
            b'H' => {
                // Set Tab Stop (hts) - we use fixed 8-column tabs, ignore
            }
            _ => {
                let mut seq = String::from("\\e");
                for intermediate in intermediates {
                    seq.push(*intermediate as char);
                }
                seq.push(byte as char);

                let mut raw = vec![0x1b];
                raw.extend_from_slice(intermediates);
                raw.push(byte);

                self.debug_buffer.push(seq, &raw);
            }
        }
    }
}
