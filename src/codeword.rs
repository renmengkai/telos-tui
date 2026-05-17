use std::collections::HashMap;
use std::fs;
use std::path::Path;

// ── Actions ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
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

// ── Signal constants ──────────────────────────────────────────────────────────

const CORRECTION_SIGNALS: &[&str] = &[
    "纠正", "你漏掉", "漏掉", "补充", "修正", "应该", "下次", "记住",
    "wrong", "missed",
];

/// Passive signals that suggest the user found procedural value in the conversation.
pub const SOFT_SIGNALS: &[&str] = &[
    "这个方法挺好", "学到了", "记住了", "明白了", "这个思路不错",
    "原来如此", "受教了", "mark一下", "码住", "涨知识了",
    "挺有用的", "很实用", "不错的方法", "好方法", "这个技巧不错",
];

// ── Built-in rules ────────────────────────────────────────────────────────────

/// (trigger phrases, action)
static BUILTIN_RULES: &[(&[&str], Action)] = &[
    (
        &["记下来", "保存", "存一下", "存起来", "归档", "收入囊中", "存好",
          "记住这个", "保存技能", "保存这个技巧", "记住这个方法", "整理一下"],
        Action::SaveSkill,
    ),
    (
        &["我都有什么技能", "看看家底", "技能库", "有哪些技能", "翻翻看",
          "列出技能", "skill list"],
        Action::ListSkills,
    ),
    (
        &["暗号大全", "对暗号", "有哪些暗号", "查看暗号", "暗号列表"],
        Action::ListCodewords,
    ),
    (
        &["启动飞轮", "开始分析", "蒸馏", "开始蒸馏", "更新skill",
          "从日志学习", "专家会诊", "复盘", "总结提升"],
        Action::StartFlywheel,
    ),
    (
        &["加载技能", "唤起", "加载领域", "载入技能"],
        Action::LoadDomain,
    ),
    (
        &["删除技能", "删掉", "移除技能", "删除这个技能"],
        Action::DeleteSkill,
    ),
    (
        &["查看技能", "看看这个技能", "技能详情", "skill show"],
        Action::ShowSkill,
    ),
    (
        &["创建新skill", "新建协议技能", "创建协议", "add-skill",
           "新增协议", "新建skill", "创建skill", "创建协议skill",
           "新skill", "add skill", "new skill", "new protocol",
           "创建新协议", "建一个skill", "给我建个skill", "我要创建skill",
           "帮我创建skill", "新建一个skill", "添加skill", "增加skill",
           "来个skill", "整一个skill", "创建一个skill"],
        Action::CreateSkill,
    ),
    (
        &["新增暗号", "加个暗号", "添加暗号", "新暗号"],
        Action::AddCodeword,
    ),
    (
        &["改暗号", "修改暗号", "编辑暗号"],
        Action::EditCodeword,
    ),
    (
        &["删暗号", "去掉暗号", "移除暗号", "删除暗号"],
        Action::RemoveCodeword,
    ),
    (
        &["使用统计", "我的积累", "进化报告", "stats", "查看统计", "积累了什么"],
        Action::Stats,
    ),
    (
        &["导出技能", "导出skill", "export skill", "技能导出"],
        Action::ExportSkill,
    ),
];

// ── Detector ──────────────────────────────────────────────────────────────────

pub struct CodewordDetector {
    /// Merged rules: (alias_list, action). Custom aliases appended to built-ins.
    rules: Vec<(Vec<String>, Action)>,
}

impl CodewordDetector {
    pub fn new() -> Self {
        let rules = BUILTIN_RULES
            .iter()
            .map(|(phrases, action)| {
                (phrases.iter().map(|s| s.to_string()).collect(), action.clone())
            })
            .collect();
        Self { rules }
    }

    /// Load custom aliases from JSON config file and merge into built-in rules.
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
                _ => continue,
            };
            // Append aliases to the matching rule
            if let Some(rule) = self.rules.iter_mut().find(|(_, a)| a == &action) {
                for alias in aliases {
                    if !rule.0.contains(&alias) {
                        rule.0.push(alias);
                    }
                }
            }
        }
    }

    /// Persist custom (non-built-in) aliases to JSON.
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
        if let Some(rule) = self.rules.iter_mut().find(|(_, a)| a == action) {
            if !rule.0.contains(&alias.to_string()) {
                rule.0.push(alias.to_string());
            }
        }
        self.save_custom(config_path, action, &[alias.to_string()]);
    }

    /// Returns the first matching action if any codeword phrase is found.
    pub fn detect(&self, message: &str) -> Option<Action> {
        for (phrases, action) in &self.rules {
            if phrases.iter().any(|p| message.contains(p.as_str())) {
                return Some(action.clone());
            }
        }
        None
    }

    pub fn is_correction(message: &str) -> bool {
        CORRECTION_SIGNALS.iter().any(|s| message.contains(s))
    }

    pub fn has_soft_signal(message: &str) -> bool {
        SOFT_SIGNALS.iter().any(|s| message.contains(s))
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
    pub fn all_rules(&self) -> &[(Vec<String>, Action)] {
        &self.rules
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
        // Strip surrounding punctuation
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
