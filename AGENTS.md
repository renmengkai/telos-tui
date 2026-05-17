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
- **`src/codeword.rs`**: Intent detection engine. Loads rules from `skills/_kernel/intent-matching.md` at startup; falls back to built-in defaults if missing. Supports exact match (immediate) and fuzzy match (confirmation prompt).

## Kernel Skills (声明式配置)

系统内核行为通过 Markdown 文件声明，修改后重启生效，无需重新编译：

- **`skills/_kernel/intent-matching.md`** — 意图识别规则
  - 定义精确匹配短语（直接触发）和模糊匹配短语（询问确认）
  - 包含软提示信号和纠正信号
  - 每个动作可配置确认提示和取消提示

- **`skills/_kernel/interaction.md`** — 交互协议
  - 各模式的确认流程定义
  - 向导步骤和提示文本
  - 快捷键映射

- **`skills/_kernel/system.md`** — 系统核心行为
  - 加载顺序、降级策略、版本兼容性

### 配置优先级

1. 运行时环境变量（`TELOS_*`）
2. `telos.toml` / `~/.config/telos/config.toml`
3. **内核 skill 定义**（`skills/_kernel/*.md`）← 新增
4. 编译时内置默认值

### 降级策略

内核文件缺失或解析失败时，系统自动使用内置默认值，并在 stderr 输出警告。

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
- Intent matching is **declarative** via `skills/_kernel/intent-matching.md`; falls back to built-in defaults
- No database; all persistence is filesystem-based (Markdown, JSON, JSONL)
- `Cargo.lock` is **gitignored** — only `Cargo.toml` is tracked
