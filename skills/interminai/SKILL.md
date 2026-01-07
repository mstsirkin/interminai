---
name: interminai
description: Control interactive terminal applications like vim, git rebase -i, git add -i, apt, rclone config, and TUI apps. Use when you need to interact with applications that require keyboard input, show prompts, menus, or have full-screen interfaces. Also use when commands fail or hang with errors like "Input is not a terminal" or "Output is not a terminal".
allowed-tools: Shell
license: See LICENSE file
metadata:
  author: Michael S. Tsirkin <mst@kernel.org>
  version: 0.1.0
  category: terminal
---

# 🌀 an Interactive Terminal for AI (interminai)

Author: Michael S. Tsirkin <mst@kernel.org>

A terminal proxy for interactive CLI applications. See [examples.md](examples.md) and [reference.md](reference.md) for details.

## When to Use

**Use for interactive commands** that wait for input, show menus/prompts, or use full-screen interfaces (vim, git rebase -i, htop, apt).

**Use if you get errors like this "Warning: Output is not to a terminal" or "Warning: Input is not from a terminal".

**Don't use** for simple commands that just run and exit - use Shell instead.

## Quick Start

```bash
# 1. Start session
SOCKET=`mktemp -d /tmp/interminai-XXXXXX`/sock
./scripts/interminai start --socket "$SOCKET" -- COMMAND

# 2. Send input (--text supports escapes: \n \e \t \xHH etc.)
./scripts/interminai input --socket "$SOCKET" --text ':wq\n'

# 3. Check screen
./scripts/interminai output --socket "$SOCKET"

# 4. Clean up (always!)
./scripts/interminai stop --socket "$SOCKET"
rm "$SOCKET"; rmdir `dirname "$SOCKET"`
```

## Essential Commands

- `start --socket PATH -- COMMAND` - Start application
- `input --socket PATH --text 'text'` - Send input (escapes: `\n` `\e` `\t` `\xHH`)
- `output --socket PATH` - Get screen (add `--cursor print` for cursor position)
- `stop --socket PATH` - Stop session

## Key Best Practices

1. **Unique sockets**: Use `` SOCKET=`mktemp -d /tmp/interminai-XXXXXX`/sock ``
2. **Always clean up**: `stop`, then `rm` the socket directory
3. **Check output after each input** - don't blindly chain commands
4. **Add delays**: `sleep 0.2` after input for processing
5. **Set GIT_EDITOR=vim** for git rebase -i, git commit, etc.
6. **If screen garbled**: Send `\f` (Ctrl+L) to redraw

## Terminal Size

Default terminal size is 80x24. If not enough context fits on screen, use `--size` on start or `resize` to increase the window. Don't go overboard to avoid filling your context with excessive output.

```bash
# Start with larger terminal
./scripts/interminai start --socket "$SOCKET" --size 80x256 -- COMMAND

# Or resize during session
./scripts/interminai resize --socket "$SOCKET" --size 80x256
```

## Vim Navigation Tips

Exact counts for `h`/`j`/`k`/`l` are critical - cursor position after `dd` isn't always intuitive. Prefer:

- `:<number>` - Go to line directly (`:5\ndd`)
- `/<pattern>` - Search for text (`/goodbye\ndd`)
- `gg`/`G` - Anchor from known position
- `--cursor print` - Check position after operations
- `:%s/old/new/gc` - Search and replace with confirmation (`y`/`n` for each match)

## Complex Edits Shortcut

For complex multi-line edits, another option is to edit outside vim:

1. Use `output` to observe the file name
2. Use the Edit tool to modify the file directly
3. In vim, reload the file (`:e!\n`) or simply exit (`:q!\n`)

This avoids tricky vim navigation for large or intricate changes.

## Git Example

```bash
SOCKET=`mktemp -d /tmp/interminai-XXXXXX`/sock
GIT_EDITOR=vim ./scripts/interminai start --socket "$SOCKET" -- git rebase -i HEAD~3
sleep 0.5
./scripts/interminai output --socket "$SOCKET"
# ... edit with input commands ...
./scripts/interminai input --socket "$SOCKET" --text ':wq\n'
./scripts/interminai wait --socket "$SOCKET"
rm "$SOCKET"; rmdir `dirname "$SOCKET"`
```
