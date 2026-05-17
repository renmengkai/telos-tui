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

/// Count total quality events (score >= 0.6) across all project log files.
pub fn total_quality_events(logs_dir: &Path) -> usize {
    fs::read_dir(logs_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jsonl"))
                .map(|e| {
                    fs::read_to_string(e.path())
                        .unwrap_or_default()
                        .lines()
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
                .sum()
        })
        .unwrap_or(0)
}

/// Count the total number of distillations across all skills.
/// A distillation is counted as a non-empty memory/distilled.md file.
pub fn total_distillations(skills_dir: &Path) -> usize {
    let mut count = 0;
    let Ok(entries) = fs::read_dir(skills_dir) else {
        return 0;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() && !path.file_name().and_then(|n| n.to_str()).unwrap_or("").starts_with('_') {
            let distilled = path.join("memory").join("distilled.md");
            if distilled.is_file() {
                if let Ok(content) = fs::read_to_string(&distilled) {
                    let non_empty_lines = content.lines().filter(|l| !l.trim().is_empty()).count();
                    if non_empty_lines > 0 {
                        count += 1;
                    }
                }
            }
        }
    }
    count
}

/// Calculate days since the earliest skill creation date.
/// Scans all .md files in skills_dir for `created:` frontmatter.
pub fn days_since_first_use(skills_dir: &Path) -> u64 {
    let mut earliest: Option<String> = None;
    let Ok(entries) = fs::read_dir(skills_dir) else {
        return 0;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            // Scan .md files in subdirectory
            if let Ok(files) = fs::read_dir(&path) {
                for file in files.filter_map(|f| f.ok()) {
                    let fp = file.path();
                    if fp.extension().and_then(|e| e.to_str()) == Some("md") {
                        if let Ok(content) = fs::read_to_string(&fp) {
                            if let Some(created) = extract_created_date(&content) {
                                earliest = Some(match earliest {
                                    Some(ref e) if created < *e => created,
                                    _ => earliest.unwrap_or(created),
                                });
                            }
                        }
                    }
                }
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Some(created) = extract_created_date(&content) {
                    earliest = Some(match earliest {
                        Some(ref e) if created < *e => created,
                        _ => earliest.unwrap_or(created),
                    });
                }
            }
        }
    }

    if let Some(earliest_date) = earliest {
        let today = cmd_date("+%Y-%m-%d");
        days_between(&earliest_date, &today).unwrap_or(0)
    } else {
        0
    }
}

fn extract_created_date(content: &str) -> Option<String> {
    content
        .lines()
        .find(|l| l.trim().starts_with("created:"))
        .and_then(|l| l.splitn(2, ':').nth(1))
        .map(|v| v.trim().to_string())
}

/// Calculate days between two ISO dates (YYYY-MM-DD) using Unix timestamp math.
fn days_between(start: &str, end: &str) -> Option<u64> {
    let start_ts = parse_iso_date_to_timestamp(start)?;
    let end_ts = parse_iso_date_to_timestamp(end)?;
    let diff = end_ts.saturating_sub(start_ts);
    Some(diff / 86400)
}

/// Parse YYYY-MM-DD to Unix timestamp (seconds since epoch) at midnight UTC.
fn parse_iso_date_to_timestamp(date: &str) -> Option<u64> {
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let year: i32 = parts[0].parse().ok()?;
    let month: u32 = parts[1].parse().ok()?;
    let day: u32 = parts[2].parse().ok()?;

    // Simple Unix timestamp calculation for dates from 1970 onwards
    // Using days-from-epoch approach
    let mut y = year as i64;
    let mut m = month as i64;
    let d = day as i64;
    if m <= 2 {
        y -= 1;
        m += 12;
    }
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + (if m > 2 { -3 } else { 9 })) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days_since_epoch = era * 146097 + doe - 719468;
    Some((days_since_epoch as u64) * 86400)
}

/// Count the number of project directories with a state.json file.
pub fn project_count(projects_dir: &Path) -> usize {
    fs::read_dir(projects_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir() && e.path().join("state.json").exists())
                .count()
        })
        .unwrap_or(0)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_parse_iso_date_to_timestamp() {
        // 1970-01-01 should be 0
        assert_eq!(parse_iso_date_to_timestamp("1970-01-01"), Some(0));
        // 1970-01-02 should be 86400
        assert_eq!(parse_iso_date_to_timestamp("1970-01-02"), Some(86400));
        // Invalid dates should return None
        assert_eq!(parse_iso_date_to_timestamp("invalid"), None);
        assert_eq!(parse_iso_date_to_timestamp(""), None);
    }

    #[test]
    fn test_days_between() {
        assert_eq!(days_between("1970-01-01", "1970-01-01"), Some(0));
        assert_eq!(days_between("1970-01-01", "1970-01-02"), Some(1));
        assert_eq!(days_between("1970-01-01", "1970-02-01"), Some(31));
    }

    #[test]
    fn test_extract_created_date() {
        let content = "---\nname: test\ncreated: 2026-05-10\n---\n\nbody";
        assert_eq!(extract_created_date(content), Some("2026-05-10".to_string()));

        let no_date = "---\nname: test\n---\n\nbody";
        assert_eq!(extract_created_date(no_date), None);
    }

    #[test]
    fn test_project_count() {
        let tmp = tempfile::tempdir().unwrap();
        let projects_dir = tmp.path();

        assert_eq!(project_count(projects_dir), 0);

        // Create a project dir with state.json
        let proj1 = projects_dir.join("proj_20260517_01");
        fs::create_dir(&proj1).unwrap();
        fs::write(proj1.join("state.json"), "{}").unwrap();

        assert_eq!(project_count(projects_dir), 1);

        // Create another project dir without state.json (should not count)
        let proj2 = projects_dir.join("proj_20260517_02");
        fs::create_dir(&proj2).unwrap();

        assert_eq!(project_count(projects_dir), 1);
    }

    #[test]
    fn test_total_quality_events() {
        let tmp = tempfile::tempdir().unwrap();
        let logs_dir = tmp.path();

        assert_eq!(total_quality_events(logs_dir), 0);

        // Create a log file with quality events
        let log_file = logs_dir.join("proj_20260517_01.jsonl");
        let mut file = fs::File::create(&log_file).unwrap();
        writeln!(file, "{{\"type\":\"test\",\"quality_score\":0.8}}").unwrap();
        writeln!(file, "{{\"type\":\"test\",\"quality_score\":0.5}}").unwrap();
        writeln!(file, "{{\"type\":\"test\",\"quality_score\":0.9}}").unwrap();

        assert_eq!(total_quality_events(logs_dir), 2);
    }

    #[test]
    fn test_total_distillations() {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path();

        assert_eq!(total_distillations(skills_dir), 0);

        // Create a skill dir with non-empty distilled.md
        let skill_dir = skills_dir.join("test-skill");
        let memory_dir = skill_dir.join("memory");
        fs::create_dir_all(&memory_dir).unwrap();
        fs::write(memory_dir.join("distilled.md"), "# Distilled content\n\nsome text").unwrap();

        assert_eq!(total_distillations(skills_dir), 1);

        // Empty distilled.md should not count
        let skill_dir2 = skills_dir.join("test-skill-2");
        let memory_dir2 = skill_dir2.join("memory");
        fs::create_dir_all(&memory_dir2).unwrap();
        fs::write(memory_dir2.join("distilled.md"), "   \n\n   ").unwrap();

        assert_eq!(total_distillations(skills_dir), 1);
    }

    #[test]
    fn test_days_since_first_use() {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path();

        assert_eq!(days_since_first_use(skills_dir), 0);

        // Create a skill file with created date
        let skill_file = skills_dir.join("test-skill.md");
        fs::write(&skill_file, "---\nname: test\ncreated: 2026-05-10\n---\n\nbody").unwrap();

        // Since today is 2026-05-17, should be 7 days
        let days = days_since_first_use(skills_dir);
        assert!(days >= 7, "Expected at least 7 days, got {}", days);
    }
}
