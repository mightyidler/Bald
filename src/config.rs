use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use windows::{
    Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
    core::PCWSTR,
};

use crate::i18n::Language;
use crate::rules::ApplicationRule;

static CONFIG_WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        self.save_to(&config_path())
    }

    pub(crate) fn save_to(&self, path: &Path) -> Result<()> {
        let _write = CONFIG_WRITES.lock().unwrap();
        let parent = path.parent().context("configuration path has no parent")?;
        fs::create_dir_all(parent).context("create configuration directory")?;
        let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        struct TemporaryFile(PathBuf);
        impl Drop for TemporaryFile {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
            }
        }
        let cleanup = TemporaryFile(temp.clone());
        let bytes = serde_json::to_vec_pretty(self).context("serialize configuration")?;
        let mut file = fs::File::create(&temp).context("create temporary configuration")?;
        file.write_all(&bytes)
            .context("write temporary configuration")?;
        file.sync_all().context("flush temporary configuration")?;
        drop(file);
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
        drop(cleanup);
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_saves_are_complete_json_and_leave_no_temporary_files() {
        let directory = std::env::temp_dir().join(format!("bald-save-{}", uuid::Uuid::new_v4()));
        let path = directory.join("config.json");
        std::thread::scope(|scope| {
            for index in 0..12 {
                let path = &path;
                scope.spawn(move || {
                    Config {
                        theme: format!("test-{index}"),
                        ..Config::default()
                    }
                    .save_to(path)
                    .unwrap();
                });
            }
        });
        let saved: Config = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(saved.theme.starts_with("test-"));
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failed_replace_keeps_destination_and_cleans_temporary_file() {
        let directory = std::env::temp_dir().join(format!("bald-save-{}", uuid::Uuid::new_v4()));
        let path = directory.join("config.json");
        fs::create_dir_all(&path).unwrap();
        assert!(Config::default().save_to(&path).is_err());
        assert!(path.is_dir());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }
}
