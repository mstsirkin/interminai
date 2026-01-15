#!/usr/bin/env python3
"""
🌀 an Interactive Terminal for AI (interminai)

Author: Michael S. Tsirkin <mst@kernel.org>

A PTY-based tool for interacting with terminal applications (Python version).
"""

import sys
import os
import pty
import select
import termios
import struct
import fcntl
import json
import socket
import signal
import time
import argparse
from pathlib import Path
import tempfile
import threading

# Try to import pyte for better terminal emulation
try:
    import pyte
    PYTE_AVAILABLE = True
except ImportError:
    PYTE_AVAILABLE = False

class DebugBuffer:
    """Ring buffer for unhandled escape sequences"""

    def __init__(self, capacity=10):
        self.capacity = capacity
        self.entries = []
        self.dropped = 0

    def record(self, sequence_bytes):
        """Record an unhandled escape sequence"""
        # Create human-readable sequence (e.g., \e[?25l)
        sequence = ''
        for b in sequence_bytes:
            if b == 0x1b:
                sequence += '\\e'
            elif 0x20 <= b < 0x7f:
                sequence += chr(b)
            else:
                sequence += f'\\x{b:02x}'

        # Create raw hex representation
        raw_hex = ' '.join(f'{b:02x}' for b in sequence_bytes)

        entry = {'sequence': sequence, 'raw_hex': raw_hex}

        if len(self.entries) >= self.capacity:
            self.entries.pop(0)
            self.dropped += 1

        self.entries.append(entry)

    def get_and_clear(self, clear):
        """Get entries and optionally clear the buffer"""
        result = {
            'unhandled': list(self.entries),
            'dropped': self.dropped
        }
        if clear:
            self.entries = []
            self.dropped = 0
        return result


class Screen:
    """Simple terminal emulator screen buffer"""

    def __init__(self, rows, cols):
        self.rows = rows
        self.cols = cols
        self.cells = [[' ' for _ in range(cols)] for _ in range(rows)]
        self.cursor_row = 0
        self.cursor_col = 0
        self.last_char = ' '
        self.debug_buffer = DebugBuffer()
        # Pending responses to be sent back to the PTY (e.g., for DSR cursor position query)
        self.pending_responses = []
        # Delayed wrap mode: when true, the next printable character will wrap to next line first
        self.pending_wrap = False
        # Activity flag: set to True when output is received, cleared by status or wait
        self.activity = False

    def scroll_up(self):
        """Scroll screen up by one line"""
        self.cells.pop(0)
        self.cells.append([' ' for _ in range(self.cols)])

    def move_cursor(self, row, col):
        """Move cursor to specified position, canceling pending wrap"""
        self.pending_wrap = False
        self.cursor_row = min(row, self.rows - 1)
        self.cursor_col = min(col, self.cols - 1)

    def move_cursor_row(self, row):
        """Move cursor to specified row, canceling pending wrap"""
        self.pending_wrap = False
        self.cursor_row = min(max(0, row), self.rows - 1)

    def move_cursor_col(self, col):
        """Move cursor to specified column, canceling pending wrap"""
        self.pending_wrap = False
        self.cursor_col = min(max(0, col), self.cols - 1)

    def print_char(self, c):
        """Print a character at cursor position"""
        self.last_char = c

        # Handle delayed wrap: if pending_wrap is set, wrap now before printing
        if self.pending_wrap:
            self.pending_wrap = False
            self.cursor_col = 0
            self.cursor_row += 1
            if self.cursor_row >= self.rows:
                self.scroll_up()
                self.cursor_row = self.rows - 1

        if self.cursor_row < self.rows and self.cursor_col < self.cols:
            self.cells[self.cursor_row][self.cursor_col] = c
            self.cursor_col += 1
            # If we've reached the right edge, set pending_wrap instead of wrapping immediately
            if self.cursor_col >= self.cols:
                self.cursor_col = self.cols - 1  # Keep cursor at last column
                self.pending_wrap = True

    def handle_control(self, byte):
        """Handle control characters"""
        # Control characters cancel pending wrap
        self.pending_wrap = False

        if byte == ord('\n'):
            self.cursor_row += 1
            if self.cursor_row >= self.rows:
                self.scroll_up()
                self.cursor_row = self.rows - 1
            self.cursor_col = 0
        elif byte == ord('\r'):
            self.cursor_col = 0
        elif byte == ord('\t'):
            self.cursor_col = ((self.cursor_col // 8) + 1) * 8
            if self.cursor_col >= self.cols:
                self.cursor_col = self.cols - 1
        elif byte == 0x08:  # backspace
            if self.cursor_col > 0:
                self.cursor_col -= 1

    def handle_csi(self, params, action, intermediates=None, raw_bytes=None):
        """Handle CSI escape sequences. Returns True if handled, False otherwise."""
        if action in ('H', 'f'):  # Cursor position
            row = (params[0] if len(params) > 0 else 1) - 1
            col = (params[1] if len(params) > 1 else 1) - 1
            self.move_cursor(row, col)
        elif action == 'A':  # Cursor up
            n = max(params[0] if params else 1, 1)
            self.move_cursor_row(self.cursor_row - n)
        elif action == 'B':  # Cursor down
            n = max(params[0] if params else 1, 1)
            self.move_cursor_row(self.cursor_row + n)
        elif action == 'C':  # Cursor forward
            n = max(params[0] if params else 1, 1)
            self.move_cursor_col(self.cursor_col + n)
        elif action == 'D':  # Cursor back
            n = max(params[0] if params else 1, 1)
            self.move_cursor_col(self.cursor_col - n)
        elif action == 'G':  # Cursor horizontal absolute (hpa)
            col = (params[0] if params else 1) - 1  # 1-based to 0-based
            self.move_cursor_col(col)
        elif action == 'd':  # Cursor vertical absolute (vpa)
            row = (params[0] if params else 1) - 1  # 1-based to 0-based
            self.move_cursor_row(row)
        elif action == 'J':  # Erase display
            mode = params[0] if params else 0
            if mode == 0:  # Clear from cursor to end
                for col in range(self.cursor_col, self.cols):
                    self.cells[self.cursor_row][col] = ' '
                for row in range(self.cursor_row + 1, self.rows):
                    for col in range(self.cols):
                        self.cells[row][col] = ' '
            elif mode == 2:  # Clear entire screen
                self.cells = [[' ' for _ in range(self.cols)] for _ in range(self.rows)]
                self.move_cursor(0, 0)
        elif action == 'K':  # Erase line
            mode = params[0] if params else 0
            if mode == 0:  # Clear from cursor to end of line
                for col in range(self.cursor_col, self.cols):
                    self.cells[self.cursor_row][col] = ' '
            elif mode == 1:  # Clear from beginning of line to cursor (el1)
                for col in range(self.cursor_col + 1):
                    self.cells[self.cursor_row][col] = ' '
            elif mode == 2:  # Clear entire line
                for col in range(self.cols):
                    self.cells[self.cursor_row][col] = ' '
        elif action == 'M':  # Delete Line (DL) - used by vim when deleting lines
            n = max(params[0] if params else 1, 1)
            for _ in range(n):
                if self.cursor_row < self.rows:
                    # Remove current line
                    self.cells.pop(self.cursor_row)
                    # Add blank line at bottom
                    self.cells.append([' ' for _ in range(self.cols)])
        elif action == 'L':  # Insert Line (IL) - used by vim when inserting lines
            n = max(params[0] if params else 1, 1)
            for _ in range(n):
                if self.cursor_row < self.rows:
                    # Remove bottom line
                    self.cells.pop()
                    # Insert blank line at cursor position
                    self.cells.insert(self.cursor_row, [' ' for _ in range(self.cols)])
        elif action == 'P':  # Delete Character (dch)
            n = max(params[0] if params else 1, 1)
            row = self.cursor_row
            for _ in range(n):
                if self.cursor_col < self.cols:
                    self.cells[row].pop(self.cursor_col)
                    self.cells[row].append(' ')
        elif action == '@':  # Insert Character (ich)
            n = max(params[0] if params else 1, 1)
            row = self.cursor_row
            for _ in range(n):
                if self.cursor_col < self.cols:
                    self.cells[row].pop()
                    self.cells[row].insert(self.cursor_col, ' ')
        elif action == 'X':  # Erase Character (ech)
            n = max(params[0] if params else 1, 1)
            for i in range(n):
                col = self.cursor_col + i
                if col < self.cols:
                    self.cells[self.cursor_row][col] = ' '
        elif action == 'S':  # Scroll Up (SU)
            n = max(params[0] if params else 1, 1)
            for _ in range(n):
                self.scroll_up()
        elif action == 'T':  # Scroll Down (SD)
            n = max(params[0] if params else 1, 1)
            for _ in range(n):
                self.cells.pop()
                self.cells.insert(0, [' ' for _ in range(self.cols)])
        elif action == 'I':  # Cursor Horizontal Tab (cht) - move forward to next tab stop N times
            n = max(params[0] if params else 1, 1)
            col = self.cursor_col
            for _ in range(n):
                col = ((col // 8) + 1) * 8
                if col >= self.cols:
                    col = self.cols - 1
                    break
            self.move_cursor_col(col)
        elif action == 'Z':  # Back Tab (cbt)
            if self.cursor_col > 0:
                self.move_cursor_col(((self.cursor_col - 1) // 8) * 8)
        elif action == 'b':  # Repeat (rep) - repeat last printed char N times
            n = max(params[0] if params else 1, 1)
            c = self.last_char
            for _ in range(n):
                self.print_char(c)
        elif action == 'm':  # SGR - intentionally ignored (colors/attributes)
            pass
        elif action == 'n':  # Device Status Report (DSR)
            mode = params[0] if params else 0
            if mode == 5:
                # Report device status: ESC [ 0 n (ready, no malfunction)
                self.pending_responses.append(b'\x1b[0n')
            elif mode == 6:
                # Report cursor position: ESC [ row ; col R (1-based)
                response = f'\x1b[{self.cursor_row + 1};{self.cursor_col + 1}R'
                self.pending_responses.append(response.encode('utf-8'))
        elif action == 'c':  # Primary Device Attributes (DA1)
            # Programs query terminal capabilities with ESC[c or ESC[0c
            # Respond as VT100 with AVO: ESC[?1;2c
            mode = params[0] if params else 0
            if mode == 0:
                self.pending_responses.append(b'\x1b[?1;2c')
        else:
            # Unhandled CSI sequence - record it
            if raw_bytes:
                self.debug_buffer.record(raw_bytes)

    def process_output(self, data):
        """Process output data with basic escape sequence parsing"""
        if data:
            self.activity = True
        i = 0
        while i < len(data):
            byte = data[i]

            # Handle escape sequences
            if byte == 0x1b and i + 1 < len(data):
                if data[i + 1] == ord('['):  # CSI
                    # Parse CSI sequence and track raw bytes
                    csi_start = i
                    i += 2
                    params = []
                    current_param = ''
                    intermediates = []

                    while i < len(data):
                        c = chr(data[i])
                        if c.isdigit():
                            current_param += c
                            i += 1
                        elif c == ';':
                            params.append(int(current_param) if current_param else 0)
                            current_param = ''
                            i += 1
                        elif c == '?':
                            # Private mode indicator - track as intermediate
                            intermediates.append(data[i])
                            i += 1
                        elif c.isalpha() or c in '@`':
                            # CSI final bytes are 0x40-0x7E (including @ and letters)
                            if current_param:
                                params.append(int(current_param))
                            # Capture raw bytes for debug logging
                            raw_bytes = bytes(data[csi_start:i+1])
                            self.handle_csi(params, c, intermediates, raw_bytes)
                            i += 1
                            break
                        else:
                            i += 1
                            break
                    continue
                else:
                    # Other escape sequences - record as unhandled
                    # Capture at least ESC + next byte
                    end = min(i + 2, len(data))
                    raw_bytes = bytes(data[i:end])
                    self.debug_buffer.record(raw_bytes)
                    i += 2
                    continue

            # Handle control characters
            if byte < 0x20:
                self.handle_control(byte)
                i += 1
            elif byte < 0x80:
                # ASCII printable
                self.print_char(chr(byte))
                i += 1
            else:
                # UTF-8 multi-byte sequence
                # Determine length from first byte
                if byte < 0xC0:
                    i += 1  # Invalid, skip
                elif byte < 0xE0:
                    # 2-byte sequence
                    if i + 1 < len(data):
                        try:
                            char = data[i:i+2].decode('utf-8')
                            self.print_char(char)
                        except:
                            pass
                    i += 2
                elif byte < 0xF0:
                    # 3-byte sequence
                    if i + 2 < len(data):
                        try:
                            char = data[i:i+3].decode('utf-8')
                            self.print_char(char)
                        except:
                            pass
                    i += 3
                elif byte < 0xF8:
                    # 4-byte sequence
                    if i + 3 < len(data):
                        try:
                            char = data[i:i+4].decode('utf-8')
                            self.print_char(char)
                        except:
                            pass
                    i += 4
                else:
                    i += 1  # Invalid, skip

    def render(self):
        """Render the screen as a string"""
        return '\n'.join(''.join(row) for row in self.cells)

    def render_ansi(self):
        """Render with ANSI codes - custom backend doesn't support colors, returns plain text"""
        return self.render()


class ExtendedPyteScreen(pyte.Screen):
    """Extended pyte Screen with additional CSI sequence methods"""

    def __init__(self, columns, lines):
        super().__init__(columns, lines)
        self._last_char = ' '
        self._pending_responses = []

    def write_process_input(self, data):
        """Capture responses (DSR, DA1, etc.) that need to be sent back to PTY"""
        self._pending_responses.append(data.encode('utf-8'))

    def report_device_status(self, mode=0, **kwargs):
        """Override to clamp cursor position for DSR 6 (cursor position report)"""
        if mode == 5:
            # Device status - just report OK
            self.write_process_input("\x1b[0n")
        elif mode == 6:
            # Cursor position - clamp to valid range
            row = min(self.cursor.y, self.lines - 1) + 1  # 1-based
            col = min(self.cursor.x, self.columns - 1) + 1  # 1-based
            self.write_process_input(f"\x1b[{row};{col}R")

    def draw(self, data):
        """Override to track last printed character for REP"""
        if data:
            self._last_char = data[-1]
        super().draw(data)

    def cursor_forward_tab(self, count=1):
        """CSI I - Cursor Horizontal Tab (CHT)"""
        for _ in range(count or 1):
            new_col = ((self.cursor.x // 8) + 1) * 8
            if new_col >= self.columns:
                new_col = self.columns - 1
            self.cursor.x = new_col

    def cursor_back_tab(self, count=1):
        """CSI Z - Cursor Backward Tab (CBT)"""
        for _ in range(count or 1):
            if self.cursor.x > 0:
                self.cursor.x = ((self.cursor.x - 1) // 8) * 8

    def scroll_up(self, count=1):
        """CSI S - Scroll Up (SU) - scroll content up, new blank lines at bottom"""
        from collections import defaultdict
        for _ in range(count or 1):
            # Shift all rows up by 1
            new_buffer = defaultdict(lambda: defaultdict(lambda: self.default_char))
            for row in range(self.lines - 1):
                new_buffer[row] = self.buffer[row + 1]
            new_buffer[self.lines - 1] = defaultdict(lambda: self.default_char)
            self.buffer = new_buffer

    def scroll_down(self, count=1):
        """CSI T - Scroll Down (SD) - scroll content down, new blank lines at top"""
        from collections import defaultdict
        for _ in range(count or 1):
            # Shift all rows down by 1
            new_buffer = defaultdict(lambda: defaultdict(lambda: self.default_char))
            for row in range(1, self.lines):
                new_buffer[row] = self.buffer[row - 1]
            new_buffer[0] = defaultdict(lambda: self.default_char)
            self.buffer = new_buffer

    def repeat_character(self, count=1):
        """CSI b - Repeat (REP) last printed character"""
        for _ in range(count or 1):
            super().draw(self._last_char)


class ExtendedPyteStream(pyte.Stream):
    """Extended pyte Stream with additional CSI handlers"""

    # Class-level CSI extensions (method name strings)
    csi = dict(pyte.Stream.csi, **{
        'I': 'cursor_forward_tab',
        'Z': 'cursor_back_tab',
        'S': 'scroll_up',
        'T': 'scroll_down',
        'b': 'repeat_character',
    })


class PyteScreen:
    """Terminal emulator using pyte library"""

    def __init__(self, rows, cols):
        self.rows = rows
        self.cols = cols
        self._screen = ExtendedPyteScreen(cols, rows)
        self._stream = ExtendedPyteStream(self._screen)
        # Pyte handles most sequences, so debug buffer will be mostly empty
        self.debug_buffer = DebugBuffer()
        self.pending_responses = []
        self.cursor_row = 0
        self.cursor_col = 0
        # Activity flag: set to True when output is received, cleared by status or wait
        self.activity = False

    def process_output(self, data):
        """Process output data through pyte"""
        if data:
            self.activity = True
        try:
            text = data.decode('utf-8', errors='replace')
            self._stream.feed(text)
        except Exception:
            pass
        # Update cursor position from pyte
        # Clamp to valid range (pyte may report cursor beyond edge for delayed wrap)
        self.cursor_row = min(self._screen.cursor.y, self.rows - 1)
        self.cursor_col = min(self._screen.cursor.x, self.cols - 1)
        # Collect any pending responses (DSR, DA1, etc.)
        self.pending_responses.extend(self._screen._pending_responses)
        self._screen._pending_responses.clear()

    def render(self):
        """Render the screen as a string"""
        return '\n'.join(self._screen.display)

    def render_ansi(self):
        """Render the screen with ANSI color codes"""
        lines = []
        for y in range(self.rows):
            line = ''
            current_fg = 'default'
            current_bg = 'default'
            current_attrs = (False, False, False, False, False, False)  # bold, italics, underscore, strikethrough, reverse, blink

            for x in range(self.cols):
                char = self._screen.buffer[y][x]
                attrs = (char.bold, char.italics, char.underscore,
                         char.strikethrough, char.reverse, char.blink)

                # Check if we need to emit SGR codes
                if char.fg != current_fg or char.bg != current_bg or attrs != current_attrs:
                    sgr = self._build_sgr(char.fg, char.bg, attrs)
                    if sgr:
                        line += sgr
                    current_fg = char.fg
                    current_bg = char.bg
                    current_attrs = attrs

                line += char.data

            # Reset at end of line if we changed attributes
            if current_fg != 'default' or current_bg != 'default' or any(current_attrs):
                line += '\x1b[0m'

            lines.append(line.rstrip())

        return '\n'.join(lines)

    def _build_sgr(self, fg, bg, attrs):
        """Build ANSI SGR sequence from pyte color and attributes"""
        codes = ['0']  # Start with reset

        # Attributes
        bold, italics, underscore, strikethrough, reverse, blink = attrs
        if bold:
            codes.append('1')
        if italics:
            codes.append('3')
        if underscore:
            codes.append('4')
        if blink:
            codes.append('5')
        if reverse:
            codes.append('7')
        if strikethrough:
            codes.append('9')

        # Foreground color
        fg_code = self._color_to_ansi(fg, is_foreground=True)
        if fg_code:
            codes.append(fg_code)

        # Background color
        bg_code = self._color_to_ansi(bg, is_foreground=False)
        if bg_code:
            codes.append(bg_code)

        if len(codes) == 1 and codes[0] == '0':
            return ''  # No attributes, skip

        return f'\x1b[{";".join(codes)}m'

    def _color_to_ansi(self, color, is_foreground):
        """Convert pyte color to ANSI code"""
        if color == 'default':
            return None

        # Named colors
        named_colors = {
            'black': (30, 40),
            'red': (31, 41),
            'green': (32, 42),
            'yellow': (33, 43),
            'blue': (34, 44),
            'magenta': (35, 45),
            'cyan': (36, 46),
            'white': (37, 47),
            'brightblack': (90, 100),
            'brightred': (91, 101),
            'brightgreen': (92, 102),
            'brightyellow': (93, 103),
            'brightblue': (94, 104),
            'brightmagenta': (95, 105),
            'brightcyan': (96, 106),
            'brightwhite': (97, 107),
        }

        color_lower = color.lower()
        if color_lower in named_colors:
            fg_code, bg_code = named_colors[color_lower]
            return str(fg_code if is_foreground else bg_code)

        # Check for hex color (6 or 8 chars without #)
        if len(color) == 6 or len(color) == 8:
            try:
                r = int(color[0:2], 16)
                g = int(color[2:4], 16)
                b = int(color[4:6], 16)
                prefix = '38;2' if is_foreground else '48;2'
                return f'{prefix};{r};{g};{b}'
            except ValueError:
                pass

        # Check for indexed color (number as string)
        try:
            idx = int(color)
            if 0 <= idx <= 255:
                prefix = '38;5' if is_foreground else '48;5'
                return f'{prefix};{idx}'
        except ValueError:
            pass

        return None


class DaemonState:
    """State for the daemon process"""

    def __init__(self, master_fd, child_pid, socket_path, rows, cols, socket_was_auto_generated, emulator='xterm', pty_dump=None):
        self.master_fd = master_fd
        self.child_pid = child_pid
        self.socket_path = socket_path
        # Select terminal emulator
        if emulator == 'xterm' and PYTE_AVAILABLE:
            self.screen = PyteScreen(rows, cols)
        else:
            self.screen = Screen(rows, cols)
        self.exit_code = None
        self.should_shutdown = False
        self.socket_was_auto_generated = socket_was_auto_generated
        self.pty_dump = pty_dump

    def check_child_status(self):
        """Check if child process has exited"""
        if self.exit_code is not None:
            return

        try:
            pid, status = os.waitpid(self.child_pid, os.WNOHANG)
            if pid == self.child_pid:
                if os.WIFEXITED(status):
                    self.exit_code = os.WEXITSTATUS(status)
                elif os.WIFSIGNALED(status):
                    self.exit_code = 128 + os.WTERMSIG(status)
        except ChildProcessError:
            pass

    def read_pty_output(self):
        """Read output from PTY and update screen"""
        try:
            data = os.read(self.master_fd, 4096)
            if data:
                # Dump raw bytes if pty_dump is enabled
                if self.pty_dump:
                    self.pty_dump.write(data)
                    self.pty_dump.flush()
                self.screen.process_output(data)
        except (OSError, IOError):
            pass

        # Send any pending responses back to the PTY (e.g., cursor position reports)
        for response in self.screen.pending_responses:
            try:
                os.write(self.master_fd, response)
            except (OSError, IOError):
                pass
        self.screen.pending_responses.clear()


def parse_terminal_size(size_str):
    """Parse terminal size string like '80x24'"""
    try:
        parts = size_str.split('x')
        if len(parts) != 2:
            raise ValueError(f"Invalid size format: {size_str}")
        cols = int(parts[0])
        rows = int(parts[1])
        if cols <= 0 or rows <= 0:
            raise ValueError(f"Invalid size: {size_str}")
        return cols, rows
    except ValueError as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)


def auto_generate_socket_path():
    """Generate an auto socket path"""
    temp_dir = tempfile.mkdtemp(prefix="interminai-")
    return os.path.join(temp_dir, "sock")


def set_window_size(fd, rows, cols):
    """Set the window size for the PTY"""
    winsize = struct.pack('HHHH', rows, cols, 0, 0)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, winsize)


def cmd_start(args):
    """Start command - launch a program in a PTY"""
    socket_was_auto_generated = args.socket is None
    socket_path = args.socket if args.socket else auto_generate_socket_path()

    cols, rows = parse_terminal_size(args.size)

    if args.no_daemon:
        # Run in foreground
        print(f"Socket: {socket_path}")
        print(f"PID: {os.getpid()}")
        print(f"Auto-generated: {socket_was_auto_generated}")
        sys.stdout.flush()
        run_daemon(socket_path, cols, rows, args.command, socket_was_auto_generated, args.emulator, args.pty_dump)
    else:
        # Daemonize (double fork)
        pid = os.fork()
        if pid > 0:
            # Parent - wait for intermediate child
            os.waitpid(pid, 0)
            print(f"Socket: {socket_path}")
            print(f"Auto-generated: {socket_was_auto_generated}")
            sys.stdout.flush()
            return

        # Intermediate child
        os.setsid()

        # Second fork
        pid = os.fork()
        if pid > 0:
            # Intermediate child prints PID and exits
            print(f"PID: {pid}")
            sys.stdout.flush()
            sys.exit(0)

        # Grandchild (daemon)
        # Close standard file descriptors properly
        os.close(0)
        os.close(1)
        os.close(2)

        # Open /dev/null for stdin/stdout/stderr
        devnull = os.open('/dev/null', os.O_RDWR)
        os.dup2(devnull, 0)
        os.dup2(devnull, 1)
        os.dup2(devnull, 2)
        if devnull > 2:
            os.close(devnull)

        # Run daemon
        run_daemon(socket_path, cols, rows, args.command, socket_was_auto_generated, args.emulator, args.pty_dump)


def run_daemon(socket_path, cols, rows, command, socket_was_auto_generated, emulator='xterm', pty_dump_path=None):
    """Run the daemon process"""
    # Ignore SIGPIPE in daemon - we handle socket errors via exceptions
    # (main() sets SIGPIPE to SIG_DFL for client commands that pipe to head/less)
    signal.signal(signal.SIGPIPE, signal.SIG_IGN)

    # Create PTY
    master_fd, slave_fd = pty.openpty()

    # Set window size
    set_window_size(slave_fd, rows, cols)

    # Fork to create child process
    child_pid = os.fork()

    if child_pid == 0:
        # Child process
        os.close(master_fd)

        # Create new session
        os.setsid()

        # Set controlling terminal
        fcntl.ioctl(slave_fd, termios.TIOCSCTTY, 0)

        # Redirect stdio
        os.dup2(slave_fd, 0)
        os.dup2(slave_fd, 1)
        os.dup2(slave_fd, 2)

        if slave_fd > 2:
            os.close(slave_fd)

        # Set TERM based on terminal emulator
        # xterm (pyte) supports full xterm-256color capabilities
        # custom uses basic ANSI escape sequences
        if emulator == 'xterm' and PYTE_AVAILABLE:
            os.environ['TERM'] = 'xterm-256color'
        else:
            os.environ['TERM'] = 'ansi'

        # Execute command
        os.execvp(command[0], command)
        sys.exit(1)

    # Parent (daemon) process
    os.close(slave_fd)

    # Open PTY dump file if specified
    pty_dump_file = None
    if pty_dump_path:
        pty_dump_file = open(pty_dump_path, 'ab')

    # Create state
    state = DaemonState(master_fd, child_pid, socket_path, rows, cols, socket_was_auto_generated, emulator, pty_dump_file)

    # Create Unix socket
    if os.path.exists(socket_path):
        os.unlink(socket_path)

    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.bind(socket_path)
    sock.listen(5)
    sock.setblocking(False)

    # Start PTY reader thread - use poll() for efficient event-driven I/O
    def pty_reader():
        poller = select.poll()
        poller.register(state.master_fd, select.POLLIN | select.POLLHUP | select.POLLERR)
        pty_closed = False
        while not state.should_shutdown:
            if pty_closed:
                # PTY closed but child may still be running - poll child status only
                state.check_child_status()
                if state.exit_code is not None:
                    break
                time.sleep(0.1)
                continue
            # Wait for PTY events
            events = poller.poll()
            for fd, event in events:
                if event & select.POLLIN:
                    state.read_pty_output()
                if event & (select.POLLHUP | select.POLLERR):
                    # PTY closed - drain remaining output, but child may still be running
                    state.read_pty_output()
                    pty_closed = True
            state.check_child_status()
            if state.exit_code is not None:
                break

    reader_thread = threading.Thread(target=pty_reader, daemon=True)
    reader_thread.start()

    # Main daemon loop - accept connections
    try:
        while not state.should_shutdown:
            # Accept connections
            try:
                client_sock, _ = sock.accept()
                # Process commands sequentially - no parallelism
                handle_client(client_sock, state)
            except BlockingIOError:
                pass

            time.sleep(0.05)
    finally:
        # Cleanup
        os.close(master_fd)
        sock.close()

        if socket_was_auto_generated:
            try:
                os.unlink(socket_path)
                os.rmdir(os.path.dirname(socket_path))
            except:
                pass


def handle_client(client_sock, state):
    """Handle a client connection"""
    try:
        # Read request (blocking, no timeout)
        data = b''
        while True:
            chunk = client_sock.recv(4096)
            if not chunk:
                break
            data += chunk
            if b'\n' in data:
                break

        if not data:
            return

        request = json.loads(data.decode('utf-8').strip())
        req_type = request.get('type')

        # Handle request
        if req_type == 'OUTPUT':
            response = handle_output(request.get('format', 'ascii'), state)
        elif req_type == 'INPUT':
            response = handle_input(request.get('data'), state)
        elif req_type == 'STATUS':
            response = handle_running(request.get('activity', False), state)
        elif req_type == 'STOP':
            response = handle_stop(state)
        elif req_type == 'WAIT':
            response = handle_wait(request.get('data'), state, client_sock)
        elif req_type == 'KILL':
            response = handle_kill(request.get('data'), state)
        elif req_type == 'RESIZE':
            response = handle_resize(request.get('data'), state)
        elif req_type == 'DEBUG':
            response = handle_debug(request.get('data'), state)
        else:
            response = {'status': 'error', 'error': f'Unknown command: {req_type}'}

        # Send response
        response_json = json.dumps(response) + '\n'
        client_sock.sendall(response_json.encode('utf-8'))

    except Exception as e:
        error_response = json.dumps({'status': 'error', 'error': str(e)}) + '\n'
        try:
            client_sock.sendall(error_response.encode('utf-8'))
        except:
            pass
    finally:
        try:
            client_sock.close()
        except:
            pass


def handle_output(fmt, state):
    """Handle OUTPUT request"""
    if fmt == 'ansi':
        screen_text = state.screen.render_ansi()
    else:
        screen_text = state.screen.render()

    return {
        'status': 'ok',
        'data': {
            'screen': screen_text,
            'cursor': {
                'row': state.screen.cursor_row,
                'col': state.screen.cursor_col
            },
            'size': {
                'rows': state.screen.rows,
                'cols': state.screen.cols
            }
        }
    }


def handle_input(data, state):
    """Handle INPUT request"""
    if not data or 'data' not in data:
        return {'status': 'error', 'error': 'Missing data field'}

    input_str = data['data']
    try:
        os.write(state.master_fd, input_str.encode('utf-8'))
        return {'status': 'ok', 'data': {'message': 'Input sent'}}
    except Exception as e:
        return {'status': 'error', 'error': str(e)}


def handle_running(activity_mode, state):
    """Handle STATUS request"""
    running = state.exit_code is None

    if activity_mode:
        activity = state.activity
        state.activity = False  # Clear the flag after reading
        data = {
            'running': running,
            'activity': activity
        }
        if state.exit_code is not None:
            data['exit_code'] = state.exit_code
        return {'status': 'ok', 'data': data}
    else:
        return {
            'status': 'ok',
            'data': {
                'running': running,
                'exit_code': state.exit_code
            }
        }


def handle_stop(state):
    """Handle STOP request"""
    if state.exit_code is None:
        try:
            os.kill(state.child_pid, signal.SIGTERM)
        except:
            pass

    state.should_shutdown = True
    return {'status': 'ok', 'data': {'message': 'Shutting down'}}


def handle_wait(data, state, client_sock):
    """Handle WAIT request"""
    activity_mode = data and data.get('activity', False)

    # In activity mode, wait for any activity (output or exit)
    # Otherwise, wait for child to exit
    while True:
        # Check if client disconnected using select
        readable, _, exceptional = select.select([client_sock], [], [client_sock], 0)
        if readable or exceptional:
            # Client sent data or disconnected - check with recv
            try:
                recv_data = client_sock.recv(1, socket.MSG_PEEK | socket.MSG_DONTWAIT)
                if not recv_data:
                    # EOF - client disconnected
                    return {'status': 'error', 'error': 'Client disconnected'}
            except (BlockingIOError, OSError):
                # Error or would block - assume disconnected
                return {'status': 'error', 'error': 'Client disconnected'}

        state.check_child_status()

        if activity_mode:
            # Activity mode: return as soon as activity or exit is detected
            # Get separate flags for PTY activity vs process exit
            pty_activity = state.screen.activity
            exited = state.exit_code is not None
            if pty_activity or exited:
                # Clear the PTY activity flag
                state.screen.activity = False
                return {
                    'status': 'ok',
                    'data': {
                        'activity': pty_activity,
                        'exited': exited
                    }
                }
        else:
            # Normal mode: wait for exit
            if state.exit_code is not None:
                return {
                    'status': 'ok',
                    'data': {
                        'exit_code': state.exit_code
                    }
                }

        time.sleep(0.1)


def handle_kill(data, state):
    """Handle KILL request"""
    if not data or 'signal' not in data:
        sig = signal.SIGTERM
    else:
        sig_str = data['signal']
        try:
            if sig_str.isdigit():
                sig = int(sig_str)
                # Validate signal number range
                if sig < 1 or sig > 64:
                    return {'status': 'error', 'error': f'Invalid signal: {sig_str}'}
            else:
                sig_name = sig_str if sig_str.startswith('SIG') else f'SIG{sig_str}'
                try:
                    sig = getattr(signal, sig_name.upper())
                except AttributeError:
                    return {'status': 'error', 'error': f'Invalid signal: {sig_str}'}
        except (ValueError, TypeError):
            return {'status': 'error', 'error': f'Invalid signal: {sig_str}'}

    try:
        os.kill(state.child_pid, sig)
        return {'status': 'ok', 'data': {'message': f'Signal {sig} sent'}}
    except Exception as e:
        return {'status': 'error', 'error': str(e)}


def handle_resize(data, state):
    """Handle RESIZE request"""
    if not data or 'cols' not in data or 'rows' not in data:
        return {'status': 'error', 'error': 'Missing cols or rows field'}

    cols = data['cols']
    rows = data['rows']

    try:
        set_window_size(state.master_fd, rows, cols)

        # Resize screen buffer
        state.screen = Screen(rows, cols)

        return {'status': 'ok', 'data': {'message': f'Resized to {cols}x{rows}'}}
    except Exception as e:
        return {'status': 'error', 'error': str(e)}


def handle_debug(data, state):
    """Handle DEBUG request"""
    clear = False
    if data and data.get('clear'):
        clear = True

    result = state.screen.debug_buffer.get_and_clear(clear)
    return {'status': 'ok', 'data': result}


def send_request(socket_path, request):
    """Send a request to the daemon"""
    sock = None
    try:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.connect(socket_path)

        request_json = json.dumps(request) + '\n'
        sock.sendall(request_json.encode('utf-8'))

        # Read response (blocking, no timeout)
        data = b''
        while True:
            chunk = sock.recv(4096)
            if not chunk:
                break
            data += chunk
            if b'\n' in data:
                break

        if not data:
            return {'status': 'error', 'error': 'No response'}

        return json.loads(data.decode('utf-8').strip())

    except FileNotFoundError as e:
        # Print OS error message to match Rust behavior (e.g., "No such file or directory")
        print(f"Error: {e.strerror}: {socket_path}", file=sys.stderr)
        sys.stderr.flush()
        sys.exit(1)
    except ConnectionRefusedError:
        print(f"Error: Connection refused: {socket_path}", file=sys.stderr)
        sys.stderr.flush()
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.stderr.flush()
        sys.exit(1)
    finally:
        if sock:
            try:
                sock.close()
            except:
                pass


def apply_cursor_inverse(screen, cursor_row, cursor_col):
    """Apply inverse video to character at cursor position"""
    lines = screen.split('\n')
    
    # Check if cursor_row is valid
    if cursor_row >= len(lines):
        return screen
    
    result = []
    for row_idx, line in enumerate(lines):
        if row_idx == cursor_row:
            # Check if cursor_col is valid
            if cursor_col >= len(line):
                result.append(line)
            else:
                # Build line with inverse video at cursor position
                new_line = ''
                for col_idx, ch in enumerate(line):
                    if col_idx == cursor_col:
                        new_line += f'\x1b[7m{ch}\x1b[27m'  # Inverse video
                    else:
                        new_line += ch
                result.append(new_line)
        else:
            result.append(line)
    
    return '\n'.join(result)


def unescape(s):
    """Unescape C-style escape sequences in a string.
    
    Supports: \\n \\r \\t \\a \\b \\f \\v \\\\ \\e \\xHH
    """
    result = []
    i = 0
    while i < len(s):
        if s[i] == '\\' and i + 1 < len(s):
            c = s[i + 1]
            if c == 'n':
                result.append('\n')
                i += 2
            elif c == 'r':
                result.append('\r')
                i += 2
            elif c == 't':
                result.append('\t')
                i += 2
            elif c == 'a':
                result.append('\x07')  # bell
                i += 2
            elif c == 'b':
                result.append('\x08')  # backspace
                i += 2
            elif c == 'f':
                result.append('\x0c')  # form feed (Ctrl+L)
                i += 2
            elif c == 'v':
                result.append('\x0b')  # vertical tab
                i += 2
            elif c == '\\':
                result.append('\\')
                i += 2
            elif c in ('e', 'E'):
                result.append('\x1b')  # ESC
                i += 2
            elif c == 'x' and i + 3 < len(s):
                try:
                    byte = int(s[i+2:i+4], 16)
                    result.append(chr(byte))
                    i += 4
                except ValueError:
                    result.append(s[i])
                    i += 1
            else:
                # Unknown escape - keep as-is
                result.append(s[i])
                i += 1
        else:
            result.append(s[i])
            i += 1
    return ''.join(result)


def cmd_output(args):
    """Output command - get screen content"""
    # Default is color (ansi), --no-color disables it
    fmt = 'ascii' if args.no_color else 'ansi'
    request = {'type': 'OUTPUT', 'format': fmt}
    response = send_request(args.socket, request)

    if response['status'] == 'error':
        print(f"Error: {response.get('error', 'Unknown error')}", file=sys.stderr)
        sys.exit(1)

    data = response['data']
    cursor_mode = args.cursor
    
    # Print cursor info if requested (convert to 1-based for display)
    if cursor_mode in ('print', 'both'):
        cursor = data.get('cursor', {})
        cursor_row = cursor.get('row', 0)
        cursor_col = cursor.get('col', 0)
        print(f"Cursor: row {cursor_row + 1}, col {cursor_col + 1}")
    
    screen = data['screen']
    
    # Apply inverse video if requested
    if cursor_mode in ('inverse', 'both'):
        cursor = data.get('cursor', {})
        cursor_row = cursor.get('row', 0)
        cursor_col = cursor.get('col', 0)
        screen = apply_cursor_inverse(screen, cursor_row, cursor_col)
    
    print(screen)


def cmd_input(args):
    """Input command - send input to the program"""
    # Priority: --password, --text, stdin
    if args.password:
        import getpass

        # Fetch current screen to show the password prompt from the application
        output_request = {'type': 'OUTPUT', 'format': 'ascii'}
        output_response = send_request(args.socket, output_request)

        # Show generic guidance, then the cursor line and previous line for context
        print("Type your secret or password and press Enter.", file=sys.stderr)
        if output_response.get('status') == 'ok':
            data = output_response.get('data', {})
            screen = data.get('screen', '')
            cursor_row = data.get('cursor', {}).get('row', 0)
            lines = screen.split('\n')
            # Show previous line if it exists and is non-empty
            if cursor_row > 0 and cursor_row - 1 < len(lines) and lines[cursor_row - 1].strip():
                print(lines[cursor_row - 1], file=sys.stderr)
            # Show cursor line
            if cursor_row < len(lines) and lines[cursor_row].strip():
                print(f"{lines[cursor_row]} ", end='', file=sys.stderr)
                sys.stderr.flush()

        try:
            password = getpass.getpass(prompt='')
        except Exception as e:
            print(f"Error: Failed to read password (is stdin a terminal?): {e}", file=sys.stderr)
            sys.exit(1)
        data = password + '\r'  # Append Enter
    elif args.text is not None:
        data = unescape(args.text)
    else:
        data = sys.stdin.read()

    request = {'type': 'INPUT', 'data': {'data': data}}
    response = send_request(args.socket, request)

    if response['status'] == 'error':
        print(f"Error: {response.get('error', 'Unknown error')}", file=sys.stderr)
        sys.exit(1)


def cmd_status(args):
    """Status command - get session status"""
    request = {'type': 'STATUS', 'activity': not args.quiet}
    response = send_request(args.socket, request)

    if response['status'] == 'error':
        print(f"Error: {response.get('error', 'Unknown error')}", file=sys.stderr)
        sys.exit(1)

    data = response['data']
    running = data.get('running', False)

    if args.quiet:
        # Quiet mode: just exit status
        if not running:
            exit_code = data.get('exit_code')
            if exit_code is not None:
                print(exit_code)
            sys.exit(1)
    else:
        # Default mode: print all status info
        print(f"Running: {str(running).lower()}")
        activity = data.get('activity', False)
        print(f"Activity: {str(activity).lower()}")
        if not running:
            exit_code = data.get('exit_code')
            if exit_code is not None:
                print(f"Exit code: {exit_code}")


def cmd_stop(args):
    """Stop command - stop the daemon"""
    request = {'type': 'STOP'}
    response = send_request(args.socket, request)

    if response['status'] == 'error':
        print(f"Error: {response.get('error', 'Unknown error')}", file=sys.stderr)
        sys.exit(1)


def cmd_wait(args):
    """Wait command - wait for program to exit or activity"""
    request = {'type': 'WAIT', 'data': {'activity': not args.quiet}}
    response = send_request(args.socket, request)

    if response['status'] == 'error':
        print(f"Error: {response.get('error', 'Unknown error')}", file=sys.stderr)
        sys.exit(1)

    data = response['data']
    if args.quiet:
        # Quiet mode: just print exit code
        if data.get('exit_code') is not None:
            print(data['exit_code'])
            sys.exit(0)
    else:
        # Default mode: report both terminal activity and exit status
        has_activity = data.get('activity', False)
        has_exited = data.get('exited', False)
        print(f"Terminal activity: {'true' if has_activity else 'false'}")
        print(f"Application exited: {'true' if has_exited else 'false'}")


def cmd_kill(args):
    """Kill command - send signal to child process"""
    request = {'type': 'KILL', 'data': {'signal': args.signal}}
    response = send_request(args.socket, request)

    if response['status'] == 'error':
        print(f"Error: {response.get('error', 'Unknown error')}", file=sys.stderr)
        sys.stderr.flush()
        sys.exit(1)


def cmd_resize(args):
    """Resize command - resize the terminal"""
    cols, rows = parse_terminal_size(args.size)
    request = {'type': 'RESIZE', 'data': {'cols': cols, 'rows': rows}}
    response = send_request(args.socket, request)

    if response['status'] == 'error':
        print(f"Error: {response.get('error', 'Unknown error')}", file=sys.stderr)
        sys.exit(1)


def cmd_debug(args):
    """Debug command - show unhandled escape sequences"""
    request = {'type': 'DEBUG', 'data': {'clear': args.clear}}
    response = send_request(args.socket, request)

    if response['status'] == 'error':
        print(f"Error: {response.get('error', 'Unknown error')}", file=sys.stderr)
        sys.exit(1)

    data = response['data']
    unhandled = data.get('unhandled', [])
    dropped = data.get('dropped', 0)

    if not unhandled:
        print("No unhandled escape sequences")
    else:
        print(f"Unhandled escape sequences ({len(unhandled)} entries):")
        for entry in unhandled:
            print(f"  {entry['sequence']} ({entry['raw_hex']})")

    if dropped > 0:
        print(f"Dropped: {dropped}")


def main():
    # Handle SIGPIPE properly - when piped to head/less/etc, exit silently
    # instead of raising BrokenPipeError
    signal.signal(signal.SIGPIPE, signal.SIG_DFL)

    # Ensure stdout is unbuffered for immediate output
    sys.stdout.reconfigure(line_buffering=True)

    parser = argparse.ArgumentParser(description='🌀 an Interactive Terminal for AI')
    subparsers = parser.add_subparsers(dest='command', required=True)

    # Start command
    start_parser = subparsers.add_parser('start', help='Start a program')
    start_parser.add_argument('--socket', help='Socket path')
    start_parser.add_argument('--size', default='80x24', help='Terminal size (COLSxROWS)')
    # Emulator choices: xterm (if pyte available) or custom
    if PYTE_AVAILABLE:
        emulator_choices = ['xterm', 'custom']
        emulator_default = 'xterm'
    else:
        emulator_choices = ['custom']
        emulator_default = 'custom'
    start_parser.add_argument('--emulator', choices=emulator_choices, default=emulator_default,
                              help='Terminal emulator backend')
    start_parser.add_argument('--no-daemon', action='store_true', help='Run in foreground')
    start_parser.add_argument('--pty-dump', help='Dump raw PTY output to this file (for debugging)')
    start_parser.add_argument('command', nargs='+', help='Command to run')
    start_parser.set_defaults(func=cmd_start)

    # Output command
    output_parser = subparsers.add_parser('output', help='Get screen output')
    output_parser.add_argument('--socket', required=True, help='Socket path')
    output_parser.add_argument('--color', action='store_true', help='Enable color output (default)')
    output_parser.add_argument('--no-color', action='store_true', dest='no_color',
                               help='Disable color output (for grep/head)')
    output_parser.add_argument('--cursor', default='none', choices=['none', 'print', 'inverse', 'both'],
                               help='Cursor display mode (default: none)')
    output_parser.set_defaults(func=cmd_output)

    # Input command
    input_parser = subparsers.add_parser('input', help='Send input')
    input_parser.add_argument('--socket', required=True, help='Socket path')
    input_parser.add_argument('--text', help='Input text with escape sequences (alternative to stdin)')
    input_parser.add_argument('--password', action='store_true',
                              help='Read password from terminal with echo disabled')
    input_parser.set_defaults(func=cmd_input)

    # Status command
    status_parser = subparsers.add_parser('status', help='Check process status')
    status_parser.add_argument('--socket', required=True, help='Socket path')
    status_parser.add_argument('--quiet', action='store_true', help='Just exit status (0 if running, 1 if exited)')
    status_parser.set_defaults(func=cmd_status)

    # Stop command
    stop_parser = subparsers.add_parser('stop', help='Stop the daemon')
    stop_parser.add_argument('--socket', required=True, help='Socket path')
    stop_parser.set_defaults(func=cmd_stop)

    # Wait command
    wait_parser = subparsers.add_parser('wait', help='Wait for exit or activity')
    wait_parser.add_argument('--socket', required=True, help='Socket path')
    wait_parser.add_argument('--quiet', action='store_true',
                             help='Wait for exit only, print exit code')
    wait_parser.set_defaults(func=cmd_wait)

    # Kill command
    kill_parser = subparsers.add_parser('kill', help='Send signal')
    kill_parser.add_argument('--socket', required=True, help='Socket path')
    kill_parser.add_argument('--signal', default='TERM', help='Signal to send')
    kill_parser.set_defaults(func=cmd_kill)

    # Resize command
    resize_parser = subparsers.add_parser('resize', help='Resize terminal')
    resize_parser.add_argument('--socket', required=True, help='Socket path')
    resize_parser.add_argument('--size', required=True, help='New size (COLSxROWS)')
    resize_parser.set_defaults(func=cmd_resize)

    # Debug command
    debug_parser = subparsers.add_parser('debug', help='Show unhandled escape sequences')
    debug_parser.add_argument('--socket', required=True, help='Socket path')
    debug_parser.add_argument('--clear', action='store_true', help='Clear buffer after reading')
    debug_parser.set_defaults(func=cmd_debug)

    args = parser.parse_args()

    # Handle special parsing for start command with --
    if args.command == 'start':
        # Find -- separator
        argv = sys.argv[1:]
        if '--' in argv:
            idx = argv.index('--')
            args.command = argv[idx + 1:]

    args.func(args)


if __name__ == '__main__':
    main()
