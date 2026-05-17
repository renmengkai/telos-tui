# Telos

[![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

> A Rust TUI for LLM-assisted work with a skill/evolution system. 
> 
> 基于 Rust 的 LLM 辅助工作终端界面，内置技能进化系统。

[中文文档](README.zh.md)

---

## What is Telos?

Telos is a terminal UI that turns your LLM into a **context-aware teammate**. It goes beyond simple chat by introducing:

- **Skill System** — Save procedural knowledge (how to do things) as reusable skills that auto-load based on context
- **Group Chat** — Invoke multiple skills sequentially (`@skill1 @skill2`) or in parallel (`--parallel`) for multi-perspective analysis
- **Distillation Flywheel** — Automatically evolve skills by distilling project logs into skill memory
- **Project Continuity** — Tracks project state, assumptions, and ring progress across sessions

Think of it as a second brain that gets smarter the more you use it.

## Features

| Feature | Description |
|---------|-------------|
| 🧠 **Auto-Matching Skills** | Skills silently load based on message content relevance |
| 👥 **Skill Group Chat** | Multiple skills analyze, then synthesize a unified view |
| 🔄 **Mini-Distill** | `@skill + correction` instantly updates skill content |
| 🏭 **Ring-3 Distillation** | Full project log distillation into skill memory |
| 📊 **Project Tracking** | Auto-generated project IDs with state persistence |
| 🎨 **Clean TUI** | Ratatui-based interface with sidebar, message history, and input |

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (2021 edition)
- LLM API key (Anthropic or OpenAI compatible)

### Installation

```bash
# Clone
git clone https://github.com/yourusername/telos.git
cd telos

# Configure
cp telos.toml.example telos.toml
# Edit telos.toml with your API key

# Run
cargo run
```

### Configuration

Create `telos.toml` in project root or `~/.config/telos/config.toml`:

```toml
[llm]
provider = "anthropic"      # or "openai"
api_key = "sk-..."
model = "claude-opus-4-7"   # or "gpt-4o"
# base_url = "..."          # optional proxy

[skills]
# dir = "./skills"          # default
```

API key priority: `TELOS_API_KEY` env > `ANTHROPIC_API_KEY`/`OPENAI_API_KEY` env > config file.

## Usage

### Core Shortcuts

| Key | Action |
|-----|--------|
| `Ctrl+Q` | Quit |
| `Ctrl+L` | Clear chat |
| `Esc` / `Ctrl+C` | Cancel streaming / current mode |
| `↑` / `↓` | Input history |
| `PgUp` / `PgDn` | Scroll messages |

### Codewords

Telos recognizes natural Chinese phrases as shortcuts for common actions:

| Phrase | Action |
|--------|--------|
| "记下来" | Save current conversation as a skill |
| "看看家底" | List all skills |
| "启动飞轮" | Distill project logs into skill memory |
| "加载技能" | Load domain-specific skills |
| "删除技能" | Delete a skill |

### Skill References

- `@skill_name` — Load a skill as context
- `@skill1 @skill2 message` — Group chat (sequential analysis)
- `@skill1 @skill2 message --parallel` — Group chat (parallel analysis)
- `@all` — Activate all skills

### Skill Types

**Knowledge Skills**: Single markdown files listed in `skills/_index.md`

**Protocol Skills**: Directories with:
```
skills/{name}/
  skill.yaml       # metadata, triggers, memory refs
  domain.md        # analysis protocol
  artifacts.md     # output specifications
  memory/          # distilled learnings & failure patterns
```

## Architecture

```
src/
├── main.rs          # Terminal setup, event loop (~60fps)
├── app.rs           # Central state machine, input handling, LLM streaming
├── ui/mod.rs        # Ratatui rendering (status bar, sidebar, messages, input)
├── config.rs        # TOML + env configuration loader
├── session.rs       # Chat history with <think> tag extraction
├── llm/             # LlmClient trait (Anthropic & OpenAI implementations)
├── skill/           # Skill storage, scanning, auto-matching
├── project.rs       # Project state & episodic logging
└── codeword.rs      # Chinese trigger-word action system
```

## Data Layout

```
project_root/
├── skills/                  # Skill files and protocol directories
├── projects/{id}/state.json # Project state (skills, ring, assumptions)
├── episodic-logs/{id}.jsonl # Interaction logs
├── .distill-needed/         # Pending distillation markers
└── ~/.config/telos/
    ├── codewords.json       # Custom codewords
    └── usage-stats.json     # Usage statistics
```

All persistence is filesystem-based (Markdown, JSON, JSONL). No database required.

## Development

```bash
# Run in debug mode
cargo run

# Release build
cargo build --release

# Note: Cargo.lock is gitignored — only Cargo.toml is tracked
```

**Important**: The app uses raw terminal mode + alternate screen. If it crashes, your terminal may be left in a bad state — run `reset` to recover.

## Why Telos?

Most LLM tools are **stateless chat**. Telos is **stateful evolution**:

- Every conversation can become a reusable skill
- Skills improve through corrections and distillation
- Projects maintain continuity across sessions
- Your "way of working" becomes encoded and shareable

> *"Let exported skills be specimens; let skills inside Telos stay alive."*

## License

MIT
