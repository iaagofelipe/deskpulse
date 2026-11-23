use serde::Serialize;
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::watch;

const ADB_SERVER_ADDR: &str = "127.0.0.1:5037";
const TRACK_DEVICES_REQ: &[u8] = b"0012host:track-devices";
const MAX_TRACK_PAYLOAD_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct AdbStatus {
    pub adb_found: bool,
    pub adb_path: Option<String>,
    pub devices: Vec<String>,
    pub reverse_active: bool,
    pub last_error: Option<String>,
}

pub fn parse_adb_devices(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("List of devices") || trimmed.starts_with('*') {
                return None;
            }
            let mut cols = trimmed.split_whitespace();
            let serial = cols.next()?;
            let state = cols.next()?;
            if state == "device" {
                Some(serial.to_string())
            } else {
                None
            }
        })
        .collect()
}

pub fn parse_adb_hex_len(header: &[u8; 4]) -> Result<usize, String> {
    let hex_str = std::str::from_utf8(header)
        .map_err(|_| "invalid UTF-8 in adb length header".to_string())?;
    let len = usize::from_str_radix(hex_str, 16)
        .map_err(|_| format!("invalid hex length from adb: {hex_str}"))?;
    if len > MAX_TRACK_PAYLOAD_BYTES {
        return Err(format!("adb track-devices payload too large: {len}"));
    }
    Ok(len)
}

pub fn diff_newly_connected(
    previous: &HashSet<String>,
    ready_devices: &[String],
) -> (Vec<String>, HashSet<String>) {
    let mut newly_connected = Vec::new();
    let mut next_known = HashSet::with_capacity(ready_devices.len());
    for serial in ready_devices {
        if !previous.contains(serial) {
            newly_connected.push(serial.clone());
        }
        next_known.insert(serial.clone());
    }
    (newly_connected, next_known)
}

pub fn find_adb_path() -> Option<PathBuf> {
    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            let candidate = dir.join("adb.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    let mut candidates = Vec::new();
    if let Some(local) = env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join("Android\\Sdk\\platform-tools\\adb.exe"));
    }
    if let Some(sdk) = env::var_os("ANDROID_HOME").or_else(|| env::var_os("ANDROID_SDK_ROOT")) {
        candidates.push(PathBuf::from(sdk).join("platform-tools\\adb.exe"));
    }
    candidates.push(PathBuf::from(r"D:\Android\Sdk\platform-tools\adb.exe"));
    candidates.push(PathBuf::from(r"C:\Android\Sdk\platform-tools\adb.exe"));

    candidates.into_iter().find(|p| p.is_file())
}

async fn run_adb(adb: &Path, args: &[&str], timeout_secs: u64) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new(adb);
    cmd.args(args);
    #[cfg(windows)]
    cmd.creation_flags(crate::monitors::CREATE_NO_WINDOW);

    let out = tokio::time::timeout(Duration::from_secs(timeout_secs), cmd.output())
        .await
        .map_err(|_| "adb command timed out".to_string())?
        .map_err(|e| format!("failed to spawn adb: {e}"))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("adb exited with {}", out.status)
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

async fn apply_devices_snapshot(
    adb_path: &Path,
    port: u16,
    devices: Vec<String>,
    known_devices: &mut HashSet<String>,
    auto_open_url: Option<&str>,
    auto_open_enabled: bool,
) -> AdbStatus {
    let adb_str = adb_path.to_string_lossy().to_string();
    if devices.is_empty() {
        known_devices.clear();
        return AdbStatus {
            adb_found: true,
            adb_path: Some(adb_str),
            devices,
            reverse_active: false,
            last_error: None,
        };
    }

    let spec = format!("tcp:{port}");
    let mut ready_devices = Vec::new();
    let mut last_error = None;

    for serial in &devices {
        match run_adb(adb_path, &["-s", serial, "reverse", &spec, &spec], 3).await {
            Ok(_) => ready_devices.push(serial.clone()),
            Err(err) => last_error = Some(err),
        }
    }

    let (newly_connected, next_known) = diff_newly_connected(known_devices, &ready_devices);
    *known_devices = next_known;

    if auto_open_enabled {
        if let Some(url) = auto_open_url {
            for serial in newly_connected {
                let _ = run_adb(
                    adb_path,
                    &[
                        "-s",
                        &serial,
                        "shell",
                        "am",
                        "start",
                        "-a",
                        "android.intent.action.VIEW",
                        "-d",
                        url,
                    ],
                    3,
                )
                .await;
                println!("Dispositivo USB detectado ({serial}): túnel ADB reverse ativo e DeskPulse aberto automaticamente.");
            }
        }
    }

    AdbStatus {
        adb_found: true,
        adb_path: Some(adb_str),
        devices,
        reverse_active: !ready_devices.is_empty(),
        last_error,
    }
}

async fn connect_track_devices(adb_path: &Path) -> Result<TcpStream, String> {
    let stream = match TcpStream::connect(ADB_SERVER_ADDR).await {
        Ok(s) => s,
        Err(_) => {
            let _ = run_adb(adb_path, &["start-server"], 6).await;
            TcpStream::connect(ADB_SERVER_ADDR)
                .await
                .map_err(|e| format!("falha ao conectar ao daemon ADB em {ADB_SERVER_ADDR}: {e}"))?
        }
    };

    let mut stream = stream;
    stream
        .write_all(TRACK_DEVICES_REQ)
        .await
        .map_err(|e| format!("falha ao enviar host:track-devices: {e}"))?;

    let mut status = [0u8; 4];
    stream
        .read_exact(&mut status)
        .await
        .map_err(|e| format!("falha ao ler resposta do ADB: {e}"))?;

    if &status != b"OKAY" {
        return Err(format!(
            "ADB recusou host:track-devices ({})",
            String::from_utf8_lossy(&status)
        ));
    }

    Ok(stream)
}

async fn read_track_frame(stream: &mut TcpStream) -> Result<String, String> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .map_err(|e| format!("stream ADB encerrado: {e}"))?;
    let len = parse_adb_hex_len(&len_buf)?;
    if len == 0 {
        return Ok(String::new());
    }
    let mut payload = vec![0u8; len];
    stream
        .read_exact(&mut payload)
        .await
        .map_err(|e| format!("falha ao ler payload ADB: {e}"))?;
    Ok(String::from_utf8_lossy(&payload).into_owned())
}

async fn sleep_or_shutdown(dur: Duration, shutdown: &mut watch::Receiver<bool>) -> bool {
    tokio::select! {
        biased;
        res = shutdown.changed() => res.is_err() || *shutdown.borrow(),
        _ = tokio::time::sleep(dur) => *shutdown.borrow(),
    }
}

pub struct AdbBridge {
    status: Arc<RwLock<AdbStatus>>,
    auto_open_enabled: Arc<AtomicBool>,
}

impl AdbBridge {
    #[allow(dead_code)]
    pub fn spawn(port: u16, shutdown: watch::Receiver<bool>) -> Self {
        Self::spawn_with_auto_open(port, None, false, shutdown)
    }

    pub fn spawn_with_auto_open(
        port: u16,
        auto_open_url: Option<String>,
        auto_open_initial: bool,
        mut shutdown: watch::Receiver<bool>,
    ) -> Self {
        let initial_adb = find_adb_path();
        let status = Arc::new(RwLock::new(AdbStatus {
            adb_found: initial_adb.is_some(),
            adb_path: initial_adb.map(|p| p.to_string_lossy().to_string()),
            ..AdbStatus::default()
        }));
        let auto_open_enabled = Arc::new(AtomicBool::new(auto_open_initial));
        let status_bg = Arc::clone(&status);
        let auto_open_bg = Arc::clone(&auto_open_enabled);

        tokio::spawn(async move {
            let mut known_devices = HashSet::new();

            loop {
                if *shutdown.borrow() {
                    break;
                }

                let Some(adb_path) = find_adb_path() else {
                    known_devices.clear();
                    if let Ok(mut w) = status_bg.write() {
                        *w = AdbStatus {
                            adb_found: false,
                            adb_path: None,
                            devices: Vec::new(),
                            reverse_active: false,
                            last_error: Some(
                                "adb.exe não encontrado no PATH ou Android SDK".to_string(),
                            ),
                        };
                    }
                    if sleep_or_shutdown(Duration::from_secs(15), &mut shutdown).await {
                        break;
                    }
                    continue;
                };

                let adb_str = adb_path.to_string_lossy().to_string();
                let mut stream = match connect_track_devices(&adb_path).await {
                    Ok(s) => s,
                    Err(err) => {
                        if let Ok(mut w) = status_bg.write() {
                            *w = AdbStatus {
                                adb_found: true,
                                adb_path: Some(adb_str),
                                devices: Vec::new(),
                                reverse_active: false,
                                last_error: Some(err),
                            };
                        }
                        if sleep_or_shutdown(Duration::from_secs(5), &mut shutdown).await {
                            break;
                        }
                        continue;
                    }
                };

                loop {
                    let frame_res = tokio::select! {
                        biased;
                        res = shutdown.changed() => {
                            if res.is_err() || *shutdown.borrow() {
                                return;
                            }
                            continue;
                        }
                        res = read_track_frame(&mut stream) => res,
                    };

                    match frame_res {
                        Ok(payload) => {
                            let devices = parse_adb_devices(&payload);
                            let next_status = apply_devices_snapshot(
                                &adb_path,
                                port,
                                devices,
                                &mut known_devices,
                                auto_open_url.as_deref(),
                                auto_open_bg.load(Ordering::Relaxed),
                            )
                            .await;
                            if let Ok(mut w) = status_bg.write() {
                                *w = next_status;
                            }
                        }
                        Err(err) => {
                            if let Ok(mut w) = status_bg.write() {
                                w.reverse_active = false;
                                w.last_error = Some(err);
                            }
                            break;
                        }
                    }
                }

                if sleep_or_shutdown(Duration::from_secs(3), &mut shutdown).await {
                    break;
                }
            }
        });

        Self {
            status,
            auto_open_enabled,
        }
    }

    pub fn set_auto_open(&self, enabled: bool) {
        self.auto_open_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn status(&self) -> AdbStatus {
        self.status
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_adb_devices_filters_unauthorized_and_offline() {
        let sample = "\
List of devices attached
R58M123ABC\tdevice
emulator-5554\toffline
999AAA111\tunauthorized
ZX1G42999\tdevice product:panther model:Pixel_7

";
        let devices = parse_adb_devices(sample);
        assert_eq!(devices, vec!["R58M123ABC", "ZX1G42999"]);
    }

    #[test]
    fn test_parse_track_devices_payload_and_hex_headers() {
        assert_eq!(parse_adb_hex_len(b"0000").unwrap(), 0);
        assert_eq!(parse_adb_hex_len(b"001a").unwrap(), 26);
        assert!(parse_adb_hex_len(b"ZZZZ").is_err());

        assert!(parse_adb_devices("").is_empty());
        let track_payload = "RXCY803GLLH\tunauthorized\nce0617161541dcc20d\tdevice\n";
        assert_eq!(parse_adb_devices(track_payload), vec!["ce0617161541dcc20d"]);
    }

    #[test]
    fn test_diff_newly_connected_tracks_plug_switch_and_replug() {
        let empty = HashSet::new();
        let (new1, known1) = diff_newly_connected(&empty, &["PHONE_A".to_string()]);
        assert_eq!(new1, vec!["PHONE_A"]);

        // Same phone stays plugged: should NOT re-open
        let (new2, known2) = diff_newly_connected(&known1, &["PHONE_A".to_string()]);
        assert!(new2.is_empty());

        // User swaps to PHONE_B: opens PHONE_B immediately
        let (new3, known3) = diff_newly_connected(&known2, &["PHONE_B".to_string()]);
        assert_eq!(new3, vec!["PHONE_B"]);
        assert!(!known3.contains("PHONE_A"));

        // User plugs PHONE_A back in later: opens PHONE_A again
        let (new4, _) = diff_newly_connected(&known3, &["PHONE_A".to_string()]);
        assert_eq!(new4, vec!["PHONE_A"]);
    }
}
