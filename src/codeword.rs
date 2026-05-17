use std::collections::HashMap;
use std::fs;
use std::path::Path;

// ── Actions ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Action {
    SaveSkill,
    ListSkills,
    ListCodewords,
    StartFlywheel,
    LoadDomain,
    DeleteSkill,
    ShowSkill,
    CreateSkill,
    AddCodeword,
    EditCodeword,
    RemoveCodeword,
    Stats,
    ExportSkill,
}

// ── Intent Rule ───────────────────────────────────────────────────────────────

/// A single intent matching rule loaded from kernel config.
#[derive(Debug, Clone)]
pub struct IntentRule {
    pub action: Action,
    pub exact_phrases: Vec<String>,
    pub fuzzy_phrases: Vec<String>,
    /// Composite keywords: all keywords must be present in the message (order-independent).
    pub composite_keywords: Vec<Vec<String>>,
    pub confirmation_prompt: Option<String>,
    pub cancellation_prompt: Option<String>,
}

impl IntentRule {
    pub fn needs_confirmation(&self) -> bool {
        self.confirmation_prompt.is_some()
            && (!self.fuzzy_phrases.is_empty() || !self.composite_keywords.is_empty())
    }
}

// ── Detector ──────────────────────────────────────────────────────────────────

pub struct CodewordDetector {
    /// Merged rules: exact phrases trigger immediately, fuzzy phrases ask for confirmation.
    rules: Vec<IntentRule>,
    /// Passive signals that suggest the user found procedural value.
    soft_signals: Vec<String>,
    /// Signals that indicate the user is correcting a skill.
    correction_signals: Vec<String>,
}

impl CodewordDetector {
    pub fn new() -> Self {
        let mut detector = Self {
            rules: Vec::new(),
            soft_signals: Vec::new(),
            correction_signals: Vec::new(),
        };
        // Try loading from kernel file first, fallback to built-in
        detector.load_kernel_rules_or_default();
        detector
    }

    // ── Kernel loading ────────────────────────────────────────────────────────

    /// Load intent rules from `skills/_kernel/intent-matching.md`.
    /// Falls back to built-in rules if file is missing or malformed.
    fn load_kernel_rules_or_default(&mut self) {
        let kernel_path = Path::new("skills/_kernel/intent-matching.md");
        if !kernel_path.exists() {
            eprintln!("telos: kernel intent-matching.md not found, using built-in rules");
            self.load_builtin_rules();
            return;
        }

        let content = match fs::read_to_string(kernel_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("telos: warning: cannot read intent-matching.md: {e}");
                self.load_builtin_rules();
                return;
            }
        };

        match Self::parse_intent_matching(&content) {
            Ok((rules, soft, correction)) => {
                self.rules = rules;
                self.soft_signals = soft;
                self.correction_signals = correction;
            }
            Err(e) => {
                eprintln!("telos: warning: failed to parse intent-matching.md: {e}");
                self.load_builtin_rules();
            }
        }
    }

    /// Parse the intent-matching.md format.
    fn parse_intent_matching(content: &str) -> Result<(Vec<IntentRule>, Vec<String>, Vec<String>), String> {
        let mut rules = Vec::new();
        let mut soft_signals = Vec::new();
        let mut correction_signals = Vec::new();

        let lines: Vec<&str> = content.lines().collect();
        let mut i = 0;
        let mut current_action: Option<Action> = None;
        let mut current_exact = Vec::new();
        let mut current_fuzzy = Vec::new();
        let mut current_composite: Vec<Vec<String>> = Vec::new();
        let mut current_confirm: Option<String> = None;
        let mut current_cancel: Option<String> = None;

        while i < lines.len() {
            let line = lines[i].trim();

            // Parse soft signals section
            if line == "## 软提示信号 (Soft Signals)" {
                i += 1;
                while i < lines.len() {
                    let l = lines[i].trim();
                    if l.starts_with("## ") { break; }
                    if l.starts_with("- \"") && l.ends_with("\"") {
                        let signal = l[3..l.len()-1].to_string();
                        if !signal.is_empty() {
                            soft_signals.push(signal);
                        }
                    }
                    i += 1;
                }
                continue;
            }

            // Parse correction signals section
            if line == "## 纠正信号 (Correction Signals)" {
                i += 1;
                while i < lines.len() {
                    let l = lines[i].trim();
                    if l.starts_with("## ") { break; }
                    if l.starts_with("- \"") && l.ends_with("\"") {
                        let signal = l[3..l.len()-1].to_string();
                        if !signal.is_empty() {
                            correction_signals.push(signal);
                        }
                    }
                    i += 1;
                }
                continue;
            }

            // Parse action section: "## ActionName — 中文名"
            if line.starts_with("## ") && line.contains(" — ") {
                // Save previous rule if exists
                if let Some(action) = current_action.take() {
                    if !current_exact.is_empty() || !current_fuzzy.is_empty() || !current_composite.is_empty() {
                        rules.push(IntentRule {
                            action,
                            exact_phrases: current_exact.clone(),
                            fuzzy_phrases: current_fuzzy.clone(),
                            composite_keywords: current_composite.clone(),
                            confirmation_prompt: current_confirm.take(),
                            cancellation_prompt: current_cancel.take(),
                        });
                    }
                    current_exact.clear();
                    current_fuzzy.clear();
                    current_composite.clear();
                }

                let action_name = line[3..].split(" — ").next().unwrap_or("").trim();
                current_action = Self::parse_action_name(action_name);
                i += 1;
                continue;
            }

            // Parse "- **精确匹配**:" or "- **模糊匹配**:" header
            if line.starts_with("- **精确匹配**:") {
                i += 1;
                while i < lines.len() {
                    let l = lines[i].trim();
                    if l.starts_with("## ") || l.starts_with("- **") { break; }
                    if l.starts_with("- \"") && l.ends_with("\"") {
                        let phrase = l[3..l.len()-1].to_string();
                        if !phrase.is_empty() {
                            current_exact.push(phrase);
                        }
                    }
                    i += 1;
                }
                continue;
            }

            if line.starts_with("- **模糊匹配**:") {
                i += 1;
                while i < lines.len() {
                    let l = lines[i].trim();
                    if l.starts_with("## ") || l.starts_with("- **") { break; }
                    if l.starts_with("- \"") && l.ends_with("\"") {
                        let phrase = l[3..l.len()-1].to_string();
                        if !phrase.is_empty() {
                            current_fuzzy.push(phrase);
                        }
                    }
                    i += 1;
                }
                continue;
            }

            // Parse composite keywords: - **组合匹配**: ["keyword1", "keyword2"]
            if line.starts_with("- **组合匹配**:") {
                let json_part = line["- **组合匹配**:".len()..].trim();
                if json_part.starts_with('[') && json_part.ends_with(']') {
                    let inner = &json_part[1..json_part.len()-1];
                    let keywords: Vec<String> = inner.split(',')
                        .map(|s| s.trim().trim_matches('"').to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if !keywords.is_empty() {
                        current_composite.push(keywords);
                    }
                }
                i += 1;
                continue;
            }

            // Parse confirmation prompt
            if line.starts_with("- **确认提示**:") {
                let prompt = line["- **确认提示**:".len()..].trim().trim_matches('"').to_string();
                if !prompt.is_empty() {
                    current_confirm = Some(prompt);
                }
                i += 1;
                continue;
            }

            // Parse cancellation prompt
            if line.starts_with("- **取消提示**:") {
                let prompt = line["- **取消提示**:".len()..].trim().trim_matches('"').to_string();
                if !prompt.is_empty() {
                    current_cancel = Some(prompt);
                }
                i += 1;
                continue;
            }

            i += 1;
        }

        // Save last rule
        if let Some(action) = current_action {
            if !current_exact.is_empty() || !current_fuzzy.is_empty() || !current_composite.is_empty() {
                rules.push(IntentRule {
                    action,
                    exact_phrases: current_exact,
                    fuzzy_phrases: current_fuzzy,
                    composite_keywords: current_composite,
                    confirmation_prompt: current_confirm,
                    cancellation_prompt: current_cancel,
                });
            }
        }

        Ok((rules, soft_signals, correction_signals))
    }

    fn parse_action_name(name: &str) -> Option<Action> {
        match name {
            "SaveSkill" | "保存技能" => Some(Action::SaveSkill),
            "ListSkills" | "查看技能库" => Some(Action::ListSkills),
            "ListCodewords" | "查看暗号列表" => Some(Action::ListCodewords),
            "StartFlywheel" | "启动飞轮/蒸馏" => Some(Action::StartFlywheel),
            "LoadDomain" | "加载领域技能" => Some(Action::LoadDomain),
            "DeleteSkill" | "删除技能" => Some(Action::DeleteSkill),
            "ShowSkill" | "查看技能详情" => Some(Action::ShowSkill),
            "CreateSkill" | "创建协议 Skill" => Some(Action::CreateSkill),
            "AddCodeword" | "新增暗号" => Some(Action::AddCodeword),
            "EditCodeword" | "修改暗号" => Some(Action::EditCodeword),
            "RemoveCodeword" | "删除暗号" => Some(Action::RemoveCodeword),
            "Stats" | "查看使用统计" => Some(Action::Stats),
            "ExportSkill" | "导出技能" => Some(Action::ExportSkill),
            _ => None,
        }
    }

    fn load_builtin_rules(&mut self) {
        self.rules = vec![
            IntentRule {
                action: Action::SaveSkill,
                exact_phrases: vec![
                    "记下来".into(), "保存".into(), "存一下".into(), "存起来".into(),
                    "归档".into(), "收入囊中".into(), "存好".into(), "记住这个".into(),
                    "保存技能".into(), "保存这个技巧".into(), "记住这个方法".into(),
                    "整理一下".into(),
                ],
                fuzzy_phrases: Vec::new(),
                composite_keywords: Vec::new(),
                confirmation_prompt: None,
                cancellation_prompt: None,
            },
            IntentRule {
                action: Action::ListSkills,
                exact_phrases: vec![
                    "我都有什么技能".into(), "看看家底".into(), "技能库".into(),
                    "有哪些技能".into(), "翻翻看".into(), "列出技能".into(),
                    "skill list".into(),
                ],
                fuzzy_phrases: Vec::new(),
                composite_keywords: Vec::new(),
                confirmation_prompt: None,
                cancellation_prompt: None,
            },
            IntentRule {
                action: Action::ListCodewords,
                exact_phrases: vec![
                    "暗号大全".into(), "对暗号".into(), "有哪些暗号".into(),
                    "查看暗号".into(), "暗号列表".into(),
                ],
                fuzzy_phrases: Vec::new(),
                composite_keywords: Vec::new(),
                confirmation_prompt: None,
                cancellation_prompt: None,
            },
            IntentRule {
                action: Action::StartFlywheel,
                exact_phrases: vec![
                    "启动飞轮".into(), "开始分析".into(), "蒸馏".into(),
                    "开始蒸馏".into(), "更新skill".into(), "从日志学习".into(),
                    "专家会诊".into(), "复盘".into(), "总结提升".into(),
                ],
                fuzzy_phrases: Vec::new(),
                composite_keywords: Vec::new(),
                confirmation_prompt: None,
                cancellation_prompt: None,
            },
            IntentRule {
                action: Action::LoadDomain,
                exact_phrases: vec![
                    "加载技能".into(), "唤起".into(), "加载领域".into(), "载入技能".into(),
                ],
                fuzzy_phrases: Vec::new(),
                composite_keywords: Vec::new(),
                confirmation_prompt: None,
                cancellation_prompt: None,
            },
            IntentRule {
                action: Action::DeleteSkill,
                exact_phrases: vec![
                    "删除技能".into(), "删掉".into(), "移除技能".into(), "删除这个技能".into(),
                ],
                fuzzy_phrases: Vec::new(),
                composite_keywords: Vec::new(),
                confirmation_prompt: None,
                cancellation_prompt: None,
            },
            IntentRule {
                action: Action::ShowSkill,
                exact_phrases: vec![
                    "查看技能".into(), "看看这个技能".into(), "技能详情".into(),
                    "skill show".into(),
                ],
                fuzzy_phrases: Vec::new(),
                composite_keywords: Vec::new(),
                confirmation_prompt: None,
                cancellation_prompt: None,
            },
            IntentRule {
                action: Action::CreateSkill,
                exact_phrases: vec![
                    "创建新skill".into(), "新建协议技能".into(), "创建协议".into(),
                    "add-skill".into(), "新增协议".into(), "新建skill".into(),
                ],
                fuzzy_phrases: vec![
                    "创建skill".into(), "创建协议skill".into(), "新skill".into(),
                    "add skill".into(), "new skill".into(), "new protocol".into(),
                    "创建新协议".into(), "建一个skill".into(), "给我建个skill".into(),
                    "我要创建skill".into(), "帮我创建skill".into(), "新建一个skill".into(),
                    "添加skill".into(), "增加skill".into(), "来个skill".into(),
                    "整一个skill".into(), "创建一个skill".into(),
                ],
                composite_keywords: vec![
                    vec!["创建".into(), "skill".into()],
                    vec!["新建".into(), "skill".into()],
                    vec!["创建".into(), "协议".into()],
                    vec!["创建".into(), "技能".into()],
                ],
                confirmation_prompt: Some("检测到你想创建新skill，是否确认创建？(y/n)".into()),
                cancellation_prompt: Some("已取消创建。".into()),
            },
            IntentRule {
                action: Action::AddCodeword,
                exact_phrases: vec![
                    "新增暗号".into(), "加个暗号".into(), "添加暗号".into(), "新暗号".into(),
                ],
                fuzzy_phrases: Vec::new(),
                composite_keywords: Vec::new(),
                confirmation_prompt: None,
                cancellation_prompt: None,
            },
            IntentRule {
                action: Action::EditCodeword,
                exact_phrases: vec![
                    "改暗号".into(), "修改暗号".into(), "编辑暗号".into(),
                ],
                fuzzy_phrases: Vec::new(),
                composite_keywords: Vec::new(),
                confirmation_prompt: None,
                cancellation_prompt: None,
            },
            IntentRule {
                action: Action::RemoveCodeword,
                exact_phrases: vec![
                    "删暗号".into(), "去掉暗号".into(), "移除暗号".into(), "删除暗号".into(),
                ],
                fuzzy_phrases: Vec::new(),
                composite_keywords: Vec::new(),
                confirmation_prompt: None,
                cancellation_prompt: None,
            },
            IntentRule {
                action: Action::Stats,
                exact_phrases: vec![
                    "使用统计".into(), "我的积累".into(), "进化报告".into(),
                    "stats".into(), "查看统计".into(), "积累了什么".into(),
                ],
                fuzzy_phrases: Vec::new(),
                composite_keywords: Vec::new(),
                confirmation_prompt: None,
                cancellation_prompt: None,
            },
            IntentRule {
                action: Action::ExportSkill,
                exact_phrases: vec![
                    "导出技能".into(), "导出skill".into(), "export skill".into(), "技能导出".into(),
                ],
                fuzzy_phrases: Vec::new(),
                composite_keywords: Vec::new(),
                confirmation_prompt: None,
                cancellation_prompt: None,
            },
        ];

        self.soft_signals = vec![
            "这个方法挺好".into(), "学到了".into(), "记住了".into(), "明白了".into(),
            "这个思路不错".into(), "原来如此".into(), "受教了".into(), "mark一下".into(),
            "码住".into(), "涨知识了".into(), "挺有用的".into(), "很实用".into(),
            "不错的方法".into(), "好方法".into(), "这个技巧不错".into(),
        ];

        self.correction_signals = vec![
            "纠正".into(), "你漏掉".into(), "漏掉".into(), "补充".into(),
            "修正".into(), "应该".into(), "下次".into(), "记住".into(),
            "wrong".into(), "missed".into(),
        ];
    }

    // ── Detection ─────────────────────────────────────────────────────────────

    /// Detect intent from user message.
    /// Returns (action, needs_confirmation) if matched.
    pub fn detect(&self, message: &str) -> Option<(Action, bool)> {
        for rule in &self.rules {
            // Check exact match first
            if rule.exact_phrases.iter().any(|p| message.contains(p)) {
                return Some((rule.action.clone(), false));
            }
            // Check fuzzy match (continuous substring)
            if rule.fuzzy_phrases.iter().any(|p| message.contains(p)) {
                return Some((rule.action.clone(), rule.needs_confirmation()));
            }
            // Check composite keywords: ALL keywords in any group must be present
            for keyword_group in &rule.composite_keywords {
                if keyword_group.iter().all(|kw| message.contains(kw)) {
                    return Some((rule.action.clone(), rule.needs_confirmation()));
                }
            }
        }
        None
    }

    /// Get the confirmation prompt for an action.
    pub fn confirmation_prompt(&self, action: &Action) -> Option<String> {
        self.rules.iter()
            .find(|r| &r.action == action)
            .and_then(|r| r.confirmation_prompt.clone())
    }

    /// Get the cancellation prompt for an action.
    pub fn cancellation_prompt(&self, action: &Action) -> Option<String> {
        self.rules.iter()
            .find(|r| &r.action == action)
            .and_then(|r| r.cancellation_prompt.clone())
    }

    pub fn is_correction(message: &str) -> bool {
        // This is a static method, use built-in defaults
        let signals = vec![
            "纠正", "你漏掉", "漏掉", "补充", "修正", "应该", "下次", "记住",
            "wrong", "missed",
        ];
        signals.iter().any(|s| message.contains(s))
    }

    pub fn has_soft_signal(message: &str) -> bool {
        // This is a static method, use built-in defaults
        let signals = vec![
            "这个方法挺好", "学到了", "记住了", "明白了", "这个思路不错",
            "原来如此", "受教了", "mark一下", "码住", "涨知识了",
            "挺有用的", "很实用", "不错的方法", "好方法", "这个技巧不错",
        ];
        signals.iter().any(|s| message.contains(s))
    }

    /// Extract @skill-name mentions from a message.
    pub fn extract_at_skills(message: &str) -> Vec<String> {
        let mut skills = Vec::new();
        let chars: Vec<char> = message.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '@' {
                i += 1;
                let start = i;
                while i < chars.len()
                    && (chars[i].is_alphanumeric()
                        || chars[i] == '-'
                        || chars[i] == '_'
                        || chars[i] == '?')
                {
                    i += 1;
                }
                let name: String = chars[start..i].iter().collect();
                if !name.is_empty() {
                    skills.push(name);
                }
            } else {
                i += 1;
            }
        }
        skills
    }

    /// All rules for listing in the codeword display.
    pub fn all_rules(&self) -> &Vec<IntentRule> {
        &self.rules
    }

    // ── Custom aliases (persisted in JSON) ────────────────────────────────────

    /// Load custom aliases from JSON config file and merge into rules.
    /// JSON format: { "save_skill": ["码住", "收好"], "list_skills": ["翻翻看"] }
    pub fn load_custom(&mut self, config_path: &Path) {
        let text = match fs::read_to_string(config_path) {
            Ok(t) => t,
            Err(_) => return,
        };
        let map: HashMap<String, Vec<String>> = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(_) => return,
        };

        for (action_key, aliases) in map {
            let action = match action_key.as_str() {
                "save_skill" | "SaveSkill" => Action::SaveSkill,
                "list_skills" | "ListSkills" => Action::ListSkills,
                "list_codewords" | "ListCodewords" => Action::ListCodewords,
                "start_flywheel" | "StartFlywheel" => Action::StartFlywheel,
                "load_domain" | "LoadDomain" => Action::LoadDomain,
                "delete_skill" | "DeleteSkill" => Action::DeleteSkill,
                "show_skill" | "ShowSkill" => Action::ShowSkill,
                "create_skill" | "CreateSkill" => Action::CreateSkill,
                "add_codeword" | "AddCodeword" => Action::AddCodeword,
                "edit_codeword" | "EditCodeword" => Action::EditCodeword,
                "remove_codeword" | "RemoveCodeword" => Action::RemoveCodeword,
                "stats" | "Stats" => Action::Stats,
                "export_skill" | "ExportSkill" => Action::ExportSkill,
                _ => continue,
            };

            if let Some(rule) = self.rules.iter_mut().find(|r| r.action == action) {
                for alias in aliases {
                    if !rule.exact_phrases.contains(&alias) {
                        rule.exact_phrases.push(alias);
                    }
                }
            }
        }
    }

    /// Persist custom aliases to JSON.
    pub fn save_custom(&self, config_path: &Path, action: &Action, new_aliases: &[String]) {
        let text = fs::read_to_string(config_path).unwrap_or_default();
        let mut map: HashMap<String, Vec<String>> =
            serde_json::from_str(&text).unwrap_or_default();

        let key = action_to_key(action);
        let entry = map.entry(key.to_string()).or_default();
        for alias in new_aliases {
            if !entry.contains(alias) {
                entry.push(alias.clone());
            }
        }

        if let Ok(json) = serde_json::to_string_pretty(&map) {
            if let Some(parent) = config_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = fs::write(config_path, json);
        }
    }

    /// Remove a custom alias and re-save.
    pub fn remove_custom_alias(&self, config_path: &Path, alias: &str) {
        let text = fs::read_to_string(config_path).unwrap_or_default();
        let mut map: HashMap<String, Vec<String>> =
            serde_json::from_str(&text).unwrap_or_default();

        for aliases in map.values_mut() {
            aliases.retain(|a| a != alias);
        }

        if let Ok(json) = serde_json::to_string_pretty(&map) {
            let _ = fs::write(config_path, json);
        }
    }

    /// Add a runtime alias (in-memory + persisted).
    pub fn add_alias_runtime(&mut self, config_path: &Path, alias: &str, action: &Action) {
        if let Some(rule) = self.rules.iter_mut().find(|r| &r.action == action) {
            if !rule.exact_phrases.contains(&alias.to_string()) {
                rule.exact_phrases.push(alias.to_string());
            }
        }
        self.save_custom(config_path, action, &[alias.to_string()]);
    }
}

impl Default for CodewordDetector {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn action_to_key(action: &Action) -> &'static str {
    match action {
        Action::SaveSkill => "save_skill",
        Action::ListSkills => "list_skills",
        Action::ListCodewords => "list_codewords",
        Action::StartFlywheel => "start_flywheel",
        Action::LoadDomain => "load_domain",
        Action::DeleteSkill => "delete_skill",
        Action::ShowSkill => "show_skill",
        Action::CreateSkill => "create_skill",
        Action::AddCodeword => "add_codeword",
        Action::EditCodeword => "edit_codeword",
        Action::RemoveCodeword => "remove_codeword",
        Action::Stats => "stats",
        Action::ExportSkill => "export_skill",
    }
}

/// Parse "domain/name" from a message. Returns (domain, name).
pub fn extract_skill_ref(message: &str) -> Option<(String, String)> {
    for word in message.split_whitespace() {
        let w = word.trim_matches(|c: char| {
            !c.is_alphanumeric() && c != '-' && c != '_' && c != '/'
        });
        if let Some((d, n)) = w.split_once('/') {
            let d = d.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
            let n = n.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
            if !d.is_empty() && !n.is_empty() {
                return Some((d.to_lowercase(), n.to_lowercase()));
            }
        }
    }
    None
}

/// Parse a bare domain name from the tail of a message like "加载技能 cooking".
pub fn extract_domain_name(message: &str) -> Option<String> {
    message
        .split_whitespace()
        .last()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_'))
        .filter(|w| !w.is_empty() && !w.contains('/') && w.len() > 1)
        .map(|w| w.to_lowercase())
}

/// Human-readable label for an action (used in codeword list display).
pub fn action_label(action: &Action) -> &'static str {
    match action {
        Action::SaveSkill => "保存技能",
        Action::ListSkills => "查看技能库",
        Action::ListCodewords => "查看暗号列表",
        Action::StartFlywheel => "启动飞轮/蒸馏",
        Action::LoadDomain => "加载领域技能",
        Action::DeleteSkill => "删除技能",
        Action::ShowSkill => "查看技能详情",
        Action::CreateSkill => "创建协议 Skill",
        Action::AddCodeword => "新增暗号",
        Action::EditCodeword => "修改暗号",
        Action::RemoveCodeword => "删除暗号",
        Action::Stats => "查看使用统计/进化报告",
        Action::ExportSkill => "导出技能（含上下文损失提示）",
    }
}
