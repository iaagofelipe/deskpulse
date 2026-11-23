use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::watch;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WeatherConfig {
    #[serde(default)]
    pub city: String,
    #[serde(default)]
    pub latitude: Option<f64>,
    #[serde(default)]
    pub longitude: Option<f64>,
    #[serde(default = "default_weather_unit")]
    pub unit: String,
}

fn default_weather_unit() -> String {
    "c".to_string()
}

impl Default for WeatherConfig {
    fn default() -> Self {
        Self {
            city: String::new(),
            latitude: None,
            longitude: None,
            unit: default_weather_unit(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GithubConfig {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub token_env: Option<String>,
}

impl GithubConfig {
    #[allow(dead_code)]
    pub fn resolve_token(&self) -> Option<String> {
        if let Some(ref t) = self.token {
            let trimmed = t.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }

        let var_name = self
            .token_env
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("GITHUB_TOKEN");

        match env::var(var_name) {
            Ok(val) => {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    Some(trimmed.to_string())
                } else {
                    None
                }
            }
            Err(_) => None,
        }
    }
}

fn default_ai_provider() -> String {
    "antigravity".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiConfig {
    #[serde(default = "default_ai_provider")]
    pub provider: String,
    #[serde(default)]
    pub budget_5h_tokens: Option<u64>,
    #[serde(default)]
    pub budget_weekly_tokens: Option<u64>,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: default_ai_provider(),
            budget_5h_tokens: None,
            budget_weekly_tokens: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShortcutAction {
    ToggleMic,
    ToggleMute,
    VolUp,
    VolDown,
    MediaPlayPause,
    MediaNext,
    MediaPrev,
    LockPc,
    Hotkey {
        keys: String,
    },
    App {
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
    Url {
        url: String,
    },
    Folder {
        path: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShortcutConfig {
    pub id: String,
    pub label: String,
    pub icon: String,
    pub color: String,
    pub action: ShortcutAction,
}

pub fn default_shortcuts() -> Vec<ShortcutConfig> {
    vec![
        ShortcutConfig {
            id: "mute_mic".to_string(),
            label: "Mutar Mic".to_string(),
            icon: "mic".to_string(),
            color: "#ef4444".to_string(),
            action: ShortcutAction::ToggleMic,
        },
        ShortcutConfig {
            id: "mute_sound".to_string(),
            label: "Mutar Som".to_string(),
            icon: "volume-x".to_string(),
            color: "#f59e0b".to_string(),
            action: ShortcutAction::ToggleMute,
        },
        ShortcutConfig {
            id: "vol_down".to_string(),
            label: "Vol -".to_string(),
            icon: "volume-1".to_string(),
            color: "#3b82f6".to_string(),
            action: ShortcutAction::VolDown,
        },
        ShortcutConfig {
            id: "vol_up".to_string(),
            label: "Vol +".to_string(),
            icon: "volume-2".to_string(),
            color: "#3b82f6".to_string(),
            action: ShortcutAction::VolUp,
        },
        ShortcutConfig {
            id: "screenshot".to_string(),
            label: "Captura".to_string(),
            icon: "camera".to_string(),
            color: "#8b5cf6".to_string(),
            action: ShortcutAction::Hotkey {
                keys: "Win+Shift+S".to_string(),
            },
        },
        ShortcutConfig {
            id: "terminal".to_string(),
            label: "Terminal".to_string(),
            icon: "terminal".to_string(),
            color: "#10b981".to_string(),
            action: ShortcutAction::App {
                program: "wt.exe".to_string(),
                args: Vec::new(),
            },
        },
        ShortcutConfig {
            id: "downloads".to_string(),
            label: "Downloads".to_string(),
            icon: "folder".to_string(),
            color: "#06b6d4".to_string(),
            action: ShortcutAction::Folder {
                path: r"%USERPROFILE%\Downloads".to_string(),
            },
        },
        ShortcutConfig {
            id: "lock_pc".to_string(),
            label: "Bloquear PC".to_string(),
            icon: "lock".to_string(),
            color: "#64748b".to_string(),
            action: ShortcutAction::LockPc,
        },
    ]
}

pub fn default_home_widgets() -> Vec<String> {
    vec![
        "ai".to_string(),
        "weather".to_string(),
        "agenda".to_string(),
        "media".to_string(),
        "github".to_string(),
    ]
}

fn default_night_mode_enabled() -> bool {
    true
}
fn default_night_mode_start_hour() -> u8 {
    23
}
fn default_night_mode_end_hour() -> u8 {
    7
}
fn default_night_mode_dim_locked() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NightModeConfig {
    #[serde(default = "default_night_mode_enabled")]
    pub enabled: bool,
    #[serde(default = "default_night_mode_start_hour")]
    pub start_hour: u8,
    #[serde(default = "default_night_mode_end_hour")]
    pub end_hour: u8,
    #[serde(default = "default_night_mode_dim_locked")]
    pub dim_when_pc_locked: bool,
}

impl Default for NightModeConfig {
    fn default() -> Self {
        Self {
            enabled: default_night_mode_enabled(),
            start_hour: default_night_mode_start_hour(),
            end_hour: default_night_mode_end_hour(),
            dim_when_pc_locked: default_night_mode_dim_locked(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    pub port: u16,
    pub token: String,
    pub approval_timeout_secs: u64,
    #[serde(default)]
    pub calendar_ics_url: Option<String>,
    #[serde(default)]
    pub remote_approval_enabled: bool,
    #[serde(default)]
    pub weather: WeatherConfig,
    #[serde(default)]
    pub github: GithubConfig,
    #[serde(default)]
    pub ai: AiConfig,
    #[serde(default = "default_shortcuts")]
    pub shortcuts: Vec<ShortcutConfig>,
    #[serde(default = "default_home_widgets")]
    pub home_widgets: Vec<String>,
    #[serde(default)]
    pub night_mode: NightModeConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            port: 8765,
            token: generate_token(),
            approval_timeout_secs: 45,
            calendar_ics_url: None,
            remote_approval_enabled: false,
            weather: WeatherConfig::default(),
            github: GithubConfig::default(),
            ai: AiConfig::default(),
            shortcuts: default_shortcuts(),
            home_widgets: default_home_widgets(),
            night_mode: NightModeConfig::default(),
        }
    }
}

pub fn deskpulse_data_dir() -> PathBuf {
    let base = env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("DeskPulse")
}

pub fn default_config_path() -> PathBuf {
    deskpulse_data_dir().join("config.json")
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

    let cfg = AppConfig::default();
    save_config(path, &cfg)?;
    Ok(cfg)
}

pub fn save_config(path: &Path, cfg: &AppConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create config dir {}: {e}", parent.display()))?;
    }

    let json = serde_json::to_string_pretty(cfg)
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

    let mut last_err = None;
    for attempt in 0..5 {
        match fs::rename(&tmp_path, path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                if attempt < 4 {
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
        }
    }

    let _ = fs::remove_file(&tmp_path);
    Err(format!(
        "failed to rename temp config to {}: {}",
        path.display(),
        last_err.expect("rename failed at least once")
    ))
}

#[allow(dead_code)]
pub struct ConfigWatcher;

impl ConfigWatcher {
    pub fn spawn(
        path: PathBuf,
        initial: AppConfig,
        shutdown: watch::Receiver<bool>,
    ) -> (Arc<RwLock<AppConfig>>, watch::Receiver<AppConfig>) {
        Self::spawn_with_interval(path, initial, shutdown, Duration::from_secs(3))
    }

    pub fn spawn_with_interval(
        path: PathBuf,
        initial: AppConfig,
        mut shutdown: watch::Receiver<bool>,
        interval: Duration,
    ) -> (Arc<RwLock<AppConfig>>, watch::Receiver<AppConfig>) {
        let (tx, rx) = watch::channel(initial.clone());
        let live = Arc::new(RwLock::new(initial));
        let live_clone = Arc::clone(&live);
        let mut last_content: Option<String> = fs::read_to_string(&path).ok();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.tick().await;

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        if *shutdown.borrow() {
                            break;
                        }
                        if let Ok(raw) = fs::read_to_string(&path) {
                            if last_content.as_deref() != Some(&raw) {
                                match serde_json::from_str::<AppConfig>(&raw) {
                                    Ok(cfg) => {
                                        last_content = Some(raw);
                                        if let Ok(mut g) = live_clone.write() {
                                            *g = cfg.clone();
                                        }
                                        let _ = tx.send(cfg);
                                    }
                                    Err(err) => {
                                        eprintln!(
                                            "Aviso: falha ao recarregar config {}: {err}",
                                            path.display()
                                        );
                                        last_content = Some(raw);
                                    }
                                }
                            }
                        }
                    }
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                }
            }
        });

        (live, rx)
    }
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(windows)]
fn run_reg(args: &[&str]) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new("reg.exe");
    cmd.args(args);
    cmd.creation_flags(CREATE_NO_WINDOW);
    let out = cmd.output().map_err(|e| format!("failed to run reg.exe: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(windows)]
fn resolve_current_exe_command() -> Result<String, String> {
    let exe = env::current_exe().map_err(|e| format!("failed to get current_exe: {e}"))?;
    Ok(format!("\"{}\" serve", exe.to_string_lossy()))
}

pub fn startup_status() -> bool {
    #[cfg(windows)]
    {
        if let Ok(out) = run_reg(&[
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "DeskPulse",
        ]) {
            return out.contains("DeskPulse");
        }
        false
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn install_startup() -> Result<(), String> {
    #[cfg(windows)]
    {
        let cmd_str = resolve_current_exe_command()?;
        run_reg(&[
            "add",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "DeskPulse",
            "/t",
            "REG_SZ",
            "/d",
            &cmd_str,
            "/f",
        ])
        .map(|_| ())
    }
    #[cfg(not(windows))]
    {
        Err("Auto-start is only supported on Windows".to_string())
    }
}

pub fn uninstall_startup() -> Result<(), String> {
    #[cfg(windows)]
    {
        let res = run_reg(&[
            "delete",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "DeskPulse",
            "/f",
        ]);
        if let Err(ref err) = res {
            if err.contains("não pôde encontrar")
                || err.contains("unable to find")
                || err.contains("not find")
            {
                return Ok(());
            }
        }
        res.map(|_| ())
    }
    #[cfg(not(windows))]
    {
        Err("Auto-start is only supported on Windows".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_deskpulse_data_dir_ends_with_deskpulse() {
        let dir = deskpulse_data_dir();
        assert!(dir.ends_with("DeskPulse"));
    }

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

    #[test]
    fn test_load_legacy_config_defaults_new_fields_and_save_config_persists() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("DeskPulse").join("config.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{"port":8765,"token":"abc123","approval_timeout_secs":45}"#,
        )
        .unwrap();

        let mut cfg = load_or_create(&path).expect("legacy config should load");
        assert_eq!(cfg.calendar_ics_url, None);
        assert!(!cfg.remote_approval_enabled);

        cfg.calendar_ics_url = Some("https://calendar.google.com/calendar/ical/test.ics".to_string());
        cfg.remote_approval_enabled = true;
        save_config(&path, &cfg).expect("save_config");

        let reloaded = load_or_create(&path).expect("reload saved config");
        assert_eq!(reloaded, cfg);
    }

    #[test]
    fn test_v2_defaults_populate_8_shortcuts_and_home_widgets_on_legacy_json() {
        let raw = r#"{"port":8765,"token":"abc","approval_timeout_secs":45}"#;
        let cfg: AppConfig = serde_json::from_str(raw).expect("deserialize legacy json");
        assert_eq!(cfg.shortcuts.len(), 8);
        assert_eq!(
            cfg.home_widgets,
            vec!["ai", "weather", "agenda", "media", "github"]
        );
        assert_eq!(cfg.weather.unit, "c");
        assert_eq!(cfg.ai.provider, "antigravity");
        assert!(cfg.night_mode.enabled);
        assert!(cfg.night_mode.dim_when_pc_locked);
    }

    #[test]
    fn test_github_resolve_token_prefers_explicit_then_env() {
        let test_env_var = "DESKPULSE_TEST_GH_TOKEN_TMP";
        std::env::set_var(test_env_var, "env_secret_token_123");

        // Explicit non-empty overrides env
        let cfg = GithubConfig {
            username: Some("octocat".to_string()),
            token: Some("explicit_token_456".to_string()),
            token_env: Some(test_env_var.to_string()),
        };
        assert_eq!(cfg.resolve_token(), Some("explicit_token_456".to_string()));

        // Trimming blanks in explicit falls back to env
        let cfg_blank = GithubConfig {
            username: None,
            token: Some("   ".to_string()),
            token_env: Some(test_env_var.to_string()),
        };
        assert_eq!(
            cfg_blank.resolve_token(),
            Some("env_secret_token_123".to_string())
        );

        // Blank in both returns None
        std::env::set_var("DESKPULSE_TEST_EMPTY_ENV", "   ");
        let cfg_all_blank = GithubConfig {
            username: None,
            token: Some("   ".to_string()),
            token_env: Some("DESKPULSE_TEST_EMPTY_ENV".to_string()),
        };
        assert_eq!(cfg_all_blank.resolve_token(), None);

        // Explicit with whitespace is trimmed
        let cfg_untrimmed = GithubConfig {
            username: None,
            token: Some("  trimmed_token  ".to_string()),
            token_env: None,
        };
        assert_eq!(
            cfg_untrimmed.resolve_token(),
            Some("trimmed_token".to_string())
        );

        std::env::remove_var(test_env_var);
        std::env::remove_var("DESKPULSE_TEST_EMPTY_ENV");
    }

    #[tokio::test]
    async fn test_config_watcher_reloads_valid_changes_and_ignores_invalid_json() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("config.json");
        let mut initial = AppConfig::default();
        initial.port = 9000;
        save_config(&path, &initial).expect("save initial");

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let (live, rx) = ConfigWatcher::spawn_with_interval(
            path.clone(),
            initial.clone(),
            shutdown_rx,
            Duration::from_millis(20),
        );

        assert_eq!(live.read().unwrap().port, 9000);

        // 1. Write valid update
        let mut updated = initial.clone();
        updated.port = 9111;
        std::thread::sleep(Duration::from_millis(30));
        save_config(&path, &updated).expect("save updated");

        // Wait for watcher to pick up update
        let timeout = Duration::from_secs(2);
        let start = std::time::Instant::now();
        loop {
            if rx.borrow().port == 9111 && live.read().unwrap().port == 9111 {
                break;
            }
            if start.elapsed() > timeout {
                panic!("timed out waiting for config watcher to reload valid change");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        // 2. Write invalid broken JSON
        std::thread::sleep(Duration::from_millis(30));
        fs::write(&path, b"{broken json").expect("write broken");

        // Allow watcher multiple ticks to process
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Verify last valid config is preserved without panic
        assert_eq!(live.read().unwrap().port, 9111);
        assert_eq!(rx.borrow().port, 9111);

        let _ = shutdown_tx.send(true);
    }

    #[tokio::test]
    async fn test_config_watcher_spawn_interface() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("config.json");
        let initial = AppConfig::default();
        save_config(&path, &initial).expect("save initial");

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let (live, rx) = ConfigWatcher::spawn(path, initial.clone(), shutdown_rx);
        assert_eq!(live.read().unwrap().port, initial.port);
        assert_eq!(rx.borrow().port, initial.port);
        let _ = shutdown_tx.send(true);
    }
}
