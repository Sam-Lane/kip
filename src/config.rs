use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    pub username: Option<String>,
    pub token: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WatcherConfig {
    pub name: String,
    pub repo: String,
    pub reference: String,
    pub working_dir: Option<PathBuf>,
    #[serde(default = "default_interval")]
    pub interval: u64,
    pub command: String,
    pub auth: Option<AuthConfig>,
}

fn default_interval() -> u64 {
    60
}

#[derive(Debug, Deserialize, Default)]
pub struct KipConfig {
    #[serde(default)]
    pub watcher: Vec<WatcherConfig>,
}

impl KipConfig {
    fn merge(&mut self, other: KipConfig) {
        self.watcher.extend(other.watcher);
    }
}

fn load_file(path: &Path) -> Result<KipConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&content).with_context(|| format!("parsing {}", path.display()))
}

pub fn load_config(override_path: Option<PathBuf>) -> Result<KipConfig> {
    if let Some(path) = override_path {
        return load_file(&path);
    }

    let mut config = KipConfig::default();
    let mut found = false;

    let default_file = Path::new("/etc/kip/config.toml");
    if default_file.exists() {
        config.merge(load_file(default_file)?);
        found = true;
    }

    let conf_d = Path::new("/etc/kip/conf.d");
    if conf_d.is_dir() {
        let pattern = conf_d.join("*.toml");
        let pattern_str = pattern.to_string_lossy();
        for entry in glob::glob(&pattern_str)
            .context("glob /etc/kip/conf.d/*.toml")?
            .flatten()
        {
            config.merge(load_file(&entry)?);
            found = true;
        }
    }

    anyhow::ensure!(
        found,
        "no config found — provide --config-path, set KIP_CONFIG, or create /etc/kip/config.toml"
    );

    Ok(config)
}
