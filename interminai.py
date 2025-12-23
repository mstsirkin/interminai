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

    def scroll_up(self):
        """Scroll screen up by one line"""
        self.cells.pop(0)
        self.cells.append([' ' for _ in range(self.cols)])

    def print_char(self, c):
        """Print a character at cursor position"""
        self.last_char = c
        if self.cursor_row < self.rows and self.cursor_col < self.cols:
            self.cells[self.cursor_row][self.cursor_col] = c
            self.cursor_col += 1
            if self.cursor_col >= self.cols:
                self.cursor_col = 0
                self.cursor_row += 1
                if self.cursor_row >= self.rows:
                    self.scroll_up()
                    self.cursor_row = self.rows - 1

    def handle_control(self, byte):
        """Handle control characters"""
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
            self.cursor_row = min(row, self.rows - 1)
            self.cursor_col = min(col, self.cols - 1)
        elif action == 'A':  # Cursor up
            n = max(params[0] if params else 1, 1)
            self.cursor_row = max(0, self.cursor_row - n)
        elif action == 'B':  # Cursor down
            n = max(params[0] if params else 1, 1)
            self.cursor_row = min(self.rows - 1, self.cursor_row + n)
        elif action == 'C':  # Cursor forward
            n = max(params[0] if params else 1, 1)
            self.cursor_col = min(self.cols - 1, self.cursor_col + n)
        elif action == 'D':  # Cursor back
            n = max(params[0] if params else 1, 1)
            self.cursor_col = max(0, self.cursor_col - n)
        elif action == 'G':  # Cursor horizontal absolute (hpa)
            col = (params[0] if params else 1) - 1  # 1-based to 0-based
            self.cursor_col = min(self.cols - 1, max(0, col))
        elif action == 'd':  # Cursor vertical absolute (vpa)
            row = (params[0] if params else 1) - 1  # 1-based to 0-based
            self.cursor_row = min(self.rows - 1, max(0, row))
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
                self.cursor_row = 0
                self.cursor_col = 0
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
        elif action == 'Z':  # Back Tab (cbt)
            if self.cursor_col > 0:
                self.cursor_col = ((self.cursor_col - 1) // 8) * 8
        elif action == 'b':  # Repeat (rep) - repeat last printed char N times
            n = max(params[0] if params else 1, 1)
            c = self.last_char
            for _ in range(n):
                self.print_char(c)
        elif action == 'm':  # SGR - intentionally ignored (colors/attributes)
            pass
        else:
            # Unhandled CSI sequence - record it
            if raw_bytes:
                self.debug_buffer.record(raw_bytes)

    def process_output(self, data):
        """Process output data with basic escape sequence parsing"""
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


class DaemonState:
    """State for the daemon process"""

    def __init__(self, master_fd, child_pid, socket_path, rows, cols, socket_was_auto_generated):
        self.master_fd = master_fd
        self.child_pid = child_pid
        self.socket_path = socket_path
        self.screen = Screen(rows, cols)
        self.exit_code = None
        self.should_shutdown = False
        self.socket_was_auto_generated = socket_was_auto_generated

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
                self.screen.process_output(data)
        except (OSError, IOError):
            pass


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
        run_daemon(socket_path, cols, rows, args.command, socket_was_auto_generated)
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
        run_daemon(socket_path, cols, rows, args.command, socket_was_auto_generated)


def run_daemon(socket_path, cols, rows, command, socket_was_auto_generated):
    """Run the daemon process"""
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

        # Set TERM=ansi to force apps to use basic escape sequences that our
        # terminal emulator can handle. The "ansi" terminfo doesn't advertise
        # scroll regions (csr) which we don't support, but does have insert/delete
        # line (il1/dl1) which we do support.
        os.environ['TERM'] = 'ansi'

        # Execute command
        os.execvp(command[0], command)
        sys.exit(1)

    # Parent (daemon) process
    os.close(slave_fd)

    # Create state
    state = DaemonState(master_fd, child_pid, socket_path, rows, cols, socket_was_auto_generated)

    # Create Unix socket
    if os.path.exists(socket_path):
        os.unlink(socket_path)

    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.bind(socket_path)
    sock.listen(5)
    sock.setblocking(False)

    # Start PTY reader thread
    def pty_reader():
        while not state.should_shutdown:
            state.check_child_status()
            state.read_pty_output()
            if state.exit_code is not None:
                break
            time.sleep(0.05)

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
            response = handle_output(state)
        elif req_type == 'INPUT':
            response = handle_input(request.get('data'), state)
        elif req_type == 'RUNNING':
            response = handle_running(state)
        elif req_type == 'STOP':
            response = handle_stop(state)
        elif req_type == 'WAIT':
            response = handle_wait(state, client_sock)
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


def handle_output(state):
    """Handle OUTPUT request"""
    return {
        'status': 'ok',
        'data': {
            'screen': state.screen.render(),
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


def handle_running(state):
    """Handle RUNNING request"""
    running = state.exit_code is None
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


def handle_wait(state, client_sock):
    """Handle WAIT request"""
    # Wait for child to exit, checking for client disconnect
    while state.exit_code is None:
        # Check if client disconnected using select
        readable, _, exceptional = select.select([client_sock], [], [client_sock], 0)
        if readable or exceptional:
            # Client sent data or disconnected - check with recv
            try:
                data = client_sock.recv(1, socket.MSG_PEEK | socket.MSG_DONTWAIT)
                if not data:
                    # EOF - client disconnected
                    return {'status': 'error', 'error': 'Client disconnected'}
            except (BlockingIOError, OSError):
                # Error or would block - assume disconnected
                return {'status': 'error', 'error': 'Client disconnected'}

        state.check_child_status()
        time.sleep(0.1)

    return {
        'status': 'ok',
        'data': {
            'exit_code': state.exit_code
        }
    }


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
    request = {'type': 'OUTPUT'}
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
    # Use --text if provided, otherwise read from stdin
    if args.text is not None:
        data = unescape(args.text)
    else:
        data = sys.stdin.read()

    request = {'type': 'INPUT', 'data': {'data': data}}
    response = send_request(args.socket, request)

    if response['status'] == 'error':
        print(f"Error: {response.get('error', 'Unknown error')}", file=sys.stderr)
        sys.exit(1)


def cmd_running(args):
    """Running command - check if daemon is running"""
    request = {'type': 'RUNNING'}
    response = send_request(args.socket, request)

    if response['status'] == 'error':
        print(f"Error: {response.get('error', 'Unknown error')}", file=sys.stderr)
        sys.exit(1)

    data = response['data']
    if not data['running']:
        # Print exit code when process is finished
        if data['exit_code'] is not None:
            print(data['exit_code'])
        sys.exit(1)


def cmd_stop(args):
    """Stop command - stop the daemon"""
    request = {'type': 'STOP'}
    response = send_request(args.socket, request)

    if response['status'] == 'error':
        print(f"Error: {response.get('error', 'Unknown error')}", file=sys.stderr)
        sys.exit(1)


def cmd_wait(args):
    """Wait command - wait for program to exit"""
    request = {'type': 'WAIT'}
    response = send_request(args.socket, request)

    if response['status'] == 'error':
        print(f"Error: {response.get('error', 'Unknown error')}", file=sys.stderr)
        sys.exit(1)

    data = response['data']
    if data['exit_code'] is not None:
        # Print exit code but exit with 0 (success) to match Rust behavior
        print(data['exit_code'])
        sys.exit(0)


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
    # Ensure stdout is unbuffered for immediate output
    sys.stdout.reconfigure(line_buffering=True)

    parser = argparse.ArgumentParser(description='🌀 an Interactive Terminal for AI')
    subparsers = parser.add_subparsers(dest='command', required=True)

    # Start command
    start_parser = subparsers.add_parser('start', help='Start a program')
    start_parser.add_argument('--socket', help='Socket path')
    start_parser.add_argument('--size', default='80x24', help='Terminal size (COLSxROWS)')
    start_parser.add_argument('--no-daemon', action='store_true', help='Run in foreground')
    start_parser.add_argument('command', nargs='+', help='Command to run')
    start_parser.set_defaults(func=cmd_start)

    # Output command
    output_parser = subparsers.add_parser('output', help='Get screen output')
    output_parser.add_argument('--socket', required=True, help='Socket path')
    output_parser.add_argument('--cursor', default='none', choices=['none', 'print', 'inverse', 'both'],
                               help='Cursor display mode (default: none)')
    output_parser.set_defaults(func=cmd_output)

    # Input command
    input_parser = subparsers.add_parser('input', help='Send input')
    input_parser.add_argument('--socket', required=True, help='Socket path')
    input_parser.add_argument('--text', help='Input text with escape sequences (alternative to stdin)')
    input_parser.set_defaults(func=cmd_input)

    # Running command
    running_parser = subparsers.add_parser('running', help='Check if running')
    running_parser.add_argument('--socket', required=True, help='Socket path')
    running_parser.set_defaults(func=cmd_running)

    # Stop command
    stop_parser = subparsers.add_parser('stop', help='Stop the daemon')
    stop_parser.add_argument('--socket', required=True, help='Socket path')
    stop_parser.set_defaults(func=cmd_stop)

    # Wait command
    wait_parser = subparsers.add_parser('wait', help='Wait for exit')
    wait_parser.add_argument('--socket', required=True, help='Socket path')
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
