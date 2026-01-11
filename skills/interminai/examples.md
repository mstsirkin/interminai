# Examples

Detailed examples for common use cases.

## Example 1: Edit File with Vim

Complete workflow for editing a file with vim.

```bash
#!/bin/bash
SOCK=`mktemp -d /tmp/interminai-XXXXXX`/sock

# Start vim (runs in background by default)
./scripts/interminai start --socket "$SOCK" -- vim myfile.txt
sleep 0.5  # Brief wait for daemon to initialize

# Check what's on screen
./scripts/interminai output --socket "$SOCK"
# Output shows vim's initial screen

# Enter insert mode
printf "i" | ./scripts/interminai input --socket "$SOCK"
sleep 0.2

# Type content
printf "Hello, World!" | ./scripts/interminai input --socket "$SOCK"
sleep 0.1

# Save and quit (ESC, :wq, Enter)
printf "\x1b:wq\n" | ./scripts/interminai input --socket "$SOCK"

# Wait for vim to exit
./scripts/interminai wait --socket "$SOCK"

# Stop daemon
./scripts/interminai stop --socket "$SOCK"
rm "$SOCK"; rmdir `dirname "$SOCK"`  # Clean up temp directory

echo "File edited successfully!"
```

## Example 2: Git Interactive Add (git add -i)

Selectively stage files using git's interactive add mode.

```bash
#!/bin/bash
SOCK=`mktemp -d /tmp/interminai-XXXXXX`/sock

# Start bash (git add -i exits quickly, so wrap in bash to keep session alive)
GIT_EDITOR=vim ./scripts/interminai start --socket "$SOCK" -- bash
sleep 0.5

echo "=== Starting git add -i ==="
printf "git add -i\n" | ./scripts/interminai input --socket "$SOCK"
sleep 1

# Check the menu
./scripts/interminai output --socket "$SOCK"
# Shows:
#   1: status  2: update  3: revert  4: add untracked
#   5: patch   6: diff    7: quit    8: help
# What now>

echo "=== Selecting files to stage (option 2: update) ==="
printf "2\n" | ./scripts/interminai input --socket "$SOCK"
sleep 0.5

# See the file list
./scripts/interminai output --socket "$SOCK"
# Shows numbered list of modified files

echo "=== Staging files 1,3,5 ==="
printf "1,3,5\n" | ./scripts/interminai input --socket "$SOCK"
sleep 0.3

# Confirm selection (empty line)
printf "\n" | ./scripts/interminai input --socket "$SOCK"
sleep 0.5

# Verify files were staged
./scripts/interminai output --socket "$SOCK"
# Should show "updated N paths"

# Note: If output looks garbled, re-run output command
# Screen updates take time and you may catch it mid-update
sleep 0.2
./scripts/interminai output --socket "$SOCK"

echo "=== Quitting git add -i ==="
printf "7\n" | ./scripts/interminai input --socket "$SOCK"
sleep 0.3

# Exit bash
printf "exit\n" | ./scripts/interminai input --socket "$SOCK"
sleep 0.5

# Clean up
./scripts/interminai stop --socket "$SOCK"
rm "$SOCK"; rmdir `dirname "$SOCK"`

echo "=== Files staged successfully! ==="
```

## Example 3: Git Interactive Rebase

Squash the last 3 commits into one.

```bash
#!/bin/bash
SOCK=`mktemp -d /tmp/interminai-XXXXXX`/sock

# IMPORTANT: Wrap in bash so session stays alive after rebase exits
# Set GIT_EDITOR=vim to ensure vim is used (recommended for interminai)
GIT_EDITOR=vim ./scripts/interminai start --socket "$SOCK" -- bash
sleep 0.5

echo "=== Starting interactive rebase ==="
printf "git rebase -i HEAD~3\n" | ./scripts/interminai input --socket "$SOCK"
sleep 1.5

# See the commit list
./scripts/interminai output --socket "$SOCK"
# Output shows:
#   pick abc123 First commit
#   pick def456 Second commit
#   pick ghi789 Third commit

echo "=== Changing commits to squash ==="
# Move to second commit and change to squash
printf "j" | ./scripts/interminai input --socket "$SOCK"          # j=down
printf "ciwsquash\x1b" | ./scripts/interminai input --socket "$SOCK"  # change inner word
sleep 0.2

# Move to third commit and change to squash
printf "j" | ./scripts/interminai input --socket "$SOCK"
printf "ciwsquash\x1b" | ./scripts/interminai input --socket "$SOCK"
sleep 0.2

# Save and exit rebase plan
printf ":wq\n" | ./scripts/interminai input --socket "$SOCK"
sleep 1

# Rebase opens commit message editor
./scripts/interminai output --socket "$SOCK"

echo "=== Editing commit message ==="
# Clear default message and write new one
printf "ggdG" | ./scripts/interminai input --socket "$SOCK"  # Delete all
printf "iCombined commit: feat, fix, and refactor\x1b:wq\n" | ./scripts/interminai input --socket "$SOCK"
sleep 1

# Check result
./scripts/interminai output --socket "$SOCK"
# Should show: "Successfully rebased and updated refs/heads/main"

# Clean up
printf "exit\n" | ./scripts/interminai input --socket "$SOCK"
sleep 0.5
./scripts/interminai stop --socket "$SOCK"
rm "$SOCK"; rmdir `dirname "$SOCK"`  # Clean up temp directory

echo "=== Rebase complete! ==="
```

## Example 4: Handle Interactive Prompt

Responding to Y/n prompts.

```bash
#!/bin/bash
SOCK=`mktemp -d /tmp/interminai-XXXXXX`/sock

# Start command with interactive prompt
./scripts/interminai start --socket "$SOCK" -- apt install something
sleep 1

# Check what the prompt is asking
OUTPUT=`./scripts/interminai output --socket "$SOCK"`
echo "Prompt says:"
echo "$OUTPUT"

# Check for specific prompt
if echo "$OUTPUT" | grep -q "Do you want to continue"; then
    echo "Sending 'y' to continue..."
    printf "y\n" | ./scripts/interminai input --socket "$SOCK"
fi

# Wait for completion
./scripts/interminai wait --socket "$SOCK"

# Get final output
./scripts/interminai output --socket "$SOCK"

# Clean up
./scripts/interminai stop --socket "$SOCK"
rm "$SOCK"; rmdir `dirname "$SOCK"`  # Clean up temp directory
```

## Example 5: Handle Rebase Conflicts

Detect and resolve conflicts during interactive rebase.

```bash
#!/bin/bash
SOCK=`mktemp -d /tmp/interminai-XXXXXX`/sock

# Set GIT_EDITOR=vim to ensure vim is used (recommended for interminai)
GIT_EDITOR=vim ./scripts/interminai start --socket "$SOCK" -- bash
sleep 0.5

# Start rebase
printf "git rebase -i HEAD~5\n" | ./scripts/interminai input --socket "$SOCK"
sleep 1.5

# Modify commits (squash some)
printf "jciwsquash\x1b:wq\n" | ./scripts/interminai input --socket "$SOCK"
sleep 1

# Check if conflict occurred
OUTPUT=`./scripts/interminai output --socket "$SOCK"`
if echo "$OUTPUT" | grep -q "CONFLICT"; then
    echo "=== Conflict detected! ==="

    # Show conflict details
    printf "git status\n" | ./scripts/interminai input --socket "$SOCK"
    sleep 0.5
    ./scripts/interminai output --socket "$SOCK"

    # Open conflicted file in vim
    printf "vim conflicted_file.txt\n" | ./scripts/interminai input --socket "$SOCK"
    sleep 1

    # Search for conflict markers
    printf "/<<<<<<\n" | ./scripts/interminai input --socket "$SOCK"
    sleep 0.2

    # Resolve conflict (example: keep incoming changes)
    printf "dd" | ./scripts/interminai input --socket "$SOCK"  # Delete <<<<<<< line
    printf "jdd" | ./scripts/interminai input --socket "$SOCK"  # Delete ======= line
    printf "/>>>>>>>\n" | ./scripts/interminai input --socket "$SOCK"
    printf "dd" | ./scripts/interminai input --socket "$SOCK"  # Delete >>>>>>> line

    # Save
    printf ":wq\n" | ./scripts/interminai input --socket "$SOCK"
    sleep 0.5

    # Continue rebase
    printf "git add .\n" | ./scripts/interminai input --socket "$SOCK"
    printf "git rebase --continue\n" | ./scripts/interminai input --socket "$SOCK"
    sleep 1

    ./scripts/interminai output --socket "$SOCK"
fi

# Clean up
printf "exit\n" | ./scripts/interminai input --socket "$SOCK"
./scripts/interminai stop --socket "$SOCK"
rm "$SOCK"; rmdir `dirname "$SOCK"`  # Clean up temp directory
```

## Example 6: Navigate in Vim

Advanced vim navigation and editing.

```bash
#!/bin/bash
SOCK=`mktemp -d /tmp/interminai-XXXXXX`/sock

./scripts/interminai start --socket "$SOCK" -- vim large_file.txt
sleep 1

# Go to specific line (line 42)
printf "42G" | ./scripts/interminai input --socket "$SOCK"
sleep 0.2

# Check we're at the right place
./scripts/interminai output --socket "$SOCK"

# Insert text at beginning of line
printf "I# TODO: " | ./scripts/interminai input --socket "$SOCK"  # I = insert at line start
printf "\x1b" | ./scripts/interminai input --socket "$SOCK"  # Back to normal mode
sleep 0.2

# Go to end of file
printf "G" | ./scripts/interminai input --socket "$SOCK"
sleep 0.2

# Add new line at end
printf "o" | ./scripts/interminai input --socket "$SOCK"  # Open line below
printf "New final line" | ./scripts/interminai input --socket "$SOCK"
printf "\x1b" | ./scripts/interminai input --socket "$SOCK"
sleep 0.2

# Save
printf ":w\n" | ./scripts/interminai input --socket "$SOCK"
sleep 0.5

# Quit
printf ":q\n" | ./scripts/interminai input --socket "$SOCK"

./scripts/interminai wait --socket "$SOCK"
./scripts/interminai stop --socket "$SOCK"
rm "$SOCK"; rmdir `dirname "$SOCK"`  # Clean up temp directory
```

## Example 7: Multiple Terminal Sizes

Resize terminal during session.

```bash
#!/bin/bash
SOCK=`mktemp -d /tmp/interminai-XXXXXX`/sock

# Start with narrow terminal
./scripts/interminai start --socket "$SOCK" --size 40x24 -- vim file.txt
sleep 1

echo "=== Narrow view (40 columns) ==="
./scripts/interminai output --socket "$SOCK"

# Resize to wide
interminai resize --socket "$SOCK" --size 120x24
sleep 0.5

echo "=== Wide view (120 columns) ==="
./scripts/interminai output --socket "$SOCK"

# Quit vim
printf ":q!\n" | ./scripts/interminai input --socket "$SOCK"
./scripts/interminai wait --socket "$SOCK"
./scripts/interminai stop --socket "$SOCK"
rm "$SOCK"; rmdir `dirname "$SOCK"`  # Clean up temp directory
```

## Example 8: Vim Search and Replace

Using vim's powerful search/replace through interminai.

```bash
#!/bin/bash
SOCK=`mktemp -d /tmp/interminai-XXXXXX`/sock

# Open file with vim
./scripts/interminai start --socket "$SOCK" -- vim myfile.txt
sleep 0.5

# Verify file loaded
./scripts/interminai output --socket "$SOCK"

# IMPORTANT: When using printf with vim commands containing %,
# you must escape % as %% (printf treats % as format specifier)

# Example 1: Replace all occurrences of "foo" with "bar"
# Note the %% to escape the % in :%s command
printf ':%%s/foo/bar/g\n' | ./scripts/interminai input --socket "$SOCK"
sleep 0.3

# Example 2: Remove trailing whitespace from all lines
# The 'e' flag suppresses errors if pattern not found
printf ':%%s/\\s\\+$//ge\n' | ./scripts/interminai input --socket "$SOCK"
sleep 0.3

# Example 3: Replace only in lines 10-20
printf ':10,20s/old/new/g\n' | ./scripts/interminai input --socket "$SOCK"
sleep 0.3

# Example 4: Replace with confirmation (c flag)
# This would prompt for each replacement (not shown in example)
# printf ':%%s/pattern/replacement/gc\n' | ./scripts/interminai input --socket "$SOCK"

# Save and check result
printf ':w\n' | ./scripts/interminai input --socket "$SOCK"
sleep 0.5
./scripts/interminai output --socket "$SOCK"

# Quit
printf ':q\n' | ./scripts/interminai input --socket "$SOCK"

# Wait for vim to exit
./scripts/interminai wait --socket "$SOCK"

# Clean up
./scripts/interminai stop --socket "$SOCK"
rm "$SOCK"; rmdir `dirname "$SOCK"`

echo "Search and replace complete!"
```

**Key Points:**
- `%%s` - The `%%` escapes to a single `%` in printf
- `/g` - Global flag (replace all occurrences on each line)
- `/e` - Suppress error if pattern not found
- `/c` - Confirm each replacement (interactive)

## Example 9: Sudo with Password

Run commands requiring sudo authentication.

```bash
#!/bin/bash
SOCK=`mktemp -d /tmp/interminai-XXXXXX`/sock

# Start sudo command
./scripts/interminai start --socket "$SOCK" -- sudo apt update
sleep 0.5

# Check for password prompt
OUTPUT=`./scripts/interminai output --socket "$SOCK"`
echo "$OUTPUT"

if echo "$OUTPUT" | grep -qi "password"; then
    echo "=== Password required, prompting user ==="
    # Use --password to securely get password from user (not echoed)
    ./scripts/interminai input --socket "$SOCK" --password
    # User types password in their terminal, Enter appended automatically
fi

# Wait for command to complete
./scripts/interminai wait --socket "$SOCK"

# Show final output
./scripts/interminai output --socket "$SOCK"

# Clean up
./scripts/interminai stop --socket "$SOCK"
rm "$SOCK"; rmdir `dirname "$SOCK"`

echo "=== Sudo command complete ==="
```

**Key Points:**
- `--password` reads password with echo disabled (secure)
- Enter (`\r`) is automatically appended
- Works with sudo, ssh, and any password prompt
- The password is never visible on screen or in logs

## Common Vim Command Patterns

Quick reference for vim operations via interminai:

```bash
SOCK=`mktemp -d /tmp/interminai-XXXXXX`/sock

# Navigation
printf "gg" | ./scripts/interminai input --socket "$SOCK"    # Top of file
printf "G" | ./scripts/interminai input --socket "$SOCK"     # Bottom of file
printf "0" | ./scripts/interminai input --socket "$SOCK"     # Line start
printf "$" | ./scripts/interminai input --socket "$SOCK"     # Line end
printf "w" | ./scripts/interminai input --socket "$SOCK"     # Next word
printf "b" | ./scripts/interminai input --socket "$SOCK"     # Previous word

# Line-specific navigation
printf "42G" | ./scripts/interminai input --socket "$SOCK"   # Go to line 42

# Modes
printf "i" | ./scripts/interminai input --socket "$SOCK"     # Insert mode
printf "a" | ./scripts/interminai input --socket "$SOCK"     # Append mode (after cursor)
printf "A" | ./scripts/interminai input --socket "$SOCK"     # Append at end of line
printf "I" | ./scripts/interminai input --socket "$SOCK"     # Insert at beginning of line
printf "o" | ./scripts/interminai input --socket "$SOCK"     # Open line below
printf "O" | ./scripts/interminai input --socket "$SOCK"     # Open line above
printf "\x1b" | ./scripts/interminai input --socket "$SOCK"  # Back to normal mode

# Editing
printf "dd" | ./scripts/interminai input --socket "$SOCK"          # Delete line
printf "yy" | ./scripts/interminai input --socket "$SOCK"          # Yank (copy) line
printf "p" | ./scripts/interminai input --socket "$SOCK"           # Paste after
printf "P" | ./scripts/interminai input --socket "$SOCK"           # Paste before
printf "u" | ./scripts/interminai input --socket "$SOCK"           # Undo
printf "\x12" | ./scripts/interminai input --socket "$SOCK"        # Redo (Ctrl+R)
printf "cw" | ./scripts/interminai input --socket "$SOCK"          # Change word
printf "ciw" | ./scripts/interminai input --socket "$SOCK"         # Change inner word

# Search
printf "/pattern\n" | ./scripts/interminai input --socket "$SOCK"  # Search forward
printf "n" | ./scripts/interminai input --socket "$SOCK"           # Next match
printf "N" | ./scripts/interminai input --socket "$SOCK"           # Previous match

# Save and Quit
printf ":w\n" | ./scripts/interminai input --socket "$SOCK"        # Save
printf ":q\n" | ./scripts/interminai input --socket "$SOCK"        # Quit
printf ":wq\n" | ./scripts/interminai input --socket "$SOCK"       # Save and quit
printf ":q!\n" | ./scripts/interminai input --socket "$SOCK"       # Quit without saving
printf ":wq!\n" | ./scripts/interminai input --socket "$SOCK"      # Force save and quit

# Search and Replace (remember: % must be escaped as %% in printf!)
printf "/pattern\n" | ./scripts/interminai input --socket "$SOCK"          # Search forward
printf "?pattern\n" | ./scripts/interminai input --socket "$SOCK"          # Search backward
printf ":%%s/old/new/g\n" | ./scripts/interminai input --socket "$SOCK"    # Replace all in file
printf ":%%s/old/new/gc\n" | ./scripts/interminai input --socket "$SOCK"   # Replace with confirmation
printf ":%%s/old/new/gi\n" | ./scripts/interminai input --socket "$SOCK"   # Replace case-insensitive
printf ":10,20s/old/new/g\n" | ./scripts/interminai input --socket "$SOCK" # Replace in lines 10-20
printf ":%%s/\\s\\+$//ge\n" | ./scripts/interminai input --socket "$SOCK"  # Remove trailing whitespace
```
