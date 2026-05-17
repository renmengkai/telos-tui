use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── project state ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectState {
    pub id: String,
    pub skills: Vec<String>,
    pub ring: u8,
    pub assumptions: u32,
}

impl ProjectState {
    pub fn new(id: String) -> Self {
        Self { id, skills: Vec::new(), ring: 1, assumptions: 0 }
    }

    /// Generate `proj_YYYYMMDD_NN` — next unused number for today.
    pub fn generate_id(projects_dir: &Path) -> String {
        let date = cmd_date("+%Y%m%d");
        let prefix = format!("proj_{date}");

        let max: u32 = fs::read_dir(projects_dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| {
                        let n = e.file_name().into_string().ok()?;
                        let suffix = n.strip_prefix(&format!("{prefix}_"))?;
                        suffix.parse::<u32>().ok()
                    })
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);

        format!("{prefix}_{:02}", max + 1)
    }

    pub fn load(projects_dir: &Path, id: &str) -> Option<Self> {
        let text = fs::read_to_string(projects_dir.join(id).join("state.json")).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save(&self, projects_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let dir = projects_dir.join(&self.id);
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("state.json"), serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

// ── episodic logger ───────────────────────────────────────────────────────────

pub struct EpisodicLogger {
    pub project_id: String,
    logs_dir: PathBuf,
    distill_dir: PathBuf,
}

impl EpisodicLogger {
    pub fn new(project_id: String, logs_dir: PathBuf, distill_dir: PathBuf) -> Self {
        Self { project_id, logs_dir, distill_dir }
    }

    pub fn log(&self, event: Value) {
        let _ = fs::create_dir_all(&self.logs_dir);
        let path = self.logs_dir.join(format!("{}.jsonl", self.project_id));
        let _ = append_file(&path, &format!("{event}\n"));
    }

    /// Count events with quality_score >= 0.6
    pub fn quality_event_count(&self) -> usize {
        let path = self.logs_dir.join(format!("{}.jsonl", self.project_id));
        fs::read_to_string(path)
            .map(|s| {
                s.lines()
                    .filter(|l| !l.trim().is_empty())
                    .filter(|l| {
                        serde_json::from_str::<Value>(l)
                            .ok()
                            .and_then(|v| v["quality_score"].as_f64())
                            .map(|q| q >= 0.6)
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    pub fn mark_distill_needed(&self) {
        let _ = fs::create_dir_all(&self.distill_dir);
        let _ = fs::write(self.distill_dir.join(&self.project_id), "");
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

pub fn append_file(path: &Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(content.as_bytes())
}

fn cmd_date(fmt: &str) -> String {
    std::process::Command::new("date")
        .arg(fmt)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "00000000".to_string())
}

pub fn timestamp_now() -> String {
    cmd_date("+%Y-%m-%dT%H:%M:%SZ")
}

/// Return the ID of the most recently modified project, if any.
pub fn last_project_id(projects_dir: &Path) -> Option<String> {
    fs::read_dir(projects_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir() && e.path().join("state.json").exists())
        .max_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .ok()
        })
        .and_then(|e| e.file_name().into_string().ok())
}

/// Count how many distinct project log files have at least one quality event (score ≥ 0.6).
/// Used for cross-project Ring 3 trigger.
pub fn projects_with_quality_events(logs_dir: &Path) -> usize {
    fs::read_dir(logs_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path().extension().and_then(|x| x.to_str()) == Some("jsonl")
                })
                .filter(|e| {
                    fs::read_to_string(e.path())
                        .unwrap_or_default()
                        .lines()
                        .any(|l| {
                            !l.trim().is_empty()
                                && serde_json::from_str::<Value>(l)
                                    .ok()
                                    .and_then(|v| v["quality_score"].as_f64())
                                    .map(|q| q >= 0.6)
                                    .unwrap_or(false)
                        })
                })
                .count()
        })
        .unwrap_or(0)
}
