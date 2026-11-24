use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::theme::{LayoutMode, ThemeId};

pub const DEFAULT_SERVER: &str = "https://muzyka.komodaoskara.pl";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionConfig {
    pub server_url: String,
    pub token: String,
    pub username: String,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default = "default_volume")]
    pub volume: u8,
    #[serde(default)]
    pub theme: ThemeId,
    #[serde(default)]
    pub layout: LayoutMode,
    #[serde(default = "default_device_id")]
    pub device_id: String,
}

fn default_volume() -> u8 {
    85
}

fn host_name() -> String {
    #[cfg(windows)]
    {
        std::env::var("COMPUTERNAME")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "Windows".to_string())
    }
    #[cfg(not(windows))]
    {
        fs::read_to_string("/etc/hostname")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("HOSTNAME").ok())
            .unwrap_or_else(|| "Linux".to_string())
    }
}

pub fn default_device_id() -> String {
    let host = host_name();
    let clean: String = host
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    format!("swcli_{}", if clean.is_empty() { "pc" } else { &clean })
}

pub fn device_display_name() -> String {
    format!("SoundWave CLI ({})", host_name())
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            server_url: DEFAULT_SERVER.to_string(),
            token: String::new(),
            username: String::new(),
            is_admin: false,
            volume: default_volume(),
            theme: ThemeId::default(),
            layout: LayoutMode::default(),
            device_id: default_device_id(),
        }
    }
}

pub fn config_path() -> PathBuf {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));

    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));

    base.join("soundwave-cli").join("session.json")
}

pub fn load_session() -> Option<SessionConfig> {
    let path = config_path();
    let raw = fs::read_to_string(path).ok()?;
    let mut cfg: SessionConfig = serde_json::from_str(&raw).ok()?;
    cfg.server_url = normalize_server(&cfg.server_url);
    if cfg.token.is_empty() {
        return None;
    }
    Some(cfg)
}

pub fn save_session(cfg: &SessionConfig) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Nie można utworzyć katalogu config: {e}"))?;
    }
    let json = serde_json::to_string_pretty(cfg)
        .map_err(|e| format!("Błąd serializacji sesji: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("Nie można zapisać sesji: {e}"))?;
    #[cfg(unix)]
    {
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn clear_session() {
    let _ = fs::remove_file(config_path());
}

pub fn normalize_server(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        DEFAULT_SERVER.to_string()
    } else if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        format!("https://{trimmed}")
    } else {
        trimmed.to_string()
    }
}
