.PHONY: help install-skill install-skill-rust install-skill-python install-skill-impl install-claude install-atomic build test-rust test-python test-skill demo demo-gdb clean

.DEFAULT_GOAL := help

help: ## Show this help message
	@echo "Available targets:"
	@echo ""
	@echo "  make install-skill        - Install Rust implementation (default)"
	@echo "  make install-skill-rust   - Install Rust implementation to skills/interminai/scripts/"
	@echo "  make install-skill-python - Install Python implementation to skills/interminai/scripts/"
	@echo "  make install-claude       - Install Rust skill to ~/.claude/skills/ for Claude Code"
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
	@echo "Building Rust release binary..."
	@cargo build --release

install-skill: install-skill-rust ## Install Rust implementation (default)

install-skill-rust: IMPL_NAME = Rust
install-skill-rust: IMPL_SRC = target/release/interminai
install-skill-rust: build install-skill-impl

install-skill-python: IMPL_NAME = Python
install-skill-python: IMPL_SRC = interminai.py
install-skill-python: install-skill-impl

install-skill-impl: INSTALL_SRC = skills/interminai
install-skill-impl: INSTALL_DST = agent/skills
install-skill-impl: INSTALL_BACKUP = agent/skills-backup
install-skill-impl: INSTALL_NAME = agent/skills/interminai
install-skill-impl: install-atomic
	@echo "Installed $(IMPL_NAME) version to agent/skills/interminai"
	@echo "(accessible via .claude/skills and .codex/skills symlinks)"

install-claude: INSTALL_SRC = agent/skills/interminai
install-claude: INSTALL_DST = ~/.claude/skills
install-claude: INSTALL_BACKUP = ~/.claude/skills-backup
install-claude: INSTALL_NAME = ~/.claude/skills/interminai
install-claude: install-skill install-atomic
	@echo "Installed skill to ~/.claude/skills/interminai"
	@echo "Old version moved to $$TMPDIR/interminai"

install-atomic:
	@test -n "$(INSTALL_SRC)"
	@test -n "$(INSTALL_DST)"
	@test -n "$(INSTALL_BACKUP)"
	@test -n "$(INSTALL_NAME)"
	@mkdir -p $(INSTALL_BACKUP)
	@mkdir -p $(INSTALL_NAME)
	@TMPDIR=$$(mktemp -d $(INSTALL_BACKUP)/XXXXXX) && \
		cp -r $(INSTALL_SRC) "$$TMPDIR/interminai" && \
		test -n "$(IMPL_SRC)" && cp $(IMPL_SRC) "$$TMPDIR/interminai/scripts/interminai" && chmod +x "$$TMPDIR/interminai/scripts/interminai"; \
		mkdir -p $(INSTALL_DST) && \
		mv --exchange "$$TMPDIR/interminai" $(INSTALL_DST)

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
