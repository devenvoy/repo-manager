use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

/// Settings used by the first desktop milestone. Credentials are deliberately
/// absent: a later authentication service will use the OS credential store.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub snapshot_path: String,
    pub include_prereleases: bool,
    pub show_archived: bool,
    pub installer_extension: String,
    pub appearance: Appearance,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            snapshot_path: String::new(),
            include_prereleases: true,
            show_archived: false,
            installer_extension: "dmg".into(),
            appearance: Appearance::System,
        }
    }
}

pub fn settings_path() -> io::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
        Ok(PathBuf::from(home).join("Library/Application Support/Release Desk/settings.json"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            return Ok(PathBuf::from(xdg).join("release-desk/settings.json"));
        }
        let home = std::env::var_os("HOME")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
        Ok(PathBuf::from(home).join(".config/release-desk/settings.json"))
    }
}

pub fn load() -> Result<Settings, String> {
    let path = settings_path().map_err(|e| format!("Cannot locate settings: {e}"))?;
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(e) => return Err(format!("Cannot read {}: {e}", path.display())),
    };
    serde_json::from_str(&text).map_err(|e| format!("Invalid {}: {e}", path.display()))
}

pub fn save(settings: &Settings) -> Result<(), String> {
    let path = settings_path().map_err(|e| format!("Cannot locate settings: {e}"))?;
    let parent = path
        .parent()
        .ok_or_else(|| "Settings path has no parent directory".to_string())?;
    fs::create_dir_all(parent).map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
    let bytes = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, bytes).map_err(|e| format!("Cannot write {}: {e}", temp.display()))?;
    fs::rename(&temp, &path).map_err(|e| format!("Cannot save {}: {e}", path.display()))
}

pub fn normalized_extension(input: &str) -> Result<String, String> {
    let value = input.trim().trim_start_matches('.').to_ascii_lowercase();
    if value.is_empty() || value.len() > 12 || !value.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err("Installer extension must contain 1–12 letters or digits".into());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_is_validated() {
        assert_eq!(normalized_extension(" .DMG ").unwrap(), "dmg");
        assert!(normalized_extension("../zip").is_err());
        assert!(normalized_extension("").is_err());
    }
}
