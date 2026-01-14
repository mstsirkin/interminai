#!/usr/bin/env python3
"""MCP server for interminai - control interactive terminal applications."""

import subprocess
import sys
import os
from pathlib import Path
from mcp.server.fastmcp import FastMCP

def load_instructions():
    """Load instructions from SKILL.md in the install directory."""
    skill_dir = Path.home() / ".mcp" / "skills" / "interminai"
    skill_md = skill_dir / "SKILL.md"
    if skill_md.exists():
        return skill_md.read_text()
    # Fallback for development
    dev_skill = Path(__file__).parent / "skills" / "interminai" / "SKILL.md"
    if dev_skill.exists():
        return dev_skill.read_text()
    return "interminai: Control interactive terminal applications (vim, git rebase -i, sudo, etc.)"

mcp = FastMCP("interminai", instructions=load_instructions())

def find_interminai_binary():
    """Find interminai binary - check skill install location first, then PATH."""
    # Check installed skill location (~/.mcp/skills/interminai/scripts/)
    skill_binary = Path.home() / ".mcp" / "skills" / "interminai" / "scripts" / "interminai"
    if skill_binary.exists():
        return str(skill_binary)
    # Fall back to system PATH
    return "interminai"

def run_interminai(*args, timeout=30):
    """Run interminai command and return output."""
    binary = find_interminai_binary()
    result = subprocess.run(
        [binary, *args],
        capture_output=True,
        text=True,
        timeout=timeout
    )
    return result.stdout, result.stderr, result.returncode


@mcp.tool()
def interminai_start(command: list[str], size: str = "80x24") -> str:
    """Start an interactive terminal application (vim, git rebase -i, etc).

    Args:
        command: Command and arguments to run (e.g., ["vim", "file.txt"])
        size: Terminal size WxH (default: 80x24)
    """
    args = ["start", "--size", size, "--"]
    args.extend(command)
    stdout, stderr, code = run_interminai(*args)
    if code == 0:
        return f"Started. Socket: {stdout.strip()}"
    else:
        return f"Error: {stderr}"


@mcp.tool()
def interminai_input(socket: str, text: str) -> str:
    """Send input to terminal session.

    Escape sequences: \\r=Enter, \\e=Escape, \\t=Tab, \\n=Newline, \\xHH=hex byte

    Args:
        socket: Socket path from interminai_start
        text: Text to send (with escape sequences like \\r for Enter)
    """
    stdout, stderr, code = run_interminai("input", "--socket", socket, "--text", text)
    return "Input sent" if code == 0 else f"Error: {stderr}"


@mcp.tool()
def interminai_output(socket: str, cursor: str = "none") -> str:
    """Get current terminal screen content.

    Args:
        socket: Socket path
        cursor: How to show cursor - none, print, inverse, or both
    """
    args = ["output", "--socket", socket]
    if cursor != "none":
        args.extend(["--cursor", cursor])
    stdout, stderr, code = run_interminai(*args)
    return stdout if code == 0 else f"Error: {stderr}"


@mcp.tool()
def interminai_status(socket: str) -> str:
    """Check if process is running and if there's new output (activity).

    Args:
        socket: Socket path
    """
    stdout, stderr, code = run_interminai("status", "--socket", socket)
    return stdout if stdout else stderr


@mcp.tool()
def interminai_wait(socket: str, quiet: bool = False, timeout: int = 30) -> str:
    """Wait for terminal activity (new output) or process exit.

    Args:
        socket: Socket path
        quiet: If true, wait for exit only (not activity)
        timeout: Timeout in seconds (default: 30)
    """
    args = ["wait", "--socket", socket]
    if quiet:
        args.append("--quiet")
    try:
        stdout, stderr, code = run_interminai(*args, timeout=timeout)
        return stdout if stdout else "Process exited"
    except subprocess.TimeoutExpired:
        return "Timeout waiting"


@mcp.tool()
def interminai_stop(socket: str) -> str:
    """Stop terminal session and clean up.

    Args:
        socket: Socket path
    """
    stdout, stderr, code = run_interminai("stop", "--socket", socket)
    return "Stopped" if code == 0 else f"Error: {stderr}"


@mcp.tool()
def interminai_kill(socket: str, signal: str = "SIGTERM") -> str:
    """Send signal to the process.

    Args:
        socket: Socket path
        signal: Signal name or number (e.g., SIGINT, SIGKILL, 9)
    """
    stdout, stderr, code = run_interminai("kill", "--socket", socket, "--signal", signal)
    return "Signal sent" if code == 0 else f"Error: {stderr}"


if __name__ == "__main__":
    mcp.run(transport="stdio")
