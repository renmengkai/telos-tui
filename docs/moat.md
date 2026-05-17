# Telos 护城河建设蓝图

> **核心结论：技术实现成本趋近于零，真正的壁垒来自用户累积的时间密度与关系网络。**

---

## 一、现状诊断：为什么代码不是壁垒

### 1.1 当前技术栈的可复制性

| 组件 | 实现复杂度 | 被复制所需时间 | 壁垒价值 |
|------|-----------|---------------|----------|
| Rust TUI (ratatui + crossterm) | 中 | 1-2 周 | **零** |
| Markdown 文件存储 | 低 | 几天 | **零** |
| LLM API 调用层 | 低 | 几天 | **零** |
| 群聊顺序/并行调度 | 中 | 2-3 周 | **低** |
| 暗号系统 | 低 | 1 周 | **低** |

**一个熟练的 Rust 开发者 + LLM 应用经验，2-4 周即可复刻核心功能。**

### 1.2 开源生态的启示

- **Obsidian**：代码本身无壁垒，壁垒是 1000+ 社区插件 + 用户多年积累的笔记图谱
- **VS Code**：编辑器市场充分竞争，壁垒是 Extension Marketplace 生态
- **Zsh**：Shell 本身可被复制，壁垒是 Oh-My-Zsh 的插件生态和用户配置习惯

**模式：开放格式 + 积累生态 + 用户时间投入 = 不可复制的护城河**

---

## 二、三层护城河模型

```
┌─────────────────────────────────────────────────────────────┐
│  Layer 3: 生态网络效应（最高壁垒，6-12 个月）                │
│  ├── Skill 市场与创作者经济                                  │
│  ├── 组织/团队工作流嵌入                                     │
│  └── 行业标准化话语权                                        │
├─────────────────────────────────────────────────────────────┤
│  Layer 2: 个人数据复利（中等壁垒，1-3 个月）                 │
│  ├── 用户技能图谱的时间密度                                  │
│  ├── 暗号与工作流肌肉记忆                                    │
│  └── 项目上下文的连续性                                      │
├─────────────────────────────────────────────────────────────┤
│  Layer 1: 协议复杂度（低壁垒，可被模仿）                     │
│  ├── Ring 1/2/3 飞轮机制                                   │
│  ├── Mini-蒸馏 vs 全蒸馏双轨                                 │
│  └── 行业包自动匹配                                          │
└─────────────────────────────────────────────────────────────┘
```

> **策略：Layer 1 作为引流钩子（开源吸引用户），Layer 2 锁定个人用户，Layer 3 形成生态垄断。**

---

## 三、Layer 1: 协议复杂度 — 引流与体验差异化

### 3.1 当前实现（已完成）

| 机制 | 文件位置 | 说明 |
|------|---------|------|
| 群聊顺序分析 | `app.rs:1004` | 多 Skill 依次分析，输出综合视图 |
| 群聊并行分析 | `app.rs:1044` | `--parallel` 并发调用多个 LLM |
| Mini-蒸馏 | `app.rs:704` | `@skill + 纠正信号`，即时更新 |
| Ring-3 全蒸馏 | `app.rs:1194` | 基于项目日志批量蒸馏到 memory |
| 行业包匹配 | `skill/store.rs:122` | 根据材料关键词自动加载行业上下文 |

### 3.2 强化策略：让"好用"成为壁垒

**核心逻辑：** 机制本身可被复制，但**调优到好用**需要大量真实用户数据。

**执行动作：**

1. **Prompt 调优数据积累**
   - 记录每次群聊的输入/输出/用户满意度（隐式：用户是否接受结果、是否纠正）
   - 用这些数据持续优化 `build_gc_skill_prompt` 和 `build_synthesis_prompt`
   - **结果：** 竞争对手没有 6 个月的 Prompt 调优数据，体验始终差一档

2. **行业包库建设**
   - 官方维护 20-30 个核心行业包（ERP、SaaS、电商、金融等）
   - 积累每个行业的"常见需求模式"和"分析陷阱"
   - **结果：** 用户发现"Telos 懂我的行业"，迁移时失去行业上下文

3. **失败模式进化**
   - `memory/failure-patterns.md` 每次蒸馏自动更新
   - 跨用户的匿名化失败模式聚合（未来）
   - **结果：** 系统越用越聪明，新系统永远是"菜鸟"

---

## 四、Layer 2: 个人数据复利 — 锁定单个用户

### 4.1 核心机制：时间密度

**用户在 Telos 中投入的每一分钟都在增加迁移成本。**

当前已积累的数据资产：

| 资产 | 位置 | 价值 |
|------|------|------|
| 技能进化历史 | `skills/{domain}/{name}.md` frontmatter | 知道每个技能被精炼了多少轮 |
| 失败模式库 | `skills/{skill}/memory/failure-patterns.md` | 该用户已知的常见错误 |
| 蒸馏历史 | `skills/{skill}/memory/distilled.md` | 从哪些项目学到了什么 |
| 项目状态 | `projects/{id}/state.json` | 技能组合、假设数量、Ring 进度 |
| 事件日志 | `episodic-logs/{id}.jsonl` | 每次交互的完整上下文 |
| 暗号习惯 | `~/.config/telos/codewords.json` | 自定义触发词和肌肉记忆 |
| 输入历史 | `app.rs:119` | 常用查询模式 |

### 4.2 执行策略：让资产显性化

**动作 1: 启动页资产仪表盘**

```rust
// 在 App::new() 中增加
let stats = format!(
    "Telos v{} | {} 天 | {} 技能 | {} 项目 | {} 反馈 | {} 蒸馏",
    env!("CARGO_PKG_VERSION"),
    days_since_first_use(),      // 从最早的 skill created 日期计算
    available_skills.len(),
    project_count(),
    total_quality_events(),      // episodic-logs 中 quality_score >= 0.6
    total_distillations(),       // memory/distilled.md 条目数
);
```

**效果：** 用户每天打开看到数字增长，形成"资产感"。

**动作 2: 导出时的"上下文剥离"提示**

```bash
$ telos skill export req-mining
⚠  导出警告
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
此 Skill 当前价值构成：
  协议正文:        ████░░░░░░  35%
  失败模式库:      ██░░░░░░░░  20%
  蒸馏进化历史:    ███░░░░░░░  25%
  项目关联上下文:  ██░░░░░░░░  20%
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
导出 Markdown 仅包含"协议正文"部分。
迁移后将失去 65% 的上下文价值。
确认导出？(y/N)
```

**效果：** 不阻止导出，但让用户意识到"这不是在搬家，是在截肢"。

**动作 3: 技能进化版本可视化**

```markdown
---
name: req-mining
domain: req-mining
created: 2026-05-10
last_updated: 2026-05-17
evolution_count: 12          # 新增
projects_involved: 5         # 新增
distill_sources:             # 新增
  - proj_20260510_01: "增加了优先级排序"
  - proj_20260512_03: "补充了预算约束检查"
---
```

**效果：** 让用户看到"这是我的 req-mining v12，不是网上下载的 v0.1"。

### 4.3 行为锁定：肌肉记忆

**当前已实现的习惯养成：**

| 习惯 | 机制 | 锁定强度 |
|------|------|---------|
| 说暗号 | `codeword.rs:191` 检测触发词 | 高 — 日常用语变成命令 |
| 自动保存 | `app.rs:312` 软提示 | 中 — 培养"值得记录"的意识 |
| @skill 群聊 | `app.rs:551` 解析 @ 引用 | 高 — 思考模式改变 |
| Ctrl+L 清屏 | `app.rs:415` 快捷键 | 低 — 但跨工具不一致会烦躁 |
| 项目自动恢复 | `app.rs:150` 上次项目 | 中 — 连续性依赖 |

**强化动作：**

1. **暗号使用统计**
   ```bash
   $ telos stats
   本月使用记录：
     「记下来」     23 次  → 保存了 18 个技能
     「看看家底」    5 次  → 最常回顾: cooking/braising
     「启动飞轮」    3 次  → 蒸馏了 req-mining, code-review
   
   最常协同的 Skill 组合：
     @req-mining + @code-review（5 次）
   
   自定义暗号：
     「复盘」→ 启动飞轮
   ```

2. **渐进式暗号教学**
   - 第 1-3 次会话：基础暗号（记下来、看看家底）
   - 第 4-7 次：中级（启动飞轮、加载领域）
   - 第 8 次+：高级（自定义暗号、群聊并行）
   - **效果：** 用户学习成本分散，每掌握一个暗号就增加一分依赖

---

## 五、Layer 3: 生态网络效应 — 最高壁垒

### 5.1 Skill 市场：内容平台模式

**核心逻辑：** 当社区有 1000 个高质量 Skill 时，新用户选择 Telos 不是因为功能，而是因为**这里有他需要的技能**。

**这不是"功能竞争"，这是"内容平台竞争"。**

**执行路径：**

#### Phase 1: 标准制定（立即开始）

1. **开源 Skill 包格式**
   ```bash
   # 创建 telos-skill-spec 仓库
   github.com/telos-org/skill-spec
   ```

2. **发布 JSON Schema**
   ```json
   {
     "$schema": "https://telos.dev/skill-schema.json",
     "type": "object",
     "required": ["name", "version", "description"],
     "properties": {
       "name": { "type": "string" },
       "version": { "type": "string" },
       "description": { "type": "string" },
       "ring1_protocol": { "type": "string", "description": "Path to domain.md" },
       "triggers": { "type": "array", "items": { "type": "string" } },
       "memory": { "type": "array", "items": { "type": "string" } }
     }
   }
   ```

3. **提供官方示例 Skill**
   - `req-mining`: 需求挖掘与落地
   - `code-review`: 代码审查协议
   - `architecture-review`: 架构评审
   - 每个都包含完整的 `domain.md` + `artifacts.md` + `memory/`

**效果：** 让竞争对手来兼容你的标准，而不是你去兼容别人的。

#### Phase 2: Skill 注册表（3 个月内）

**不需要自建服务器 — 用 GitHub 做后端。**

```bash
# 搜索社区 Skill
$ telos skill search "code review"
1. code-review     — 代码审查协议          ★ 234  [官方]
2. rust-review     — Rust 专项代码审查       ★ 89   [社区]
3. security-audit  — 安全审计检查清单        ★ 156  [社区]

# 安装
$ telos skill install code-review
✓ 已安装 code-review → skills/code-review/
  domain.md
  artifacts.md
  memory/failure-patterns.md

# 发布（需要 GitHub 账号）
$ telos skill publish ./skills/my-custom-skill
✓ 已提交 PR 到 telos-skill-registry
  https://github.com/telos-org/skill-registry/pull/123
```

**技术实现：**
- 注册表 = GitHub 仓库 `telos-org/skill-registry`
- `index.json` 记录所有已审核 Skill 的元数据
- `install` = 从 GitHub raw 下载 zip/tar + 解压到 `skills/`
- `publish` = Fork 注册表 + 添加条目 + 提交 PR

**审核机制：**
- 官方维护核心 Skill（高质量标准）
- 社区 Skill 通过 PR 审核（基础质量标准）
- 未审核 Skill 可通过直接 URL 安装（风险自负）

#### Phase 3: 创作者经济（6 个月内）

**激励高质量 Skill 创作：**

```yaml
# skill.yaml 增加创作者信息
author: "alice"
author_id: "github:alice"
license: "CC-BY-SA"
donation: "https://github.com/sponsors/alice"
```

**展示机制：**
- 安装时显示作者信息 + 捐赠链接
- Skill 列表页展示"本月最佳创作者"
- 高星 Skill 作者获得官方推荐位

**网络效应飞轮：**
```
创作者发布 Skill → 用户安装使用 → 用户反馈/纠正 → Skill 进化
       ↑___________________________________________|
                (创作者获得声誉 + 捐赠)
```

### 5.2 组织/团队嵌入（B2B 壁垒）

**当团队使用 Telos 共享 Skill 和项目状态时，迁移成本从个人级跃升到组织级。**

**数据模型：**

```
team-config.toml
├── shared_skills/          # 团队共享的 skills（Git 子模块）
│   ├── company-standards/
│   ├── internal-review/
│   └── onboarding-guide/
├── projects/               # 团队项目（共享 state.json）
│   ├── proj_20260517_01/
│   └── proj_20260518_02/
└── team-memory/            # 团队级 failure-patterns
    └── distilled.md
```

**使用场景：**

1. **新员工入职**
   ```bash
   $ telos team join acme-corp
   ✓ 已同步团队 Skill：company-standards, internal-review
   ✓ 已恢复最近项目：proj_20260517_01
   ```

2. **项目模板**
   ```bash
   $ telos project new --template "sprint-planning"
   ✓ 已创建项目，预加载 Skill：
     @req-mining @estimation @risk-assessment
   ```

3. **知识传承**
   - 老员工离职 → 他的 `skills/` + `memory/` 已同步到团队仓库
   - 新员工继承 → 直接使用进化后的 Skill

**迁移成本：**
- 个人版：1 人决策，成本 = 个人时间投入
- 团队版：需全团队同意，成本 = 组织知识资产的迁移

### 5.3 行业标准化话语权

**终极目标：让 `skill.yaml` 成为 Agent 技能的事实标准。**

**执行路径：**

1. **文档化标准**（已完成 80%）
   - `skill.yaml` 字段定义
   - `domain.md` 协议格式
   - `artifacts.md` 产物规格
   - `memory/` 目录结构

2. **兼容性声明**
   - "Compatible with Telos Skill Spec v1.0"
   - 提供验证工具 `telos skill validate ./my-skill`

3. **生态合作**
   - 向 LangChain、AutoGPT 等框架提议兼容
   - 在 AI 社区推广标准
   - 举办 Skill 创作比赛

**效果：** 成为"Agent 技能的 Markdown" — 不是最复杂的，但是最被广泛采用的。

---

## 六、执行路线图

### 第 1 个月：数据资产显性化

| 周 | 任务 | 交付物 | 负责人 |
|----|------|--------|--------|
| 1 | 启动页资产仪表盘 | `app.rs` 增加 stats 展示 | Dev |
| 1 | 导出剥离提示 | `skill/store.rs` 导出函数 | Dev |
| 2 | 技能进化 frontmatter | `store.rs:save_or_merge()` 增加字段 | Dev |
| 2 | 使用统计命令 | `telos stats` CLI 命令 | Dev |
| 3 | 暗号使用统计 | `codeword.rs` 增加使用计数 | Dev |
| 4 | 文档发布 | 本文件 + 用户可见的"你的资产"页面 | PM |

### 第 2-3 个月：Skill 标准与注册表

| 周 | 任务 | 交付物 | 负责人 |
|----|------|--------|--------|
| 5 | Skill 格式规范文档 | `telos-skill-spec` GitHub 仓库 | Tech Lead |
| 5 | JSON Schema 发布 | `skill-schema.json` | Tech Lead |
| 6 | 官方示例 Skill 包 | 5 个高质量示例 Skill | Content |
| 7 | 注册表仓库搭建 | `telos-org/skill-registry` | Dev |
| 7 | `telos skill search` 命令 | CLI 搜索功能 | Dev |
| 8 | `telos skill install` 命令 | CLI 安装功能 | Dev |
| 9 | `telos skill publish` 命令 | CLI 发布功能 | Dev |
| 10-12 | 社区推广 | 博客、Twitter、Discord | Marketing |

### 第 4-6 个月：生态建设

| 周 | 任务 | 交付物 | 负责人 |
|----|------|--------|--------|
| 13 | 创作者激励设计 | 捐赠链接 + 推荐机制 | Product |
| 14 | 团队版 MVP | `telos team` 命令集 | Dev |
| 16 | 项目模板 | `--template` 参数支持 | Dev |
| 18 | 社区比赛 | "最佳 Skill" 评选 | Community |
| 20 | 行业标准化提案 | 向主流框架推广 | BD |
| 24 | 团队版正式发布 | B2B 定价 + 销售 | Business |

---

## 七、关键指标（North Star Metrics）

### Layer 2 指标（个人锁定）

| 指标 | 定义 | 目标（6 个月） |
|------|------|---------------|
| 技能时间密度 | 平均每个 skill 的 `conversation_count` | > 3 |
| 项目连续性 | 用户返回上次项目的频率 | > 80% |
| 暗号依赖度 | 使用暗号而非完整命令的比例 | > 60% |
| 蒸馏参与度 | 触发蒸馏的用户占比 | > 30% |

### Layer 3 指标（生态网络）

| 指标 | 定义 | 目标（6 个月） |
|------|------|---------------|
| 社区 Skill 数 | 注册表中的第三方 Skill | > 100 |
| Skill 安装量 | 累计安装次数 | > 5000 |
| 活跃创作者 | 发布过 Skill 的唯一用户 | > 20 |
| 团队版客户 | 付费团队数 | > 5 |

---

## 八、风险与应对

| 风险 | 可能性 | 影响 | 应对 |
|------|--------|------|------|
| 大厂推出类似产品 | 高 | 高 | **速度优先** — 6 个月内建立生态基础，大厂决策慢 |
| 用户不认可"资产"概念 | 中 | 高 | **渐进教育** — 从 stats 展示开始，逐步强化感知 |
| Skill 质量参差不齐 | 高 | 中 | **分级审核** — 官方/社区/未审核三级 |
| 社区创作者流失 | 中 | 中 | **激励设计** — 声誉 + 捐赠 + 官方推荐 |
| 技术债拖累迭代 | 中 | 高 | **保持简洁** — Markdown + Git 足够，不引入复杂数据库 |

---

## 九、核心原则

1. **开放格式是优势，不是劣势**
   - Markdown 可被任何编辑器打开 → 降低用户心理门槛
   - 但关系网络只在 Telos 中完整 → 形成"开放性锁定"

2. **时间是最深的护城河**
   - 每个用户投入的 1 小时都在增加迁移成本
   - 要设计机制让用户"感知"到时间的积累

3. **生态竞争大于功能竞争**
   - 不要试图在功能上击败所有对手
   - 要在 Skill 数量和质量上建立不可替代性

4. **速度 > 完美**
   - 先发布 Skill 注册表 MVP，再迭代完善
   - 先积累 100 个 Skill，再优化推荐算法

---

## 十、一句话总结

> **让导出的 Skill 成为"标本"，让留在 Telos 里的 Skill 保持"活体"。**
> 
> **不锁数据，但让数据在系统内持续进化；不防导出，但让导出后的价值自然衰减。**
> 
> **最终壁垒：用户说"这不是一个工具，这是我的外脑"。**

---

*文档版本: v1.0*
*创建日期: 2026-05-17*
*下次评审: 2026-06-17*
