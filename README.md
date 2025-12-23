# interminai - 🌀 an Interactive Terminal for AI

A terminal proxy that enables programmatic interaction with interactive CLI applications.

![Demo: Claude using interminai for git rebase](demo.gif)

*AI agent (Claude) using interminai to perform an interactive git rebase*

![Demo: Claude using interminai for gdb](demo-gdb.gif)

*AI agent (Claude) using interminai to perform interactive gdb debugging*

## What It Does

Many powerful CLI tools require human interaction - vim waits for keystrokes, `git rebase -i` opens an editor, `apt` asks for confirmation, TUI applications respond to keyboard input. These tools can't be automated with simple shell scripts because they:

- Open full-screen interfaces
- Wait for user input
- Show dynamic menus and prompts
- Require terminal emulation

**interminai solves this** by wrapping any interactive program in a pseudo-terminal (PTY), capturing its screen output as text, and providing a simple API to send input and read the display. This allows AI agents, scripts, or automated systems to interact with vim, git, debuggers, configuration wizards, and any other interactive terminal application.

### Core Capabilities

- **Screen capture**: Read the current terminal display as ASCII text
- **Input control**: Send keystrokes, commands, and control sequences
- **Process management**: Start, monitor, signal, and stop wrapped processes
- **Daemon mode**: Run in background for long-lived interactive sessions
- **Terminal emulation**: Full PTY with proper ANSI escape sequence handling

### Use Cases

- AI agents editing files with vim
- Automated git operations (`git rebase -i`, `git add -i`, `git commit`)
- Interactive package management (`apt`, `yum`)
- Debugging with gdb or lldb
- Configuration wizards (rclone, raspi-config)
- TUI applications (htop, tmux, screen)
- Any CLI tool that requires keyboard interaction

## Installation

interminai is available in two implementations with identical functionality:

- **Rust** (recommended) - Fast, zero dependencies, single binary
- **Python** - Easier to modify, requires Python 3.6+

Choose the one that best fits your needs.

### Option 1: Rust Implementation (Recommended)

**Prerequisites:**
- Rust toolchain (rustc 1.70+, cargo)
- Linux or macOS (requires PTY support)

**Build from source:**

```bash
# Clone the repository
git clone <repository-url>
cd rust

# Build release binary
cargo build --release

# Binary will be at: target/release/interminai

# Check available commands
./target/release/interminai --help
```

**Install as Agent Skill:**

```bash
# Build and install Rust version
make install-skill-rust

# Or just:
make install-skill

# The binary is installed to: skills/interminai/scripts/interminai
# (accessible via .claude/skills and .codex/skills symlinks)
```

### Option 2: Python Implementation

**Prerequisites:**
- Python 3.6+
- Linux or macOS (requires PTY support)

**Install as Agent Skill:**

```bash
# Install Python version
make install-skill-python

# The script is installed to: skills/interminai/scripts/interminai
# (accessible via .claude/skills and .codex/skills symlinks)
```

The Python implementation (`interminai.py`) is ready to use without compilation.

```bash
# Check available commands
./interminai.py --help
```

## Quick Start

```bash
# Start vim editing a file
interminai start --socket /tmp/vim.sock -- vim myfile.txt

# Send keystrokes (enter insert mode, type text, escape, save)
interminai input --socket /tmp/vim.sock --text "iHello, World!\e:wq\n"

# View the screen
interminai output --socket /tmp/vim.sock

# Stop the daemon
interminai stop --socket /tmp/vim.sock
```

## Documentation

- **SKILL.md** - Agent skill documentation and best practices
- **examples.md** - Detailed usage examples (vim, git, debugging)
- **reference.md** - Complete command reference
- **PROTOCOL.md** - Socket communication protocol specification

## Verification

```bash
# Run tests
make test
```

## Commands

```bash
# Start an interactive program (runs as daemon by default)
interminai start [--socket PATH] [--size WxH] [--no-daemon] -- COMMAND...

# Send input
interminai input --socket PATH --text TEXT

# Get screen output
interminai output --socket PATH

# Check if running
interminai running --socket PATH

# Wait for process exit
interminai wait --socket PATH

# Send signal
interminai kill --socket PATH --signal SIGNAL

# Stop daemon
interminai stop --socket PATH
```

## License

This project is licensed under the GNU General Public License v2.0 - see the [LICENSE](LICENSE) file for details.

## Author

Michael S. Tsirkin <mst@kernel.org>
