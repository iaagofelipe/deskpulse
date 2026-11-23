use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Serialize;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc;
use tokio::sync::oneshot;

use windows::core::GUID;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows::Storage::Streams::DataReader;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

pub const TICKS_PER_SECOND: i64 = 10_000_000;

pub fn ticks_to_secs(ticks: i64) -> Option<u64> {
    if ticks < 0 {
        None
    } else {
        Some((ticks / TICKS_PER_SECOND) as u64)
    }
}

pub fn secs_to_ticks(secs: u64) -> i64 {
    let max_secs = (i64::MAX / TICKS_PER_SECOND) as u64;
    (secs.min(max_secs) as i64) * TICKS_PER_SECOND
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct MediaSnapshot {
    pub playing: bool,
    pub status: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub app: String,
    pub artwork_data_url: Option<String>,
    pub volume_pct: u8,
    pub muted: bool,
    pub position_secs: Option<u64>,
    pub duration_secs: Option<u64>,
    pub can_seek: bool,
    pub mic_muted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCommand {
    PlayPause,
    Next,
    Previous,
    Mute,
    Unmute,
    ToggleMute,
    SetVolume(u8),
    Seek(u64),
    ToggleMicMute,
}

pub fn parse_media_command(action: &str, level: Option<i32>) -> Result<MediaCommand, String> {
    match action.trim().to_ascii_lowercase().as_str() {
        "play-pause" => Ok(MediaCommand::PlayPause),
        "next" => Ok(MediaCommand::Next),
        "prev" | "previous" => Ok(MediaCommand::Previous),
        "mute" => Ok(MediaCommand::Mute),
        "unmute" => Ok(MediaCommand::Unmute),
        "toggle-mute" => Ok(MediaCommand::ToggleMute),
        "volume" => {
            let v = level.ok_or_else(|| "missing volume level".to_string())?;
            Ok(MediaCommand::SetVolume(v.clamp(0, 100) as u8))
        }
        "seek" => {
            let v = level.ok_or_else(|| "missing seek position".to_string())?;
            Ok(MediaCommand::Seek(v.max(0) as u64))
        }
        "toggle-mic" | "toggle-mic-mute" => Ok(MediaCommand::ToggleMicMute),
        other => Err(format!("unknown media action: {other}")),
    }
}

pub trait MediaBackend: Send + Sync {
    fn snapshot(&self) -> Pin<Box<dyn Future<Output = MediaSnapshot> + Send + '_>>;
    fn execute(
        &self,
        cmd: MediaCommand,
    ) -> Pin<Box<dyn Future<Output = Result<MediaSnapshot, String>> + Send + '_>>;
}

enum WorkerMsg {
    Snapshot(oneshot::Sender<MediaSnapshot>),
    Execute(MediaCommand, oneshot::Sender<Result<MediaSnapshot, String>>),
}

pub struct WindowsMediaBackend {
    tx: mpsc::Sender<WorkerMsg>,
}

fn endpoint_volume() -> Result<IAudioEndpointVolume, String> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("IMMDeviceEnumerator failed: {e}"))?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|e| format!("GetDefaultAudioEndpoint failed: {e}"))?;
        device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| format!("IAudioEndpointVolume activate failed: {e}"))
    }
}

fn capture_endpoint_volume() -> Result<IAudioEndpointVolume, String> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("IMMDeviceEnumerator failed: {e}"))?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eCapture, eConsole)
            .or_else(|_| enumerator.GetDefaultAudioEndpoint(eCapture, eCommunications))
            .map_err(|e| format!("GetDefaultAudioEndpoint capture failed: {e}"))?;
        device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| format!("IAudioEndpointVolume activate failed: {e}"))
    }
}

fn read_audio_state() -> (u8, bool) {
    let Ok(vol) = endpoint_volume() else {
        return (0, false);
    };
    let scalar = unsafe { vol.GetMasterVolumeLevelScalar() }.unwrap_or(0.0);
    let muted = unsafe { vol.GetMute() }
        .map(|b| b.as_bool())
        .unwrap_or(false);
    (((scalar * 100.0).round().clamp(0.0, 100.0)) as u8, muted)
}

fn read_mic_muted_state() -> bool {
    let Ok(vol) = capture_endpoint_volume() else {
        return false;
    };
    unsafe { vol.GetMute() }
        .map(|b| b.as_bool())
        .unwrap_or(false)
}

fn toggle_mic_mute() -> Result<(), String> {
    let vol = capture_endpoint_volume()?;
    unsafe {
        let current = vol.GetMute().map(|b| b.as_bool()).unwrap_or(false);
        vol.SetMute(!current, &GUID::zeroed())
            .map_err(|e| format!("SetMute mic toggle failed: {e}"))
    }
}

fn apply_audio_command(cmd: MediaCommand) -> Result<(), String> {
    let vol = endpoint_volume()?;
    unsafe {
        match cmd {
            MediaCommand::Mute => vol
                .SetMute(true, &GUID::zeroed())
                .map_err(|e| format!("SetMute(true) failed: {e}")),
            MediaCommand::Unmute => vol
                .SetMute(false, &GUID::zeroed())
                .map_err(|e| format!("SetMute(false) failed: {e}")),
            MediaCommand::ToggleMute => {
                let current = vol.GetMute().map(|b| b.as_bool()).unwrap_or(false);
                vol.SetMute(!current, &GUID::zeroed())
                    .map_err(|e| format!("SetMute toggle failed: {e}"))
            }
            MediaCommand::SetVolume(pct) => {
                let scalar = (pct.min(100) as f32) / 100.0;
                vol.SetMasterVolumeLevelScalar(scalar, &GUID::zeroed())
                    .map_err(|e| format!("SetMasterVolumeLevelScalar failed: {e}"))
            }
            _ => Ok(()),
        }
    }
}

fn detect_image_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/jpeg"
    }
}

fn read_thumbnail_data_url(
    session_props: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties,
) -> Option<String> {
    let thumb_ref = session_props.Thumbnail().ok()?;
    let stream = thumb_ref.OpenReadAsync().ok()?.get().ok()?;
    let raw_size = stream.Size().ok()?;
    if raw_size == 0 || raw_size > 128 * 1024 {
        return None;
    }
    let size = raw_size as u32;
    let input = stream.GetInputStreamAt(0).ok()?;
    let reader = DataReader::CreateDataReader(&input).ok()?;
    let loaded = reader.LoadAsync(size).ok()?.get().ok()?;
    if loaded == 0 {
        return None;
    }
    let mut buf = vec![0u8; loaded as usize];
    reader.ReadBytes(&mut buf).ok()?;
    let mime = detect_image_mime(&buf);
    Some(format!("data:{mime};base64,{}", BASE64.encode(&buf)))
}

fn current_session() -> Option<GlobalSystemMediaTransportControlsSession> {
    let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .ok()?
        .get()
        .ok()?;
    manager.GetCurrentSession().ok()
}

pub fn friendly_app_name(app_raw: &str) -> String {
    let base = app_raw
        .split('!')
        .next()
        .unwrap_or(app_raw)
        .trim()
        .trim_end_matches(".exe")
        .trim_end_matches(".EXE");
    if base.is_empty() {
        return String::new();
    }
    let lower = base.to_ascii_lowercase();
    match lower.as_str() {
        "308046b0af4a39cb" | "e7cf176e110c211b" | "firefox" => return "Firefox".to_string(),
        "chrome" => return "Chrome".to_string(),
        "msedge" => return "Edge".to_string(),
        "brave" => return "Brave".to_string(),
        "opera" => return "Opera".to_string(),
        "vivaldi" => return "Vivaldi".to_string(),
        "zen" => return "Zen".to_string(),
        "vlc" => return "VLC".to_string(),
        "spotify" => return "Spotify".to_string(),
        _ => {}
    }
    if lower.contains("spotify") {
        return "Spotify".to_string();
    }
    if lower.contains("firefox") {
        return "Firefox".to_string();
    }
    if lower.contains("chrome") {
        return "Chrome".to_string();
    }
    if lower.contains("msedge") || lower.contains("microsoftedge") {
        return "Edge".to_string();
    }
    if lower.contains("zunemusic") || lower.contains("windowsmediaplayer") {
        return "Media Player".to_string();
    }
    if base.len() == 16 && base.chars().all(|c| c.is_ascii_hexdigit()) {
        return "Navegador".to_string();
    }
    base.to_string()
}

fn collect_media_snapshot(artwork_cache: &mut HashMap<(String, String, String), Option<String>>) -> MediaSnapshot {
    let (volume_pct, muted) = read_audio_state();
    let mic_muted = read_mic_muted_state();
    let Some(session) = current_session() else {
        return MediaSnapshot {
            playing: false,
            status: "Idle".to_string(),
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            app: String::new(),
            artwork_data_url: None,
            volume_pct,
            muted,
            position_secs: None,
            duration_secs: None,
            can_seek: false,
            mic_muted,
        };
    };

    let playback = session.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()).ok();
    let (playing, status) = match playback {
        Some(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing) => (true, "Playing"),
        Some(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused) => (false, "Paused"),
        Some(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Stopped) => (false, "Stopped"),
        _ => (false, "Idle"),
    };

    let app_raw = session
        .SourceAppUserModelId()
        .map(|h| h.to_string())
        .unwrap_or_default();
    let app = friendly_app_name(&app_raw);

    let props_opt = session.TryGetMediaPropertiesAsync().ok().and_then(|op| op.get().ok());
    let (title, artist, album, artwork_data_url) = if let Some(ref props) = props_opt {
        let title = props.Title().map(|h| h.to_string()).unwrap_or_default();
        let artist = props.Artist().map(|h| h.to_string()).unwrap_or_default();
        let album = props.AlbumTitle().map(|h| h.to_string()).unwrap_or_default();
        let key = (app.clone(), title.clone(), artist.clone());
        let art = if let Some(cached) = artwork_cache.get(&key) {
            cached.clone()
        } else {
            let fetched = read_thumbnail_data_url(props);
            if artwork_cache.len() > 24 {
                artwork_cache.clear();
            }
            artwork_cache.insert(key, fetched.clone());
            fetched
        };
        (title, artist, album, art)
    } else {
        (String::new(), String::new(), String::new(), None)
    };

    let (position_secs, duration_secs) = if let Ok(timeline) = session.GetTimelineProperties() {
        let pos = timeline.Position().ok().and_then(|t| ticks_to_secs(t.Duration));
        let dur = timeline.EndTime().ok().and_then(|t| ticks_to_secs(t.Duration));
        (pos, dur)
    } else {
        (None, None)
    };
    let can_seek = duration_secs.is_some_and(|d| d > 0);

    MediaSnapshot {
        playing,
        status: status.to_string(),
        title,
        artist,
        album,
        app,
        artwork_data_url,
        volume_pct,
        muted,
        position_secs,
        duration_secs,
        can_seek,
        mic_muted,
    }
}

fn run_media_command(cmd: MediaCommand) -> Result<(), String> {
    match cmd {
        MediaCommand::Mute
        | MediaCommand::Unmute
        | MediaCommand::ToggleMute
        | MediaCommand::SetVolume(_) => apply_audio_command(cmd),
        MediaCommand::ToggleMicMute => toggle_mic_mute(),
        MediaCommand::Seek(secs) => {
            let Some(session) = current_session() else {
                return Err("no active media session".to_string());
            };
            let ticks = secs_to_ticks(secs);
            let ok = session
                .TryChangePlaybackPositionAsync(ticks)
                .and_then(|op| op.get())
                .map_err(|e| format!("TryChangePlaybackPositionAsync failed: {e}"))?;
            if ok {
                Ok(())
            } else {
                Err("seek command was not accepted".to_string())
            }
        }
        MediaCommand::PlayPause | MediaCommand::Next | MediaCommand::Previous => {
            let Some(session) = current_session() else {
                return Err("no active media session".to_string());
            };
            let ok = match cmd {
                MediaCommand::PlayPause => session
                    .TryTogglePlayPauseAsync()
                    .and_then(|op| op.get())
                    .map_err(|e| format!("TryTogglePlayPauseAsync failed: {e}"))?,
                MediaCommand::Next => session
                    .TrySkipNextAsync()
                    .and_then(|op| op.get())
                    .map_err(|e| format!("TrySkipNextAsync failed: {e}"))?,
                MediaCommand::Previous => session
                    .TrySkipPreviousAsync()
                    .and_then(|op| op.get())
                    .map_err(|e| format!("TrySkipPreviousAsync failed: {e}"))?,
                _ => false,
            };
            if ok {
                Ok(())
            } else {
                Err("media transport command was not accepted".to_string())
            }
        }
    }
}

impl WindowsMediaBackend {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<WorkerMsg>();
        std::thread::Builder::new()
            .name("deskpulse-media-com".to_string())
            .spawn(move || {
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                }
                let mut artwork_cache = HashMap::new();
                while let Ok(msg) = rx.recv() {
                    match msg {
                        WorkerMsg::Snapshot(reply) => {
                            let _ = reply.send(collect_media_snapshot(&mut artwork_cache));
                        }
                        WorkerMsg::Execute(cmd, reply) => {
                            let res = run_media_command(cmd)
                                .map(|()| collect_media_snapshot(&mut artwork_cache));
                            let _ = reply.send(res);
                        }
                    }
                }
            })
            .expect("spawn media COM thread");

        Self { tx }
    }
}

impl Default for WindowsMediaBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaBackend for WindowsMediaBackend {
    fn snapshot(&self) -> Pin<Box<dyn Future<Output = MediaSnapshot> + Send + '_>> {
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            if self.tx.send(WorkerMsg::Snapshot(tx)).is_err() {
                return MediaSnapshot {
                    status: "Idle".to_string(),
                    ..MediaSnapshot::default()
                };
            }
            rx.await.unwrap_or_else(|_| MediaSnapshot {
                status: "Idle".to_string(),
                ..MediaSnapshot::default()
            })
        })
    }

    fn execute(
        &self,
        cmd: MediaCommand,
    ) -> Pin<Box<dyn Future<Output = Result<MediaSnapshot, String>> + Send + '_>> {
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            self.tx
                .send(WorkerMsg::Execute(cmd, tx))
                .map_err(|_| "media worker channel closed".to_string())?;
            rx.await
                .map_err(|_| "media worker dropped reply".to_string())?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_media_command_parsing_and_volume_clamping() {
        assert_eq!(parse_media_command("play-pause", None).unwrap(), MediaCommand::PlayPause);
        assert_eq!(parse_media_command("next", None).unwrap(), MediaCommand::Next);
        assert_eq!(parse_media_command("prev", None).unwrap(), MediaCommand::Previous);
        assert_eq!(parse_media_command("previous", None).unwrap(), MediaCommand::Previous);
        assert_eq!(parse_media_command("mute", None).unwrap(), MediaCommand::Mute);
        assert_eq!(parse_media_command("unmute", None).unwrap(), MediaCommand::Unmute);
        assert_eq!(parse_media_command("toggle-mute", None).unwrap(), MediaCommand::ToggleMute);
        assert_eq!(parse_media_command("volume", Some(150)).unwrap(), MediaCommand::SetVolume(100));
        assert_eq!(parse_media_command("volume", Some(-10)).unwrap(), MediaCommand::SetVolume(0));
        assert!(parse_media_command("volume", None).is_err());
        assert!(parse_media_command("explode", None).is_err());
    }

    #[test]
    fn test_parse_seek_and_mic_commands() {
        assert_eq!(parse_media_command("seek", Some(75)), Ok(MediaCommand::Seek(75)));
        assert!(parse_media_command("seek", None).is_err());
        assert_eq!(parse_media_command("seek", Some(-5)), Ok(MediaCommand::Seek(0)));
        assert_eq!(parse_media_command("toggle-mic", None), Ok(MediaCommand::ToggleMicMute));
        assert_eq!(parse_media_command("toggle-mic-mute", None), Ok(MediaCommand::ToggleMicMute));
    }

    #[test]
    fn test_winrt_timespan_tick_conversions() {
        assert_eq!(ticks_to_secs(750_000_000), Some(75));
        assert_eq!(ticks_to_secs(-1), None);
        assert_eq!(secs_to_ticks(120), 1_200_000_000);
    }

    #[test]
    fn test_friendly_app_name_normalizes_aumids_and_hashes() {
        assert_eq!(friendly_app_name("308046B0AF4A39CB"), "Firefox");
        assert_eq!(friendly_app_name("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"), "Spotify");
        assert_eq!(friendly_app_name("chrome.exe"), "Chrome");
        assert_eq!(friendly_app_name("msedge.exe"), "Edge");
        assert_eq!(friendly_app_name("A1B2C3D4E5F60718"), "Navegador");
        assert_eq!(friendly_app_name("foobar2000.exe"), "foobar2000");
        assert_eq!(friendly_app_name(""), "");
    }

    #[tokio::test]
    async fn test_windows_media_backend_snapshot_does_not_panic() {
        let backend = WindowsMediaBackend::new();
        let snap = backend.snapshot().await;
        assert!(!snap.status.is_empty());
        assert!(snap.volume_pct <= 100);
    }
}
