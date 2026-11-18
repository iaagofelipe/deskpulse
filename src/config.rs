use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppConfig {
    pub port: u16,
    pub token: String,
    pub approval_timeout_secs: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            port: 8765,
            token: generate_token(),
            approval_timeout_secs: 45,
        }
    }
}

pub fn default_config_path() -> PathBuf {
    let base = env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("DeskPulse").join("config.json")
}

pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn load_or_create(path: &Path) -> Result<AppConfig, String> {
    if path.exists() {
        let raw = fs::read_to_string(path)
            .map_err(|e| format!("failed to read config {}: {e}", path.display()))?;
        let cfg: AppConfig = serde_json::from_str(&raw)
            .map_err(|e| format!("failed to parse config {}: {e}", path.display()))?;
        return Ok(cfg);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create config dir {}: {e}", parent.display()))?;
    }

    let cfg = AppConfig::default();
    let json = serde_json::to_string_pretty(&cfg)
        .map_err(|e| format!("failed to serialize config: {e}"))?;

    let tmp_name = format!(
        ".{}.tmp.{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("config.json"),
        std::process::id()
    );
    let tmp_path = path.with_file_name(tmp_name);
    fs::write(&tmp_path, json.as_bytes())
        .map_err(|e| format!("failed to write temp config {}: {e}", tmp_path.display()))?;
    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("failed to rename temp config to {}: {e}", path.display()));
    }

    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_load_or_create_generates_64_hex_token_and_reuses_it() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("DeskPulse").join("config.json");

        let first = load_or_create(&path).expect("first load_or_create");
        assert_eq!(first.port, 8765);
        assert_eq!(first.approval_timeout_secs, 45);
        assert_eq!(first.token.len(), 64);
        assert!(first.token.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(path.exists());

        let second = load_or_create(&path).expect("second load_or_create");
        assert_eq!(second, first);
    }
}
