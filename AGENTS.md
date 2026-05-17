# AGENTS.md

## Project

- **Name**: `telos` — Rust TUI for LLM-assisted work with a skill/evolution system
- **Stack**: Rust 2021, ratatui, crossterm, tokio, reqwest
- **Binary**: `cargo run` produces `telos` binary; entrypoint `src/main.rs`
- **No tests** currently exist; no CI configured

## Running

```bash
# Build and run (requires LLM API key — see Config)
cargo run

# Release build
cargo build --release
```

## Config

App reads `telos.toml` (project root) or `~/.config/telos/config.toml`:

```toml
[llm]
provider = "anthropic"   # or "openai"
api_key = "sk-..."
model = "claude-opus-4-7" # or "gpt-4o"
# base_url = "..."       # optional proxy/compatible endpoint

[skills]
# dir = "./skills"       # default
```

API key priority: `TELOS_API_KEY` env > provider-specific env (`ANTHROPIC_API_KEY` / `OPENAI_API_KEY`) > config file.

## Architecture

- **`src/main.rs`**: Terminal setup (raw mode + alternate screen), event loop at ~60fps, spawns blocking crossterm input thread
- **`src/app.rs`**: Central state machine (`AppMode`), handles keyboard input, LLM streaming, skill operations
- **`src/ui/mod.rs`**: Ratatui rendering — status bar, sidebar (skills list), messages, input box
- **`src/config.rs`**: TOML + env config loader
- **`src/session.rs`**: Chat message history with `<think>` tag extraction
- **`src/llm/`**: Trait `LlmClient` with Anthropic and OpenAI implementations; streaming via mpsc
- **`src/skill/`**: Skill storage, scanning, matching
- **`src/project.rs`**: Project state (`proj_YYYYMMDD_NN`), episodic logging
- **`src/codeword.rs`**: Chinese trigger-word action system (暗号)

## Skills System

- **Knowledge skills**: Listed in `skills/_index.md`, single markdown files
- **Protocol skills**: Directories under `skills/` containing `skill.yaml` + `domain.md` + `artifacts.md` + `memory/`
- **Auto-match**: Silent skill loading based on message content via `SkillMatcher`
- **Explicit load**: `@skill_name` in chat input
- **Group chat**: `@skill1 @skill2 message` (sequential); append `--parallel` for concurrent analysis

## Key Behaviors

- **Ctrl+Q**: quit; **Ctrl+L**: clear chat; **Esc** / **Ctrl+C**: cancel streaming or current mode
- **Soft prompt**: After user signals value (e.g. "学到了"), app suggests saving with "记下来"
- **Distillation**: "启动飞轮" triggers Ring-3 project log distillation into skill `memory/`
- **Mini-distill**: `@skill + correction` (纠正/补充/修正) updates skill content directly

## Data Directories

Relative to skills dir parent (`project_root`):

- `skills/` — skill files and protocol directories
- `projects/{id}/state.json` — project state (skills, ring, assumptions)
- `episodic-logs/{id}.jsonl` — interaction logs
- `.distill-needed/` — markers for pending distillations
- `~/.config/telos/codewords.json` — custom codewords
- `~/.config/telos/usage-stats.json` — usage statistics

## Important Notes

- App uses **raw terminal mode + alternate screen**; crashes may leave terminal in bad state (run `reset` if needed)
- Streaming LLM calls run in separate tokio tasks; cancellation uses oneshot channel + task abort
- Chinese UI strings are hardcoded; codeword detection relies on Chinese trigger phrases
- No database; all persistence is filesystem-based (Markdown, JSON, JSONL)
- `Cargo.lock` is **gitignored** — only `Cargo.toml` is tracked
