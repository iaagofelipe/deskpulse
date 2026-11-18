use serde::Serialize;
use std::env;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::watch;

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

async fn run_adb(adb: &PathBuf, args: &[&str]) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new(adb);
    cmd.args(args);
    #[cfg(windows)]
    cmd.creation_flags(crate::monitors::CREATE_NO_WINDOW);

    let out = tokio::time::timeout(Duration::from_secs(3), cmd.output())
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

async fn sync_adb_reverse(port: u16) -> AdbStatus {
    let Some(adb_path) = find_adb_path() else {
        return AdbStatus {
            adb_found: false,
            adb_path: None,
            devices: Vec::new(),
            reverse_active: false,
            last_error: Some("adb.exe não encontrado no PATH ou Android SDK".to_string()),
        };
    };

    let adb_str = adb_path.to_string_lossy().to_string();
    let devices_out = match run_adb(&adb_path, &["devices"]).await {
        Ok(out) => out,
        Err(err) => {
            return AdbStatus {
                adb_found: true,
                adb_path: Some(adb_str),
                devices: Vec::new(),
                reverse_active: false,
                last_error: Some(err),
            };
        }
    };

    let devices = parse_adb_devices(&devices_out);
    if devices.is_empty() {
        return AdbStatus {
            adb_found: true,
            adb_path: Some(adb_str),
            devices,
            reverse_active: false,
            last_error: None,
        };
    }

    let spec = format!("tcp:{port}");
    let mut any_ok = false;
    let mut last_error = None;

    for serial in &devices {
        match run_adb(&adb_path, &["-s", serial, "reverse", &spec, &spec]).await {
            Ok(_) => any_ok = true,
            Err(err) => last_error = Some(err),
        }
    }

    AdbStatus {
        adb_found: true,
        adb_path: Some(adb_str),
        devices,
        reverse_active: any_ok,
        last_error,
    }
}

pub struct AdbBridge {
    status: Arc<RwLock<AdbStatus>>,
}

impl AdbBridge {
    pub fn spawn(port: u16, mut shutdown: watch::Receiver<bool>) -> Self {
        let status = Arc::new(RwLock::new(AdbStatus {
            adb_found: find_adb_path().is_some(),
            adb_path: find_adb_path().map(|p| p.to_string_lossy().to_string()),
            ..AdbStatus::default()
        }));
        let status_bg = Arc::clone(&status);

        tokio::spawn(async move {
            loop {
                let next_status = sync_adb_reverse(port).await;
                if let Ok(mut w) = status_bg.write() {
                    *w = next_status;
                }

                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(4)) => {}
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() {
                            break;
                        }
                    }
                }
                if *shutdown.borrow() {
                    break;
                }
            }
        });

        Self { status }
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
}
