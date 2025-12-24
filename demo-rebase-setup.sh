#!/bin/bash
# Setup script to test rebase workflow with interminai
set -e

echo "Setting up rebase demo environment..."

SRC=${PWD}
cd /tmp
rm -rf interminai-rebase-demo

# Create fresh demo git repository
mkdir interminai-rebase-demo
cd interminai-rebase-demo

git init
git config user.name "Demo User"
git config user.email "demo@example.com"

# Initial commit
cat > config.txt << 'EOF'
# Configuration
debug=false
timeout=30
EOF
git add config.txt
git commit -m "Initial config"

# Create feature branch
git checkout -b feature

# Feature branch changes timeout
sed -i 's/timeout=30/timeout=60/' config.txt
git add config.txt
git commit -m "Increase timeout to 60"

# Add another commit on feature
echo "cache=enabled" >> config.txt
git add config.txt
git commit -m "Enable cache"

# Go back to master and create a conflicting change
git checkout master

# Master branch also changes timeout (conflict!)
sed -i 's/timeout=30/timeout=45/' config.txt
git add config.txt
git commit -m "Adjust timeout to 45"

# Copy interminai skill
mkdir -p .claude
cp -fr ${SRC}/skills .claude/

echo ""
echo "✓ Demo repository ready at /tmp/interminai-rebase-demo"
echo ""
echo "Branches:"
git branch -a
echo ""
echo "Master log:"
git log --oneline master
echo ""
echo "Feature log:"
git log --oneline feature
echo ""
echo "To test rebase conflict workflow:"
echo "  cd /tmp/interminai-rebase-demo"
echo "  git checkout feature"
echo "  # Then use interminai to run: git rebase master"
echo "  # This will create a conflict on config.txt"
