.PHONY: help install-skill install-skill-rust install-skill-python install-skill-impl install-claude install-claude-rust install-claude-python install-atomic build test-rust test-python test-skill demo demo-gdb clean

.DEFAULT_GOAL := help

help: ## Show this help message
	@echo "Available targets:"
	@echo ""
	@echo "  make install-skill        - Install Rust implementation (default)"
	@echo "  make install-skill-rust   - Install Rust implementation to skills/interminai/scripts/"
	@echo "  make install-skill-python - Install Python implementation to skills/interminai/scripts/"
	@echo "  make install-claude       - Install Rust skill to ~/.claude/skills/ for Claude Code (default)"
	@echo "  make install-claude-rust  - Install Rust skill to ~/.claude/skills/ for Claude Code"
	@echo "  make install-claude-python - Install Python skill to ~/.claude/skills/ for Claude Code"
	@echo "  make test                 - Run all tests"
	@echo "  make test-rust            - Run tests with Rust implementation"
	@echo "  make test-python          - Run tests with Python implementation"
	@echo "  make test-skill           - Validate skill using skills-ref"
	@echo "  make demo                 - Generate demo.gif showing Claude using interminai"
	@echo "  make demo-gdb             - Generate demo-gdb.gif showing Claude debugging with gdb"
	@echo "  make build                - Generate a release binary (don't install)
	@echo "  make clean                - Remove build artifacts and executables installed in this repo"
	@echo "  make help                 - Show this help message"
	@echo ""

build:
	@test "$(NAME)" "!=" "Rust" || \
		(echo "Building Rust release binary..." ; \
		 cargo build --release)

install-skill: install-skill-rust ## Install Rust implementation (default)

install-skill-rust: NAME = Rust
install-skill-rust: SRC = target/release/interminai
install-skill-rust: install-skill-impl

install-skill-python: NAME = Python
install-skill-python: SRC = interminai.py
install-skill-python: install-skill-impl

install-skill-impl: DST = agent/skills
install-skill-impl: install-atomic
	@echo "(accessible via .claude/skills and .codex/skills symlinks)"

install-claude: install-claude-rust ## Install Rust skill to ~/.claude/skills/ for Claude Code

install-claude-rust: NAME = Rust
install-claude-rust: SRC = target/release/interminai
install-claude-rust: DST = ~/.claude/skills
install-claude-rust: install-atomic

install-claude-python: NAME = Python
install-claude-python: SRC = interminai.py
install-claude-python: DST = ~/.claude/skills
install-claude-python: install-atomic

install-atomic: build
	@test -n "$(DST)"
	@mkdir -p $(DST)-backup
	@mkdir -p $(DST)/interminai
	@TMPDIR=$$(mktemp -d $(DST)-backup/XXXXXX) && \
		cp -r skills/interminai "$$TMPDIR/interminai" && \
		test -n "$(SRC)" && cp $(SRC) "$$TMPDIR/interminai/scripts/interminai"; \
		mkdir -p $(DST) && \
		mv --exchange "$$TMPDIR/interminai" $(DST)
	@echo "Installed $(NAME) version to $(DST)/interminai"

test: test-rust test-python test-skill

test-rust:
	cargo test

test-python:
	@echo "Running tests with Python implementation..."
	OVERRIDE_CARGO_BIN_EXE_interminai=$(PWD)/interminai.py cargo test

test-skill: subprojects/agentskills/skills-ref/.venv ## Validate skill using skills-ref
	@echo "Validating skill..."
	@. subprojects/agentskills/skills-ref/.venv/bin/activate && skills-ref validate skills/interminai
	@echo "Skill validation passed"

subprojects/agentskills/skills-ref/.venv:
	@git submodule update --init
	@echo "Installing skills-ref..."
	@cd subprojects/agentskills/skills-ref && uv sync

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
	@echo "Demo created: demo-gdb.gif"

clean:
	cargo clean
	rm -rf agent
	-git submodule deinit -f subprojects/agentskills
