use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::storage::atomic_write_private;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default)]
    pub quota_disclosure_acknowledged: bool,
}

pub fn preferences_path() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("", "", "codex-account-hub")
        .context("resolve the platform application config directory")?;
    Ok(dirs.config_dir().join("prefs.json"))
}

pub fn load() -> Result<Preferences> {
    let path = preferences_path()?;
    match fs::read(&path) {
        Ok(data) => serde_json::from_slice(&data)
            .with_context(|| format!("parse preferences at {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Preferences::default()),
        Err(error) => Err(error).with_context(|| format!("read preferences at {}", path.display())),
    }
}

pub fn save(preferences: &Preferences) -> Result<()> {
    let path = preferences_path()?;
    let data = serde_json::to_vec_pretty(preferences)?;
    atomic_write_private(&path, &data)
}
