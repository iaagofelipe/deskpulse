use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

use crate::config::{ShortcutAction, ShortcutConfig};
use crate::monitors::media::{MediaBackend, MediaSnapshot};

pub const MAX_SHORTCUT_TILES: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShortcutTileState {
    pub id: String,
    pub label: String,
    pub icon: String,
    pub color: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ShortcutError {
    NotFound(String),
    Invalid(String),
    Failed(String),
}

impl std::fmt::Display for ShortcutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShortcutError::NotFound(id) => write!(f, "Atalho não encontrado: {id}"),
            ShortcutError::Invalid(msg) => write!(f, "Atalho inválido: {msg}"),
            ShortcutError::Failed(msg) => write!(f, "Falha ao executar atalho: {msg}"),
        }
    }
}

impl std::error::Error for ShortcutError {}

#[allow(dead_code)]
pub fn parse_hotkey_vks(combo: &str) -> Result<Vec<u16>, String> {
    let trimmed = combo.trim();
    if trimmed.is_empty() {
        return Err("combinação de teclas vazia".to_string());
    }

    let parts: Vec<&str> = trimmed.split('+').map(str::trim).collect();
    if parts.is_empty() {
        return Err("combinação de teclas vazia".to_string());
    }

    let mut vks = Vec::with_capacity(parts.len());
    for part in parts {
        if part.is_empty() {
            return Err("token de tecla vazio na combinação".to_string());
        }
        let vk = parse_single_vk(part)?;
        vks.push(vk);
    }
    Ok(vks)
}

fn parse_single_vk(token: &str) -> Result<u16, String> {
    let lower = token.to_ascii_lowercase();
    match lower.as_str() {
        "win" | "lwin" => Ok(0x5B), // VK_LWIN
        "rwin" => Ok(0x5C),         // VK_RWIN
        "ctrl" | "control" | "lctrl" | "rctrl" => Ok(0x11), // VK_CONTROL
        "alt" | "lalt" | "ralt" | "menu" => Ok(0x12),       // VK_MENU
        "shift" | "lshift" | "rshift" => Ok(0x10),  // VK_SHIFT
        "tab" => Ok(0x09),                          // VK_TAB
        "enter" | "return" => Ok(0x0D),             // VK_RETURN
        "esc" | "escape" => Ok(0x1B),               // VK_ESCAPE
        "space" => Ok(0x20),                        // VK_SPACE
        "printscreen" | "prtsc" | "print_screen" | "snapshot" => Ok(0x2C), // VK_SNAPSHOT
        "up" => Ok(0x26),                           // VK_UP
        "down" => Ok(0x28),                         // VK_DOWN
        "left" => Ok(0x25),                         // VK_LEFT
        "right" => Ok(0x27),                        // VK_RIGHT
        "home" => Ok(0x24),                         // VK_HOME
        "end" => Ok(0x23),                          // VK_END
        "pageup" | "pgup" => Ok(0x21),              // VK_PRIOR
        "pagedown" | "pgdn" => Ok(0x22),            // VK_NEXT
        "delete" | "del" => Ok(0x2E),               // VK_DELETE
        "backspace" | "bksp" => Ok(0x08),           // VK_BACK
        "mediaplaypause" | "playpause" => Ok(0xB3), // VK_MEDIA_PLAY_PAUSE
        "medianext" | "nexttrack" => Ok(0xB0),      // VK_MEDIA_NEXT_TRACK
        "mediaprev" | "prevtrack" => Ok(0xB1),      // VK_MEDIA_PREV_TRACK
        _ => {
            if lower.starts_with('f') {
                if let Ok(num) = lower[1..].parse::<u16>() {
                    if (1..=12).contains(&num) {
                        return Ok(0x70 + num - 1); // VK_F1..VK_F12 (0x70..0x7B)
                    } else if (13..=24).contains(&num) {
                        return Ok(0x7C + num - 13); // VK_F13..VK_F24 (0x7C..0x87)
                    }
                }
            }
            if token.len() == 1 {
                let ch = token.chars().next().unwrap();
                if ch.is_ascii_alphabetic() {
                    return Ok(ch.to_ascii_uppercase() as u16);
                } else if ch.is_ascii_digit() {
                    return Ok(ch as u16);
                }
            }
            Err(format!("tecla desconhecida: '{token}'"))
        }
    }
}

#[allow(dead_code)]
pub fn validate_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("URL não pode ser vazia".to_string());
    }
    if trimmed != url || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("URL não pode conter espaços ou caracteres de controle".to_string());
    }
    let rest = if let Some(r) = url.strip_prefix("http://") {
        r
    } else if let Some(r) = url.strip_prefix("https://") {
        r
    } else {
        return Err("URL deve começar com http:// ou https://".to_string());
    };
    let host_part = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = if let Some(pos) = host_part.find('@') {
        &host_part[pos + 1..]
    } else {
        host_part
    };
    let host = host.split(':').next().unwrap_or("");
    if host.is_empty() {
        return Err("URL deve conter um host válido".to_string());
    }
    Ok(())
}

#[allow(dead_code)]
pub fn expand_env_vars(path: &str) -> String {
    let mut res = path.to_string();
    let vars = ["USERPROFILE", "APPDATA", "LOCALAPPDATA", "TEMP", "HOME"];
    for var in vars {
        let pattern = format!("%{}%", var);
        let upper_res = res.to_ascii_uppercase();
        if upper_res.contains(&pattern) {
            let val = std::env::var(var).ok().or_else(|| {
                if var == "HOME" {
                    std::env::var("USERPROFILE").ok()
                } else {
                    None
                }
            });
            if let Some(v) = val {
                let mut start = 0;
                let mut new_res = String::new();
                while let Some(pos) = upper_res[start..].find(&pattern) {
                    let match_idx = start + pos;
                    new_res.push_str(&res[start..match_idx]);
                    new_res.push_str(&v);
                    start = match_idx + pattern.len();
                }
                new_res.push_str(&res[start..]);
                res = new_res;
            }
        }
    }
    res
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(windows)]
fn send_hotkey(vks: &[u16]) -> Result<(), ShortcutError> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };

    let mut inputs = Vec::with_capacity(vks.len() * 2);
    // Key presses in forward order
    for &vk in vks {
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }
    // Key releases in reverse order
    for &vk in vks.iter().rev() {
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }

    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent != inputs.len() as u32 {
        return Err(ShortcutError::Failed(
            "Falha ao injetar sequência de teclas via SendInput".to_string(),
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn send_hotkey(_vks: &[u16]) -> Result<(), ShortcutError> {
    Err(ShortcutError::Failed(
        "Envio de teclas suportado apenas no Windows".to_string(),
    ))
}

fn lock_pc() -> Result<(), ShortcutError> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("rundll32.exe");
        cmd.args(["user32.dll,LockWorkStation"]);
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd.spawn()
            .map_err(|e| ShortcutError::Failed(format!("Falha ao bloquear PC: {e}")))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err(ShortcutError::Failed(
            "Bloqueio de PC suportado apenas no Windows".to_string(),
        ))
    }
}

fn launch_app(program: &str, args: &[String]) -> Result<(), ShortcutError> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    cmd.spawn()
        .map_err(|e| ShortcutError::Failed(format!("Falha ao iniciar aplicativo '{program}': {e}")))?;
    Ok(())
}

fn open_url(url: &str) -> Result<(), ShortcutError> {
    validate_url(url).map_err(ShortcutError::Invalid)?;
    let mut cmd = std::process::Command::new("explorer.exe");
    cmd.arg(url);
    cmd.spawn()
        .map_err(|e| ShortcutError::Failed(format!("Falha ao abrir URL '{url}': {e}")))?;
    Ok(())
}

fn open_folder(path: &str) -> Result<(), ShortcutError> {
    let expanded = expand_env_vars(path);
    let p = std::path::Path::new(&expanded);
    if !p.exists() {
        return Err(ShortcutError::Invalid(format!(
            "Pasta não encontrada: {expanded}"
        )));
    }
    let mut cmd = std::process::Command::new("explorer.exe");
    cmd.arg(&expanded);
    cmd.spawn()
        .map_err(|e| ShortcutError::Failed(format!("Falha ao abrir pasta '{expanded}': {e}")))?;
    Ok(())
}

#[allow(dead_code)]
pub struct ShortcutRunner {
    tiles: Arc<RwLock<Vec<ShortcutConfig>>>,
}

impl ShortcutRunner {
    #[allow(dead_code)]
    pub fn new(mut initial: Vec<ShortcutConfig>) -> Self {
        initial.truncate(MAX_SHORTCUT_TILES);
        Self {
            tiles: Arc::new(RwLock::new(initial)),
        }
    }

    #[allow(dead_code)]
    pub fn update_config(&self, mut tiles: Vec<ShortcutConfig>) {
        tiles.truncate(MAX_SHORTCUT_TILES);
        if let Ok(mut g) = self.tiles.write() {
            *g = tiles;
        }
    }

    #[allow(dead_code)]
    pub fn tile_states(&self, media: &MediaSnapshot) -> Vec<ShortcutTileState> {
        let tiles = self.tiles.read().unwrap_or_else(|e| e.into_inner());
        tiles
            .iter()
            .map(|t| {
                let active = match &t.action {
                    ShortcutAction::ToggleMic => media.mic_muted,
                    ShortcutAction::ToggleMute => media.muted,
                    _ => false,
                };
                ShortcutTileState {
                    id: t.id.clone(),
                    label: t.label.clone(),
                    icon: t.icon.clone(),
                    color: t.color.clone(),
                    active,
                }
            })
            .collect()
    }

    #[allow(dead_code)]
    pub async fn trigger(
        &self,
        id: &str,
        media: &Arc<dyn MediaBackend>,
    ) -> Result<Option<MediaSnapshot>, ShortcutError> {
        let action = {
            let tiles = self.tiles.read().map_err(|_| {
                ShortcutError::Failed("Falha de concorrência ao ler atalhos".to_string())
            })?;
            tiles
                .iter()
                .find(|t| t.id == id)
                .map(|t| t.action.clone())
                .ok_or_else(|| ShortcutError::NotFound(id.to_string()))?
        };

        match action {
            ShortcutAction::ToggleMic => {
                let snap = media
                    .execute(crate::monitors::media::MediaCommand::ToggleMicMute)
                    .await
                    .map_err(ShortcutError::Failed)?;
                Ok(Some(snap))
            }
            ShortcutAction::ToggleMute => {
                let snap = media
                    .execute(crate::monitors::media::MediaCommand::ToggleMute)
                    .await
                    .map_err(ShortcutError::Failed)?;
                Ok(Some(snap))
            }
            ShortcutAction::VolUp => {
                let snap = media.snapshot().await;
                let new_vol = snap.volume_pct.saturating_add(5).min(100);
                let updated = media
                    .execute(crate::monitors::media::MediaCommand::SetVolume(new_vol))
                    .await
                    .map_err(ShortcutError::Failed)?;
                Ok(Some(updated))
            }
            ShortcutAction::VolDown => {
                let snap = media.snapshot().await;
                let new_vol = snap.volume_pct.saturating_sub(5);
                let updated = media
                    .execute(crate::monitors::media::MediaCommand::SetVolume(new_vol))
                    .await
                    .map_err(ShortcutError::Failed)?;
                Ok(Some(updated))
            }
            ShortcutAction::MediaPlayPause => {
                let snap = media
                    .execute(crate::monitors::media::MediaCommand::PlayPause)
                    .await
                    .map_err(ShortcutError::Failed)?;
                Ok(Some(snap))
            }
            ShortcutAction::MediaNext => {
                let snap = media
                    .execute(crate::monitors::media::MediaCommand::Next)
                    .await
                    .map_err(ShortcutError::Failed)?;
                Ok(Some(snap))
            }
            ShortcutAction::MediaPrev => {
                let snap = media
                    .execute(crate::monitors::media::MediaCommand::Previous)
                    .await
                    .map_err(ShortcutError::Failed)?;
                Ok(Some(snap))
            }
            ShortcutAction::LockPc => {
                lock_pc()?;
                Ok(None)
            }
            ShortcutAction::Hotkey { keys } => {
                let vks = parse_hotkey_vks(&keys).map_err(ShortcutError::Invalid)?;
                send_hotkey(&vks)?;
                Ok(None)
            }
            ShortcutAction::App { program, args } => {
                launch_app(&program, &args)?;
                Ok(None)
            }
            ShortcutAction::Url { url } => {
                open_url(&url)?;
                Ok(None)
            }
            ShortcutAction::Folder { path } => {
                open_folder(&path)?;
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitors::media::{MediaCommand, WindowsMediaBackend};
    use std::future::Future;
    use std::pin::Pin;

    struct MockMediaBackend {
        snap: std::sync::Mutex<MediaSnapshot>,
    }

    impl MockMediaBackend {
        fn new(snap: MediaSnapshot) -> Self {
            Self {
                snap: std::sync::Mutex::new(snap),
            }
        }
    }

    impl MediaBackend for MockMediaBackend {
        fn snapshot(&self) -> Pin<Box<dyn Future<Output = MediaSnapshot> + Send + '_>> {
            let snap = self.snap.lock().unwrap().clone();
            Box::pin(async move { snap })
        }

        fn execute(
            &self,
            cmd: MediaCommand,
        ) -> Pin<Box<dyn Future<Output = Result<MediaSnapshot, String>> + Send + '_>> {
            let mut snap = self.snap.lock().unwrap();
            match cmd {
                MediaCommand::ToggleMicMute => {
                    snap.mic_muted = !snap.mic_muted;
                }
                MediaCommand::ToggleMute => {
                    snap.muted = !snap.muted;
                }
                MediaCommand::SetVolume(v) => {
                    snap.volume_pct = v;
                }
                _ => {}
            }
            let res = snap.clone();
            Box::pin(async move { Ok(res) })
        }
    }

    #[test]
    fn test_parse_hotkey_vks_valid_and_rejects_unknown_keys() {
        let win_shift_s = parse_hotkey_vks("Win+Shift+S").expect("Win+Shift+S should succeed");
        assert_eq!(win_shift_s, vec![0x5B, 0x10, 0x53]);

        let ctrl_alt_f13 = parse_hotkey_vks("Ctrl+Alt+F13").expect("Ctrl+Alt+F13 should succeed");
        assert_eq!(ctrl_alt_f13, vec![0x11, 0x12, 0x7C]);

        assert!(parse_hotkey_vks("").is_err());
        assert!(parse_hotkey_vks("Ctrl+BogusKey").is_err());
    }

    #[test]
    fn test_validate_url_allows_http_https_and_rejects_file_or_cmd() {
        assert!(validate_url("https://github.com").is_ok());
        assert!(validate_url("file:///C:/Windows/cmd.exe").is_err());
        assert!(validate_url("javascript:alert(1)").is_err());
        assert!(validate_url("http://127.0.0.1:8765").is_ok());
        assert!(validate_url("").is_err());
        assert!(validate_url("   ").is_err());
        assert!(validate_url("https://").is_err());
    }

    #[test]
    fn test_tile_states_reflect_live_mic_and_sound_mute_without_leaking_actions() {
        let tiles = vec![
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
                id: "screenshot".to_string(),
                label: "Captura".to_string(),
                icon: "camera".to_string(),
                color: "#8b5cf6".to_string(),
                action: ShortcutAction::Hotkey {
                    keys: "Win+Shift+S".to_string(),
                },
            },
        ];

        let runner = ShortcutRunner::new(tiles);

        let mut media = MediaSnapshot::default();
        media.mic_muted = true;
        media.muted = false;

        let states1 = runner.tile_states(&media);
        assert_eq!(states1.len(), 3);
        assert_eq!(states1[0].id, "mute_mic");
        assert!(states1[0].active);
        assert_eq!(states1[1].id, "mute_sound");
        assert!(!states1[1].active);

        media.mic_muted = false;
        media.muted = true;
        let states2 = runner.tile_states(&media);
        assert!(!states2[0].active);
        assert!(states2[1].active);

        // Verify JSON serialization contains NO program, path, keys, or url
        let json = serde_json::to_string(&states2).expect("serialize tile states");
        assert!(!json.contains("program"));
        assert!(!json.contains("path"));
        assert!(!json.contains("keys"));
        assert!(!json.contains("url"));
        assert!(!json.contains("Win+Shift+S"));
    }

    #[tokio::test]
    async fn test_trigger_unknown_id_returns_not_found() {
        let runner = ShortcutRunner::new(vec![]);
        let media: Arc<dyn MediaBackend> = Arc::new(WindowsMediaBackend::new());
        let res = runner.trigger("nonexistent", &media).await;
        match res {
            Err(ShortcutError::NotFound(id)) => assert_eq!(id, "nonexistent"),
            other => panic!("expected Err(ShortcutError::NotFound), got {:?}", other),
        }
    }

    #[test]
    fn test_cap_tiles_at_16() {
        let mut tiles = Vec::new();
        for i in 0..20 {
            tiles.push(ShortcutConfig {
                id: format!("tile_{i}"),
                label: format!("Tile {i}"),
                icon: "icon".to_string(),
                color: "#000000".to_string(),
                action: ShortcutAction::ToggleMic,
            });
        }
        let runner = ShortcutRunner::new(tiles);
        let states = runner.tile_states(&MediaSnapshot::default());
        assert_eq!(states.len(), 16);

        let mut more_tiles = Vec::new();
        for i in 0..25 {
            more_tiles.push(ShortcutConfig {
                id: format!("more_{i}"),
                label: format!("More {i}"),
                icon: "icon".to_string(),
                color: "#000000".to_string(),
                action: ShortcutAction::ToggleMute,
            });
        }
        runner.update_config(more_tiles);
        let updated_states = runner.tile_states(&MediaSnapshot::default());
        assert_eq!(updated_states.len(), 16);
    }

    #[test]
    fn test_expand_env_vars() {
        let expanded = expand_env_vars("%USERPROFILE%\\test");
        assert!(!expanded.contains("%USERPROFILE%"));
        assert!(expanded.ends_with("\\test"));
    }

    #[tokio::test]
    async fn test_trigger_media_actions_with_mock() {
        let tiles = vec![
            ShortcutConfig {
                id: "mute_mic".to_string(),
                label: "Mutar Mic".to_string(),
                icon: "mic".to_string(),
                color: "#ef4444".to_string(),
                action: ShortcutAction::ToggleMic,
            },
            ShortcutConfig {
                id: "vol_up".to_string(),
                label: "Vol +".to_string(),
                icon: "volume-2".to_string(),
                color: "#3b82f6".to_string(),
                action: ShortcutAction::VolUp,
            },
            ShortcutConfig {
                id: "vol_down".to_string(),
                label: "Vol -".to_string(),
                icon: "volume-1".to_string(),
                color: "#3b82f6".to_string(),
                action: ShortcutAction::VolDown,
            },
        ];

        let runner = ShortcutRunner::new(tiles);
        let mut initial_snap = MediaSnapshot::default();
        initial_snap.volume_pct = 98;
        initial_snap.mic_muted = false;
        let mock = Arc::new(MockMediaBackend::new(initial_snap));
        let media: Arc<dyn MediaBackend> = mock;

        // Trigger mute_mic
        let res = runner.trigger("mute_mic", &media).await.expect("trigger mute_mic");
        assert!(res.is_some());
        assert!(res.unwrap().mic_muted);

        // Trigger vol_up (98 + 5 = 103 -> clamped to 100)
        let res = runner.trigger("vol_up", &media).await.expect("trigger vol_up");
        assert_eq!(res.unwrap().volume_pct, 100);

        // Trigger vol_down (100 - 5 = 95)
        let res = runner.trigger("vol_down", &media).await.expect("trigger vol_down");
        assert_eq!(res.unwrap().volume_pct, 95);
    }

    #[tokio::test]
    async fn test_trigger_invalid_url_and_missing_folder() {
        let tiles = vec![
            ShortcutConfig {
                id: "bad_url".to_string(),
                label: "Bad URL".to_string(),
                icon: "link".to_string(),
                color: "#000".to_string(),
                action: ShortcutAction::Url {
                    url: "javascript:evil()".to_string(),
                },
            },
            ShortcutConfig {
                id: "bad_folder".to_string(),
                label: "Bad Folder".to_string(),
                icon: "folder".to_string(),
                color: "#000".to_string(),
                action: ShortcutAction::Folder {
                    path: "Z:\\this\\path\\does\\not\\exist\\12345".to_string(),
                },
            },
        ];
        let runner = ShortcutRunner::new(tiles);
        let media: Arc<dyn MediaBackend> = Arc::new(WindowsMediaBackend::new());

        let res_url = runner.trigger("bad_url", &media).await;
        assert!(matches!(res_url, Err(ShortcutError::Invalid(_))));

        let res_folder = runner.trigger("bad_folder", &media).await;
        assert!(matches!(res_folder, Err(ShortcutError::Invalid(_))));
    }
}
