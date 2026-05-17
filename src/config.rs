use std::path::PathBuf;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub enum Provider {
    Anthropic,
    OpenAi,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub provider: Provider,
    pub model: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub skills_dir: PathBuf,
}

// ── TOML schema ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    #[serde(default)]
    llm: LlmFileConfig,
    #[serde(default)]
    skills: SkillsFileConfig,
}

#[derive(Debug, Deserialize, Default)]
struct LlmFileConfig {
    provider: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct SkillsFileConfig {
    dir: Option<String>,
}

// ── Loading ───────────────────────────────────────────────────────────────────

impl Config {
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let file_cfg = Self::load_file().unwrap_or_default();

        // Provider: env > file > default (anthropic)
        let provider_str = std::env::var("TELOS_PROVIDER")
            .ok()
            .or(file_cfg.llm.provider)
            .unwrap_or_else(|| "anthropic".to_string());

        let provider = match provider_str.to_lowercase().as_str() {
            "openai" => Provider::OpenAi,
            _ => Provider::Anthropic,
        };

        // API key: env > file > error
        let default_key_env = match &provider {
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::OpenAi => "OPENAI_API_KEY",
        };
        let api_key = std::env::var("TELOS_API_KEY")
            .ok()
            .or_else(|| std::env::var(default_key_env).ok())
            .or(file_cfg.llm.api_key)
            .ok_or("API key not set (use TELOS_API_KEY, ANTHROPIC_API_KEY/OPENAI_API_KEY, or llm.api_key in config file)")?;

        // Model: env > file > default
        let default_model = match &provider {
            Provider::Anthropic => "claude-opus-4-7",
            Provider::OpenAi => "gpt-4o",
        };
        let model = std::env::var("TELOS_MODEL")
            .ok()
            .or(file_cfg.llm.model)
            .unwrap_or_else(|| default_model.to_string());

        // Base URL: env > file
        let base_url = std::env::var("TELOS_BASE_URL")
            .ok()
            .or(file_cfg.llm.base_url);

        // Skills dir: env > file > default
        let skills_dir = std::env::var("TELOS_SKILLS_DIR")
            .ok()
            .or(file_cfg.skills.dir)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("./skills"));

        Ok(Config { provider, model, api_key, base_url, skills_dir })
    }

    /// Try candidate paths in order; return the first successfully parsed file.
    fn load_file() -> Option<FileConfig> {
        let candidates = Self::config_paths();
        for path in candidates {
            if let Ok(text) = std::fs::read_to_string(&path) {
                match toml::from_str::<FileConfig>(&text) {
                    Ok(cfg) => return Some(cfg),
                    Err(e) => eprintln!("telos: warning: could not parse {}: {e}", path.display()),
                }
            }
        }
        None
    }

    fn config_paths() -> Vec<PathBuf> {
        let mut paths = vec![PathBuf::from("./telos.toml")];

        // ~/.config/telos/config.toml
        if let Some(mut home) = dirs_or_env_home() {
            home.push(".config");
            home.push("telos");
            home.push("config.toml");
            paths.push(home);
        }

        paths
    }

    // ── path helpers ──────────────────────────────────────────────────────────

    /// Root of the project workspace (parent of skills_dir).
    pub fn project_root(&self) -> PathBuf {
        self.skills_dir
            .parent()
            .unwrap_or(&self.skills_dir)
            .to_path_buf()
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.project_root().join("projects")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.project_root().join("episodic-logs")
    }

    pub fn distill_needed_dir(&self) -> PathBuf {
        self.project_root().join(".distill-needed")
    }

    /// Path for persisted custom codewords: ~/.config/telos/codewords.json
    pub fn codewords_config_path(&self) -> PathBuf {
        std::env::var("HOME")
            .map(PathBuf::from)
            .map(|p| p.join(".config").join("telos").join("codewords.json"))
            .unwrap_or_else(|_| self.project_root().join(".telos-codewords.json"))
    }

    /// Path for usage stats (codeword counts): ~/.config/telos/usage-stats.json
    pub fn stats_path(&self) -> PathBuf {
        std::env::var("HOME")
            .map(PathBuf::from)
            .map(|p| p.join(".config").join("telos").join("usage-stats.json"))
            .unwrap_or_else(|_| self.project_root().join(".telos-stats.json"))
    }
}

fn dirs_or_env_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}
