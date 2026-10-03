use std::{fs, io::Write, path::PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use windows::{
    Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
    core::PCWSTR,
};

use crate::i18n::Language;
use crate::rules::ApplicationRule;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub automatic_application: bool,
    pub startup_enabled: bool,
    pub language: Language,
    pub theme: String,
    pub applications: Vec<ApplicationRule>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 3,
            automatic_application: true,
            startup_enabled: true,
            language: Language::default(),
            theme: "system".to_owned(),
            applications: Vec::new(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        migrate_legacy_config(&path);
        match fs::read_to_string(&path) {
            Ok(contents) => {
                let mut config = serde_json::from_str(&contents).unwrap_or_else(|error| {
                    eprintln!("Could not parse {}: {error}", path.display());
                    Self::default()
                });
                config.version = 3;
                config
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                eprintln!("Could not read {}: {error}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path();
        let parent = path.parent().context("configuration path has no parent")?;
        fs::create_dir_all(parent).context("create configuration directory")?;
        let temp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self).context("serialize configuration")?;
        let mut file = fs::File::create(&temp).context("create temporary configuration")?;
        file.write_all(&bytes)
            .context("write temporary configuration")?;
        file.sync_all().context("flush temporary configuration")?;
        let temp_wide: Vec<u16> = temp
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let path_wide: Vec<u16> = path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            MoveFileExW(
                PCWSTR(temp_wide.as_ptr()),
                PCWSTR(path_wide.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
            .context("replace configuration")?;
        }
        Ok(())
    }
}

pub fn config_path() -> PathBuf {
    ProjectDirs::from("app", "Bald", "Bald")
        .map(|dirs| dirs.config_dir().join("config.json"))
        .unwrap_or_else(|| PathBuf::from("bald-config.json"))
}

fn migrate_legacy_config(new_path: &PathBuf) {
    if new_path.exists() {
        return;
    }
    let Some(old_path) = ProjectDirs::from("app", "Baldless", "Baldless")
        .map(|dirs| dirs.config_dir().join("config.json"))
    else {
        return;
    };
    if !old_path.exists() {
        return;
    }
    if let Some(parent) = new_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::copy(old_path, new_path);
}
