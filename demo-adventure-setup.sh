#!/bin/bash
# Setup script to prepare for recording a real Claude CLI session
set -e

echo "Setting up demo environment..."

SRC=${PWD}
# Ensure clean start - remove any existing directory
cd /tmp
rm -rf adventure-demo

# Create fresh demo git repository
mkdir adventure-demo
cd adventure-demo


echo ""
echo "✓ Demo directory ready at /tmp/adventure-demo"
mkdir -p .claude
cp -fr ${SRC}/skills .claude/
echo "Skills:"
ls .claude/skills
echo ""
echo "Now record your Claude CLI session with:"
echo "  cd /tmp/adventure-demo"
echo "  # Start recording (asciinema or vhs)"
echo -e "  # Ask Claude: 'You will run adventure and play it for me. To avoid\n" \
     "being lost, keep files map.txt with a grid-based map explored, where each room\n" \
     "is marked by a letter, and items.txt with items on \n" \
     "the map. Keep them up to date periodically! you might need to expand the map or have \n" \
     "several maps if you e.g. teleport. Once in a while print an emoji to make it stand out and update\n" \
     "me on progress so I can track it. Write a copy of these, with the emojis into log.txt.\n"
     "Each log entry must include time in seconds. Keep playing until you finish the game.\n" 
