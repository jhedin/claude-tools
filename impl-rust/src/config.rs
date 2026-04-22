use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub planner: PlannerCfg,
    #[serde(default)]
    pub safety: SafetyCfg,
    #[serde(default)]
    pub cache: CacheCfg,
    #[serde(default)]
    pub aliases: AliasCfg,
    #[serde(default)]
    pub allowlist: AllowCfg,
    #[serde(default)]
    pub rejectlist: RejectCfg,
    #[serde(default)]
    pub history: HistoryCfg,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlannerCfg {
    #[serde(default = "default_planner_model")]
    pub model: String,
    #[serde(default = "default_planner_timeout")]
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SafetyCfg {
    #[serde(default = "default_safety_model")]
    pub model: String,
    #[serde(default = "default_safety_timeout")]
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CacheCfg {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AliasCfg {
    #[serde(default = "default_tools")]
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AllowCfg {
    #[serde(default)]
    pub extra_safe: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RejectCfg {
    #[serde(default)]
    pub extra_reject: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HistoryCfg {
    #[serde(default)]
    pub path: Option<String>,
}

fn default_planner_model() -> String {
    "claude-sonnet-4-6".into()
}
fn default_safety_model() -> String {
    "claude-sonnet-4-6".into()
}
fn default_planner_timeout() -> u64 {
    30
}
fn default_safety_timeout() -> u64 {
    15
}
fn yes() -> bool {
    true
}
fn default_tools() -> Vec<String> {
    vec![
        "find".into(),
        "grep".into(),
        "jq".into(),
        "awk".into(),
        "sed".into(),
    ]
}

impl Default for PlannerCfg {
    fn default() -> Self {
        Self {
            model: default_planner_model(),
            timeout_seconds: default_planner_timeout(),
        }
    }
}
impl Default for SafetyCfg {
    fn default() -> Self {
        Self {
            model: default_safety_model(),
            timeout_seconds: default_safety_timeout(),
        }
    }
}
impl Default for CacheCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            path: None,
        }
    }
}
impl Default for AliasCfg {
    fn default() -> Self {
        Self {
            tools: default_tools(),
        }
    }
}
impl Default for HistoryCfg {
    fn default() -> Self {
        Self { path: None }
    }
}
impl Default for Config {
    fn default() -> Self {
        Self {
            planner: Default::default(),
            safety: Default::default(),
            cache: Default::default(),
            aliases: Default::default(),
            allowlist: Default::default(),
            rejectlist: Default::default(),
            history: Default::default(),
        }
    }
}

/// Load config from `$CLAUDE_TOOLS_CONFIG` or default `~/.config/claude-tools/config.toml`.
/// Missing file → defaults.
pub fn load() -> Result<Config> {
    let path = config_path();
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading config {}", path.display()))?;
    let cfg: Config = toml::from_str(&text)
        .with_context(|| format!("parsing config {}", path.display()))?;
    Ok(cfg)
}

pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("CLAUDE_TOOLS_CONFIG") {
        return PathBuf::from(p);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("claude-tools")
        .join("config.toml")
}

pub fn cache_path(cfg: &Config) -> PathBuf {
    if let Some(p) = &cfg.cache.path {
        return expand_tilde(p);
    }
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("claude-tools")
        .join("rewrites.db")
}

pub fn history_path(cfg: &Config) -> PathBuf {
    if let Some(p) = &cfg.history.path {
        return expand_tilde(p);
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("claude-tools")
        .join("history.jsonl")
}

pub fn error_log_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("claude-tools")
        .join("errors.log")
}

fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(p)
}

#[allow(dead_code)]
pub fn ensure_parent(p: &Path) -> Result<()> {
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
    }
    Ok(())
}
