.PHONY: help install-skill install-skill-rust install-skill-python install-claude build test test-python demo demo-gdb clean

.DEFAULT_GOAL := help

help: ## Show this help message
	@echo "Available targets:"
	@echo ""
	@echo "  make install-skill        - Install Rust implementation (default)"
	@echo "  make install-skill-rust   - Install Rust implementation to skills/interminai/scripts/"
	@echo "  make install-skill-python - Install Python implementation to skills/interminai/scripts/"
	@echo "  make install-claude       - Install skill to ~/.claude/skills/ for Claude Code"
	@echo "  make test                 - Run tests with Rust implementation"
	@echo "  make test-python          - Run tests with Python implementation"
	@echo "  make demo                 - Generate demo.gif showing Claude using interminai"
	@echo "  make demo-gdb             - Generate demo-gdb.gif showing Claude debugging with gdb"
	@echo "  make build                - Generate a release binary (don't install)
	@echo "  make clean                - Remove build artifacts and installed binaries"
	@echo "  make help                 - Show this help message"
	@echo ""

build:
	@echo "Building Rust release binary..."
	@cargo build --release

install-skill: install-skill-rust ## Install Rust implementation (default)

install-skill-rust: build
	@echo "Installing to skills/interminai/scripts/"
	@mkdir -p skills/interminai/scripts
	@cp target/release/interminai skills/interminai/scripts/interminai
	@echo "✓ Installed Rust version to skills/interminai/scripts/interminai"
	@echo "  (accessible via .claude/skills and .codex/skills symlinks)"

install-skill-python:
	@echo "Installing Python implementation..."
	@mkdir -p skills/interminai/scripts
	@cp interminai.py skills/interminai/scripts/interminai
	@chmod +x skills/interminai/scripts/interminai
	@echo "✓ Installed Python version to skills/interminai/scripts/interminai"
	@echo "  (accessible via .claude/skills and .codex/skills symlinks)"

install-claude: install-skill ## Install skill to ~/.claude/skills/ for Claude Code
	@echo "Installing skill to ~/.claude/skills/..."
	@mkdir -p ~/.claude/skills
	@cp -r skills/interminai ~/.claude/skills/
	@echo "✓ Installed skill to ~/.claude/skills/interminai"

test:
	cargo test

test-python:
	@echo "Running tests with Python implementation..."
	OVERRIDE_CARGO_BIN_EXE_interminai=$(PWD)/interminai.py cargo test

demo:
	@echo "Setting up clean demo repository..."
	@./demo-setup.sh
	@echo "Recording demo with VHS..."
	@FORCE_COLOR=1 vhs demo-real.tape
	@echo "✓ Demo created: demo.gif"

demo-gdb:
	@echo "Setting up GDB demo environment..."
	@./demo-gdb-setup.sh
	@echo "Recording GDB demo with VHS..."
	@FORCE_COLOR=1 vhs demo-gdb.tape
	@echo "✓ Demo created: demo-gdb.gif"

clean:
	cargo clean
	rm -f skills/interminai/scripts/interminai
