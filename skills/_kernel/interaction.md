---
name: interaction
domain: _kernel
version: 0.1.0
description: Telos 交互协议与确认流程定义
---

# Interaction Protocols

## 确认流程 (Confirmation Flow)

### 创建 Skill 确认

当意图匹配引擎检测到**模糊匹配**的创建 skill 请求时：

1. **系统**: "检测到你想创建新skill，是否确认创建？(y/n)"
2. **用户输入 y/yes/确认/是**: 启动 `/add-skill` 向导
3. **用户输入 n/no/否/取消**: "已取消创建。"

### 保存技能确认

当 `AutoSave` 模式推断完成时：

1. **系统**: 显示推断结果摘要
   ```
   推断完成：**{domain}/{name}**
   摘要：{summary}
   标签：{tags}
   
   输入 `y` 确认保存，`n` 取消。
   ```
2. **用户输入 y**: 执行 `save_or_merge`
3. **用户输入 n**: "已取消。"

### 删除技能确认

当用户触发删除技能时：

1. **系统**: "确认删除 {domain}/{name}？此操作不可恢复。(y / n)"
2. **用户输入 y**: 执行 `delete_skill`
3. **用户输入 n**: "已取消。"

### Mini-蒸馏确认

当用户发送 `@skill + 纠正信号` 时：

1. **系统**: 显示生成的更新内容预览
   ```
   已生成更新内容：
   
   ```
   {new_content}
   ```
   
   输入 `y` 应用到 @{skill_name}，`n` 取消。
   ```
2. **用户输入 y**: 执行 `update_content`
3. **用户输入 n**: "已取消。"

### 项目蒸馏确认

当 Ring-3 蒸馏完成时：

1. **系统**: 显示蒸馏结果摘要
   ```
   蒸馏完成（项目 {project_id}）
   将保存到以下 Skill 的 memory/ 目录：{skills}
   
   输入 `y` 保存，`n` 取消。
   ```
2. **用户输入 y**: 保存到各 skill 的 memory/distilled.md
3. **用户输入 n**: "已取消。"

## 向导流程 (Wizard Flow)

### /add-skill 向导

用于创建新的协议 Skill：

**Step 1: 询问名称**
- **系统**: "Skill 名称（英文 kebab-case，如 code-review）？"
- **输入验证**: 不能为空，转换为小写，空格替换为连字符
- **用户输入空**: "名称不能为空，请重新输入："

**Step 2: 询问描述**
- **系统**: "一句话描述（中文）？"
- **输入**: 任意非空字符串

**Step 3: 询问触发词**
- **系统**: "激活触发词，逗号分隔（可直接 Enter 跳过）？"
- **输入**: 逗号分隔的短语列表，可为空

**Step 4: 创建文件**
创建以下文件结构：
```
skills/{name}/
  skill.yaml
  domain.md
  artifacts.md
  memory/failure-patterns.md
```

### 新增暗号向导

**Step 1: 询问触发词**
- **系统**: "新增暗号 → 触发词（说出这个词时触发，如「码住」）？"
- **输入验证**: 不能为空

**Step 2: 选择动作**
- **系统**: 
  ```
  对应动作（输入数字）：
  1. 保存技能
  2. 查看技能库
  3. 启动飞轮/蒸馏
  4. 加载领域技能
  5. 查看暗号列表
  ```
- **输入**: 1-5 或对应中文名称

## 输入模式提示

各模式的输入框提示文本：

| 模式 | 提示 |
|------|------|
| Chat | 输入消息 ↑↓ 历史 Ctrl+Q 退出 |
| AutoSave | 推断中... |
| ConfirmSave | y 确认保存 n 取消 |
| ConfirmDelete | y 确认删除 n 取消 |
| ConfirmCreateSkill | y 确认创建 n 取消 |
| Distilling | 蒸馏中... |
| ConfirmDistill | y 应用更新 n 取消 |
| DistillProject | Ring 3 蒸馏中... |
| ConfirmDistillProject | y 保存蒸馏结果 n 取消 |
| GroupChatAnalyzing | Ring 1 分析中... |
| GroupChatSynthesizing | Ring 1 综合中... |
| CreateSkill(AskName) | Skill 名称 |
| CreateSkill(AskDescription) | 一句话描述 |
| CreateSkill(AskTriggers) | 触发词，逗号分隔 |
| AddCodeword(AskTrigger) | 触发词 |
| AddCodeword(AskAction) | 动作编号 (1-5) |
| AddCodeword(RemoveTrigger) | 输入要删除的触发词 |

## 快捷键定义

| 快捷键 | 动作 |
|--------|------|
| Ctrl+Q | 退出程序 |
| Ctrl+L | 清空对话 |
| Esc | 取消当前操作/流 |
| Ctrl+C | 取消流（仅在流式输出时） |
| ↑ | 上一条输入历史 |
| ↓ | 下一条输入历史 |
| PageUp | 消息区域向上滚动 10 行 |
| PageDown | 消息区域向下滚动 10 行 |
| Home | 光标移至行首 |
| End | 光标移至行尾 |
| Enter | 提交输入 |
| Backspace | 删除光标前一个字符 |
| Delete | 删除光标处字符 |
| ← | 光标左移一个字符 |
| → | 光标右移一个字符 |