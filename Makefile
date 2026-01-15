.PHONY: help build clean
.PHONY: install-all
.PHONY: install-skill install-skill-rust install-skill-python install-skill-impl install-atomic
.PHONY: install-claude install-claude-rust install-claude-python
.PHONY: install-codex install-codex-rust install-codex-python
.PHONY: install-mcp install-mcp-rust install-mcp-python install-cursor
.PHONY: install-tool-rust install-tool-python
.PHONY: test test-rust test-python test-xterm test-custom test-skill
.PHONY: demo demo-gdb

.DEFAULT_GOAL := help

help: ## Show this help message
	@echo "Available targets:"
	@echo ""
	@echo "  make install-all          - Install to all locations (skill, claude, codex, cursor)"
	@echo "  make install-skill        - Install Rust implementation (default)"
	@echo "  make install-skill-rust   - Install Rust implementation to skills/interminai/scripts/"
	@echo "  make install-skill-python - Install Python implementation to skills/interminai/scripts/"
	@echo "  make install-claude       - Install Rust skill to ~/.claude/skills/ for Claude Code (default)"
	@echo "  make install-claude-rust  - Install Rust skill to ~/.claude/skills/ for Claude Code"
	@echo "  make install-claude-python - Install Python skill to ~/.claude/skills/ for Claude Code"
	@echo "  make install-codex       - Install Rust skill to ~/.codex/skills/ for Codex (default)"
	@echo "  make install-codex-rust  - Install Rust skill to ~/.codex/skills/ for Codex"
	@echo "  make install-codex-python - Install Python skill to ~/.codex/skills/ for Codex"
	@echo "  make install-mcp         - Install MCP server to ~/.mcp/skills/ (manual config)"
	@echo "  make install-mcp-rust    - Install Rust MCP server"
	@echo "  make install-mcp-python  - Install Python MCP server"
	@echo "  make install-cursor      - Install MCP server and configure cursor-agent"
	@echo "  make test                 - Run all tests (both emulators, both implementations)"
	@echo "  make test-rust            - Run Rust tests with both emulators"
	@echo "  make test-python          - Run Python tests with both emulators"
	@echo "  make test-xterm           - Run tests with xterm emulator"
	@echo "  make test-custom          - Run tests with custom emulator"
	@echo "  make test-skill           - Validate skill using skills-ref"
	@echo "  make demo                 - Generate demo.gif showing Claude using interminai"
	@echo "  make demo-gdb             - Generate demo-gdb.gif showing Claude debugging with gdb"
	@echo "  make build                - Generate a release binary (don't install)"
	@echo "  make clean                - Remove build artifacts and executables installed in this repo"
	@echo "  make help                 - Show this help message"
	@echo ""

build:
	@test "$(NAME)" "!=" "Rust" || \
		(echo "Building Rust release binary..." ; \
		 cargo build --release)

install-all: install-skill install-claude install-codex install-cursor ## Install to all locations

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

install-tool-rust: NAME = Rust
install-tool-rust: SRC = target/release/interminai
install-tool-rust: DST = ~/.${TOOL}/skills
install-tool-rust: install-atomic

install-tool-python: NAME = Python
install-tool-python: SRC = interminai.py
install-tool-python: DST = ~/.${TOOL}/skills
install-tool-python: install-atomic


install-claude-rust: TOOL = claude
install-claude-rust: install-tool-rust
install-claude-python: TOOL = claude
install-claude-python: install-tool-python

install-codex: install-codex-rust ## Install Rust skill to ~/.codex/skills/ for Codex

install-codex-rust: TOOL = codex
install-codex-rust: install-tool-rust
install-codex-python: TOOL = codex
install-codex-python: install-tool-python

install-mcp: install-mcp-rust ## Install MCP server to ~/.mcp/skills/ for cursor-agent

install-mcp-rust: NAME = Rust
install-mcp-rust: SRC = target/release/interminai
install-mcp-rust: install-mcp-impl

install-mcp-python: NAME = Python
install-mcp-python: SRC = interminai.py
install-mcp-python: install-mcp-impl

install-mcp-impl: DST = ~/.mcp/skills
install-mcp-impl: install-atomic
	@cp mcp_server.py $(DST)/interminai/
	@chmod +x $(DST)/interminai/mcp_server.py
	@echo ""
	@echo "MCP server installed to $(DST)/interminai/"
	@echo "Run 'make install-cursor' to configure cursor-agent automatically."

install-cursor: install-mcp ## Install MCP server and configure cursor-agent
	@mkdir -p ~/.cursor
	@MCP_JSON=~/.cursor/mcp.json; \
	MCP_PATH="$(HOME)/.mcp/skills/interminai/mcp_server.py"; \
	if [ ! -f "$$MCP_JSON" ]; then \
		echo '{"mcpServers": {"interminai": {"command": "'"$$MCP_PATH"'"}}}' > "$$MCP_JSON"; \
		echo "Created $$MCP_JSON with interminai configured"; \
	elif grep -q '"interminai"' "$$MCP_JSON" 2>/dev/null; then \
		echo "interminai already configured in $$MCP_JSON"; \
	else \
		python3 -c "import json; f='$$MCP_JSON'; d=json.load(open(f)); d.setdefault('mcpServers',{})['interminai']={'command':'$$MCP_PATH'}; json.dump(d,open(f,'w'),indent=2)"; \
		echo "Added interminai to $$MCP_JSON"; \
	fi
	@echo ""
	@echo "Enable with: cursor-agent mcp enable interminai"

install-atomic: build
	@test -n "$(DST)"
	@mkdir -p $(DST)-backup
	@mkdir -p $(DST)/interminai
	@TMPDIR=$$(mktemp -d $(DST)-backup/XXXXXX) && \
		cp -r skills/interminai "$$TMPDIR/interminai" && \
		test -n "$(SRC)" && cp $(SRC) "$$TMPDIR/interminai/scripts/interminai"; \
		mkdir -p $(DST) && \
		mv --exchange "$$TMPDIR/interminai" $(DST) && \
		echo "Old version backed up at $$TMPDIR/interminai"
	@echo "Installed $(NAME) version to $(DST)/interminai"

test: test-rust test-python test-skill

# Emulator selection
test-xterm: INTERMINAI_EMULATOR = xterm
test-custom: INTERMINAI_EMULATOR = custom

# Python uses the same test-xterm/test-custom but with Python binary
test-python: INTERMINAI_BIN = $(PWD)/interminai.py

test-rust test-python: test-xterm test-custom

test-xterm test-custom:
	@echo "Running tests with $(INTERMINAI_EMULATOR) emulator..."
	$(if $(INTERMINAI_BIN),OVERRIDE_CARGO_BIN_EXE_interminai=$(INTERMINAI_BIN)) INTERMINAI_EMULATOR=$(INTERMINAI_EMULATOR) cargo test

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
