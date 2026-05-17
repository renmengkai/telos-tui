use std::fs;
use std::path::{Path, PathBuf};

use super::Skill;

pub struct SkillStore {
    pub skills_dir: PathBuf,
}

impl SkillStore {
    pub fn new(skills_dir: PathBuf) -> Self {
        Self { skills_dir }
    }

    /// Scan _index.md for knowledge skills only.
    pub fn scan(&self) -> Vec<Skill> {
        let index = self.skills_dir.join("_index.md");
        let content = fs::read_to_string(&index).unwrap_or_default();
        parse_index(&content, &self.skills_dir)
    }

    /// Scan both knowledge skills (_index.md) and protocol skills (dirs with skill.yaml).
    pub fn scan_all(&self) -> Vec<Skill> {
        let mut skills = self.scan();

        let entries = match fs::read_dir(&self.skills_dir) {
            Ok(e) => e,
            Err(_) => return skills,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let dir_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if dir_name.starts_with('_') {
                continue; // skip _kernel, _template
            }
            if !path.join("skill.yaml").exists() {
                continue;
            }
            if let Some(skill) = self.load_protocol_meta(&path, &dir_name) {
                if !skills.iter().any(|s| s.name == skill.name) {
                    skills.push(skill);
                }
            }
        }

        skills
    }

    /// Parse skill.yaml without a YAML crate — handles simple `key: value` lines.
    fn load_protocol_meta(&self, dir: &Path, dir_name: &str) -> Option<Skill> {
        let text = fs::read_to_string(dir.join("skill.yaml")).ok()?;
        let mut name = dir_name.to_string();
        let mut summary = String::new();
        let mut tags: Vec<String> = Vec::new();

        for line in text.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("name:") {
                name = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("description:") {
                summary = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("tags:") {
                let v = v.trim().trim_start_matches('[').trim_end_matches(']');
                tags = v.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
            }
        }

        Some(Skill {
            name,
            domain: dir_name.to_string(),
            path: dir.to_path_buf(),
            tags,
            summary,
            is_protocol: true,
            content: None,
        })
    }

    /// Load skill content from disk.
    /// Knowledge skills: reads the .md file.
    /// Protocol skills: reads domain.md inside the skill directory.
    pub fn load_content(&self, skill: &mut Skill) {
        if skill.content.is_some() {
            return;
        }
        if skill.is_protocol {
            skill.content = fs::read_to_string(skill.path.join("domain.md")).ok();
        } else {
            skill.content = fs::read_to_string(&skill.path).ok();
        }
    }

    /// Concatenate all .md files in skill/memory/ into one string.
    pub fn load_memory(&self, skill: &Skill) -> String {
        let memory_dir = skill.path.join("memory");
        if !memory_dir.is_dir() {
            return String::new();
        }
        let mut out = String::new();
        if let Ok(entries) = fs::read_dir(&memory_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("md") {
                    if let Ok(c) = fs::read_to_string(&p) {
                        out.push_str(&c);
                        out.push('\n');
                    }
                }
            }
        }
        out
    }

    /// Find and read an industry pack whose filename appears as a keyword in `text`.
    pub fn find_industry_pack(&self, skill: &Skill, text: &str) -> Option<String> {
        let dir = skill.path.join("industry");
        if !dir.is_dir() {
            return None;
        }
        let lower = text.to_lowercase();
        let entries = fs::read_dir(&dir).ok()?;
        for entry in entries.filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let stem = p.file_stem().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
            if lower.contains(&stem) {
                return fs::read_to_string(&p).ok();
            }
        }
        None
    }

    /// Save a new skill or merge into an existing one.
    /// When the file already exists: increments `conversation_count`, updates `last_updated`,
    /// replaces body with new_body (which LLM already produced from full conversation context).
    pub fn save_or_merge(
        &self,
        domain: &str,
        name: &str,
        new_body: &str,
        tags: &[String],
        summary: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let domain_dir = self.skills_dir.join(domain);
        fs::create_dir_all(&domain_dir)?;
        let file_path = domain_dir.join(format!("{name}.md"));
        let today = today_iso();
        let tags_str = tags.join(", ");

        if file_path.exists() {
            let existing = fs::read_to_string(&file_path).unwrap_or_default();
            let created = frontmatter_value(&existing, "created").unwrap_or_else(|| today.clone());
            let count: u32 = frontmatter_value(&existing, "conversation_count")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1)
                + 1;

            let content = format!(
                "---\nname: {name}\ndomain: {domain}\ntags: [{tags_str}]\n\
                 created: {created}\nlast_updated: {today}\nconversation_count: {count}\n---\n\n{new_body}\n"
            );
            fs::write(&file_path, content)?;
            self.update_index_entry(domain, name, tags, summary)?;
        } else {
            let content = format!(
                "---\nname: {name}\ndomain: {domain}\ntags: [{tags_str}]\n\
                 created: {today}\nlast_updated: {today}\nconversation_count: 1\n---\n\n{new_body}\n"
            );
            fs::write(&file_path, &content)?;
            self.append_index(domain, name, &file_path, tags, summary)?;
        }

        Ok(file_path)
    }

    /// Overwrite the body of an existing skill, preserving frontmatter metadata.
    /// For protocol skills writes to domain.md (fixes write-to-directory bug).
    /// Increments distill_count and preserves projects_involved.
    pub fn update_content(
        &self,
        skill: &Skill,
        new_body: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let file_path = skill_file_path(skill);
        let existing = fs::read_to_string(&file_path).unwrap_or_default();
        let today = today_iso();

        let created = frontmatter_value(&existing, "created").unwrap_or_else(|| today.clone());
        let count: u32 = frontmatter_value(&existing, "conversation_count")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        let distill_count: u32 = frontmatter_value(&existing, "distill_count")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0) + 1;
        let projects = frontmatter_value(&existing, "projects_involved").unwrap_or_default();

        let tags_str = skill.tags.join(", ");
        let mut new_file = format!(
            "---\nname: {}\ndomain: {}\ntags: [{tags_str}]\n\
             created: {created}\nlast_updated: {today}\nconversation_count: {count}\ndistill_count: {distill_count}\n",
            skill.name, skill.domain
        );
        if !projects.is_empty() {
            new_file.push_str(&format!("projects_involved: {projects}\n"));
        }
        new_file.push_str(&format!("---\n\n{new_body}\n"));

        fs::write(&file_path, new_file)?;
        Ok(())
    }

    /// Increment distill_count in frontmatter without replacing the body.
    /// Used by Ring-3 distillation which writes to memory/ rather than replacing domain.md body.
    pub fn increment_distill_count(&self, skill: &Skill) {
        let file_path = skill_file_path(skill);
        let Ok(existing) = fs::read_to_string(&file_path) else { return };
        let (mut pairs, body) = parse_skill_file(&existing);
        let mut found = false;
        for (k, v) in &mut pairs {
            if k == "distill_count" {
                let n: u32 = v.parse().unwrap_or(0);
                *v = (n + 1).to_string();
                found = true;
                break;
            }
        }
        if !found {
            pairs.push(("distill_count".to_string(), "1".to_string()));
        }
        let _ = fs::write(&file_path, rebuild_skill_file(&pairs, &body));
    }

    /// Append project_id to the projects_involved frontmatter list (deduped).
    pub fn add_project_to_skill(&self, skill: &Skill, project_id: &str) {
        let file_path = skill_file_path(skill);
        let Ok(existing) = fs::read_to_string(&file_path) else { return };
        let (mut pairs, body) = parse_skill_file(&existing);
        let mut found = false;
        for (k, v) in &mut pairs {
            if k == "projects_involved" {
                let mut projects: Vec<String> = v
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !projects.contains(&project_id.to_string()) {
                    projects.push(project_id.to_string());
                    *v = format!("[{}]", projects.join(", "));
                }
                found = true;
                break;
            }
        }
        if !found {
            pairs.push(("projects_involved".to_string(), format!("[{project_id}]")));
        }
        let _ = fs::write(&file_path, rebuild_skill_file(&pairs, &body));
    }

    /// Return (conversation_count, distill_count, projects_involved_count) from frontmatter.
    pub fn skill_evolution_info(&self, skill: &Skill) -> (u32, u32, usize) {
        let file_path = skill_file_path(skill);
        let content = fs::read_to_string(&file_path).unwrap_or_default();
        let conv: u32 = frontmatter_value(&content, "conversation_count")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let distill: u32 = frontmatter_value(&content, "distill_count")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let projects_count = frontmatter_value(&content, "projects_involved")
            .map(|s| {
                s.trim_start_matches('[')
                 .trim_end_matches(']')
                 .split(',')
                 .filter(|p| !p.trim().is_empty())
                 .count()
            })
            .unwrap_or(0);
        (conv, distill, projects_count)
    }

    /// Delete a knowledge skill file and remove its entry from _index.md.
    pub fn delete_skill(&self, domain: &str, name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let file_path = self.skills_dir.join(domain).join(format!("{name}.md"));
        if file_path.exists() {
            fs::remove_file(&file_path)?;
        }
        self.remove_index_entry(domain, name)?;
        Ok(())
    }

    /// Update (replace) an existing entry in _index.md for the given domain/name.
    fn update_index_entry(
        &self,
        domain: &str,
        name: &str,
        tags: &[String],
        summary: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let index_path = self.skills_dir.join("_index.md");
        let content = fs::read_to_string(&index_path).unwrap_or_default();

        let needle = format!("[{name}]");
        let new_entry = format!(
            "- [{name}]({domain}/{name}.md) — {summary} · [{}]",
            tags.join(", ")
        );

        let updated: Vec<String> = content
            .lines()
            .map(|l| {
                if l.contains(&needle) && l.contains(&format!("/{domain}/{name}")) {
                    new_entry.clone()
                } else {
                    l.to_string()
                }
            })
            .collect();

        fs::write(&index_path, updated.join("\n"))?;
        Ok(())
    }

    /// Remove an entry from _index.md.
    fn remove_index_entry(&self, domain: &str, name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let index_path = self.skills_dir.join("_index.md");
        let content = fs::read_to_string(&index_path).unwrap_or_default();
        let needle = format!("[{name}]");
        let filtered: Vec<&str> = content
            .lines()
            .filter(|l| !(l.contains(&needle) && l.contains(&format!("/{domain}/{name}"))))
            .collect();
        fs::write(&index_path, filtered.join("\n"))?;
        Ok(())
    }

    /// Append an entry to the appropriate domain section of _index.md
    fn append_index(
        &self,
        domain: &str,
        name: &str,
        path: &Path,
        tags: &[String],
        summary: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let index_path = self.skills_dir.join("_index.md");
        let mut content = fs::read_to_string(&index_path).unwrap_or_default();

        let rel = path
            .strip_prefix(&self.skills_dir)
            .unwrap_or(path)
            .to_string_lossy();
        let entry = format!("- [{name}]({rel}) — {summary} · [{}]\n", tags.join(", "));
        let header = format!("\n## {domain}\n");

        if let Some(pos) = content.find(&header) {
            content.insert_str(pos + header.len(), &entry);
        } else {
            content.push_str(&header);
            content.push_str(&entry);
        }

        fs::write(&index_path, content)?;
        Ok(())
    }
}

// ── index parsing ─────────────────────────────────────────────────────────────

fn parse_index(content: &str, skills_dir: &Path) -> Vec<Skill> {
    let mut skills = Vec::new();
    let mut current_domain = String::new();
    let mut in_comment = false;

    for line in content.lines() {
        if line.contains("<!--") {
            in_comment = true;
        }
        if line.contains("-->") {
            in_comment = false;
            continue;
        }
        if in_comment {
            continue;
        }

        if let Some(domain) = line.strip_prefix("## ") {
            let d = domain.trim();
            // Skip kernel and template domains
            if !d.starts_with('_') {
                current_domain = d.to_string();
            } else {
                current_domain.clear();
            }
            continue;
        }

        if line.starts_with("- [") && !current_domain.is_empty() {
            if let Some(skill) = parse_entry(line, &current_domain, skills_dir) {
                skills.push(skill);
            }
        }
    }

    skills
}

/// Parse: `- [name](path) — summary · [tag1, tag2]`
fn parse_entry(line: &str, domain: &str, skills_dir: &Path) -> Option<Skill> {
    let name_start = line.find('[')? + 1;
    let name_end = line.find(']')?;
    let name = line[name_start..name_end].to_string();

    let path_start = line.find('(')? + 1;
    let path_end = line[path_start..].find(')')? + path_start;
    let rel_path = &line[path_start..path_end];
    let path = skills_dir.join(rel_path);

    let summary = line
        .split(" — ")
        .nth(1)
        .and_then(|s| s.split(" · ").next())
        .unwrap_or("")
        .to_string();

    // Tags are in the last [...] on the line
    let tags = line
        .rfind('[')
        .zip(line.rfind(']'))
        .filter(|(s, e)| s < e)
        .map(|(s, e)| {
            line[s + 1..e]
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default();

    Some(Skill {
        name,
        domain: domain.to_string(),
        path,
        tags,
        summary,
        is_protocol: false,
        content: None,
    })
}

fn today_iso() -> String {
    std::process::Command::new("date")
        .arg("+%Y-%m-%d")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Extract a scalar value from YAML frontmatter: `key: value`.
fn frontmatter_value(content: &str, key: &str) -> Option<String> {
    content
        .lines()
        .find(|l| l.starts_with(&format!("{key}:")))
        .and_then(|l| l.splitn(2, ':').nth(1))
        .map(|v| v.trim().to_string())
}

/// Resolve the actual .md file path for a skill (protocol → domain.md, knowledge → skill.path).
fn skill_file_path(skill: &Skill) -> PathBuf {
    if skill.is_protocol {
        skill.path.join("domain.md")
    } else {
        skill.path.clone()
    }
}

/// Parse a skill .md file into (frontmatter key-value pairs, body text).
/// Splits on the closing `---` line; preserves all frontmatter fields.
fn parse_skill_file(content: &str) -> (Vec<(String, String)>, String) {
    let mut pairs = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    if lines.first().copied() != Some("---") {
        return (pairs, content.to_string());
    }

    let mut i = 1;
    while i < lines.len() && lines[i] != "---" {
        if let Some(colon) = lines[i].find(':') {
            let k = lines[i][..colon].trim().to_string();
            let v = lines[i][colon + 1..].trim().to_string();
            if !k.is_empty() {
                pairs.push((k, v));
            }
        }
        i += 1;
    }
    if i < lines.len() { i += 1; } // skip closing ---
    while i < lines.len() && lines[i].is_empty() { i += 1; } // skip blank lines

    let body = lines[i..].join("\n");
    (pairs, body)
}

/// Reconstruct a skill .md file from frontmatter pairs and body.
fn rebuild_skill_file(pairs: &[(String, String)], body: &str) -> String {
    let mut out = "---\n".to_string();
    for (k, v) in pairs {
        out.push_str(&format!("{k}: {v}\n"));
    }
    out.push_str("---\n\n");
    out.push_str(body);
    if !body.is_empty() && !body.ends_with('\n') {
        out.push('\n');
    }
    out
}
