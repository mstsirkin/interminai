Output demo.gif

Set FontSize 16
Set Width 800
Set Height 600
Set Theme "VibrantInk"
Set TypingSpeed 100ms

# NOTE: Run ./demo-adventure-setup.sh before running this tape to ensure clean git repo
Type "cd /tmp/adventure-demo"
Enter
Sleep 1s
Type "pwd"
Enter
Sleep 1s

Type "ls .claude/skills/"
Enter
Sleep 1s

Type "claude --dangerously-skip-permissions"
Enter
Sleep 3s

# User's request to Claude - let Claude figure out the approach
Type "You will run adventure and play it for me. To avoid
     being lost, keep files map.txt with a grid-based map explored where each room
     is marked by a special letter, and items.txt with items on
     the map. Keep them up to date periodically! you might need several maps if you
     e.g. teleport. Once in a while print an emoji to make it stand out and update
     me on progress so I can track it. Write a copy of these, with the emojis into log.txt.
     Each log entry must include time in seconds. Keep playing until you finish the game.
Enter

# Wait for Claude to:
# - Start up and connect (25s)
# - Check git log
# - Start interminai session
# - Run git rebase
# - Capture vim output
# - Complete rebase
# - Verify result
Sleep 600s

# Exit Claude
Ctrl+C
Sleep 2s

# Verify the result (commented out - Claude does this itself)
# Type "git log --oneline"
# Enter
# Sleep 15s
