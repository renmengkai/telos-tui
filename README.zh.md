# Telos

[![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

> 基于 Rust 的 LLM 辅助工作终端界面，内置技能进化系统。
> 
> A Rust TUI for LLM-assisted work with a skill/evolution system.

[English Documentation](README.md)

---

## Telos 是什么？

Telos 是一个终端界面，将你的 LLM 变成一位**懂上下文的队友**。它超越了简单聊天，引入了：

- **技能系统** — 将程序性知识（如何做某事）保存为可复用的技能，根据上下文自动加载
- **群聊分析** — 顺序（`@skill1 @skill2`）或并行（`--parallel`）调用多个技能进行多角度分析
- **蒸馏飞轮** — 自动将项目日志蒸馏为技能记忆，持续进化
- **项目连续性** — 跨会话追踪项目状态、假设和 Ring 进度

把它想象成一个越用越聪明的外脑。

## 功能特性

| 特性 | 说明 |
|------|------|
| 🧠 **技能自动匹配** | 根据消息内容相关性静默加载技能 |
| 👥 **技能群聊** | 多技能分析后综合输出统一视图 |
| 🔄 **迷你蒸馏** | `@skill + 纠正` 即时更新技能内容 |
| 🏭 **Ring-3 全蒸馏** | 将完整项目日志蒸馏为技能记忆 |
| 📊 **项目追踪** | 自动生成项目 ID，持久化状态 |
| 🎨 **简洁 TUI** | 基于 Ratatui 的界面，含侧边栏、消息历史和输入框 |

## 快速开始

### 前置要求

- [Rust](https://rustup.rs/) (2021 edition)
- LLM API 密钥（Anthropic 或 OpenAI 兼容）

### 安装运行

```bash
# 克隆
git clone https://github.com/yourusername/telos.git
cd telos

# 配置
cp telos.toml.example telos.toml
# 编辑 telos.toml 填入你的 API 密钥

# 运行
cargo run
```

### 配置

在项目根目录创建 `telos.toml` 或在 `~/.config/telos/config.toml`：

```toml
[llm]
provider = "anthropic"      # 或 "openai"
api_key = "sk-..."
model = "claude-opus-4-7"   # 或 "gpt-4o"
# base_url = "..."          # 可选代理地址

[skills]
# dir = "./skills"          # 默认
```

API 密钥优先级：`TELOS_API_KEY` 环境变量 > `ANTHROPIC_API_KEY`/`OPENAI_API_KEY` 环境变量 > 配置文件。

## 使用指南

### 核心快捷键

| 按键 | 功能 |
|------|------|
| `Ctrl+Q` | 退出 |
| `Ctrl+L` | 清空对话 |
| `Esc` / `Ctrl+C` | 取消流式输出 / 当前模式 |
| `↑` / `↓` | 输入历史 |
| `PgUp` / `PgDn` | 滚动消息 |

### 暗号触发

Telos 支持用自然中文短语作为快捷指令：

| 暗号 | 动作 |
|------|------|
| 「记下来」 | 将当前对话保存为技能 |
| 「看看家底」 | 列出所有技能 |
| 「启动飞轮」 | 将项目日志蒸馏为技能记忆 |
| 「加载技能」 | 加载领域相关技能 |
| 「删除技能」 | 删除指定技能 |

### 技能引用

- `@skill_name` — 加载技能作为上下文
- `@skill1 @skill2 消息` — 群聊（顺序分析）
- `@skill1 @skill2 消息 --parallel` — 群聊（并行分析）
- `@all` — 激活所有技能

### 技能类型

**知识技能**：`skills/_index.md` 中列出的单文件 Markdown

**协议技能**：包含以下结构的目录：
```
skills/{name}/
  skill.yaml       # 元数据、触发词、记忆引用
  domain.md        # 分析协议
  artifacts.md     # 输出规格
  memory/          # 蒸馏成果与失败模式
```

## 架构

```
src/
├── main.rs          # 终端设置、事件循环（约60fps）
├── app.rs           # 中央状态机、输入处理、LLM 流式输出
├── ui/mod.rs        # Ratatui 渲染（状态栏、侧边栏、消息、输入框）
├── config.rs        # TOML + 环境变量配置加载
├── session.rs       # 聊天历史，支持 <think> 标签提取
├── llm/             # LlmClient 特性（Anthropic & OpenAI 实现）
├── skill/           # 技能存储、扫描、自动匹配
├── project.rs       # 项目状态与事件日志
└── codeword.rs      # 中文触发词动作系统
```

## 数据目录

```
project_root/
├── skills/                  # 技能文件和协议目录
├── projects/{id}/state.json # 项目状态（技能、Ring、假设）
├── episodic-logs/{id}.jsonl # 交互日志
├── .distill-needed/         # 待蒸馏标记
└── ~/.config/telos/
    ├── codewords.json       # 自定义暗号
    └── usage-stats.json     # 使用统计
```

所有数据持久化基于文件系统（Markdown、JSON、JSONL）。无需数据库。

## 开发

```bash
# 调试模式运行
cargo run

# 发布构建
cargo build --release

# 注意：Cargo.lock 已加入 gitignore，仅跟踪 Cargo.toml
```

**重要提示**：应用使用原始终端模式 + 备用屏幕。若崩溃可能导致终端状态异常 — 运行 `reset` 恢复。

## 为什么是 Telos？

大多数 LLM 工具是**无状态聊天**。Telos 是**状态化进化**：

- 每次对话都可以变成可复用的技能
- 技能通过纠正和蒸馏持续改进
- 项目跨会话保持连续性
- 你的"工作方式"被编码并可分享

> *"让导出的技能成为标本，让 Telos 内的技能保持活体。"*

## 许可证

MIT
