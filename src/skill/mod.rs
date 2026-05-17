pub mod store;
pub mod matcher;

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub domain: String,
    /// Knowledge skills: path to .md file. Protocol skills: path to skill dir.
    pub path: PathBuf,
    pub tags: Vec<String>,
    pub summary: String,
    /// Protocol skills have skill.yaml + domain.md; knowledge skills don't.
    pub is_protocol: bool,
    pub content: Option<String>,
}
