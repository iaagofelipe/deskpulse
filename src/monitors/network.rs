use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sysinfo::Networks;
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::time::timeout;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NetworkSnapshot {
    pub online: bool,
    pub latency_ms: Option<u32>,
    pub jitter_ms: Option<u32>,
    pub loss_pct: f32,
    pub history_ms: Vec<i32>,
    pub up_since_ms: Option<u64>,
    pub last_drop_ms: Option<u64>,
    pub today_bytes: u64,
    pub month_bytes: u64,
    pub speed_down_mbps: Option<f32>,
    pub speed_tested_at_ms: Option<u64>,
    pub testing: bool,
}

impl Default for NetworkSnapshot {
    fn default() -> Self {
        Self {
            online: false,
            latency_ms: None,
            jitter_ms: None,
            loss_pct: 0.0,
            history_ms: Vec::new(),
            up_since_ms: None,
            last_drop_ms: None,
            today_bytes: 0,
            month_bytes: 0,
            speed_down_mbps: None,
            speed_tested_at_ms: None,
            testing: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetUsageFile {
    pub day_key: String,
    pub month_key: String,
    pub today_bytes: u64,
    pub month_bytes: u64,
}

pub fn compute_jitter_ms(history: &[i32]) -> Option<u32> {
    let valid: Vec<i32> = history.iter().copied().filter(|&x| x >= 0).collect();
    if valid.len() < 2 {
        return None;
    }
    let mut diff_sum: u64 = 0;
    let pairs = valid.len() - 1;
    for i in 0..pairs {
        diff_sum += (valid[i + 1] - valid[i]).unsigned_abs() as u64;
    }
    Some((diff_sum as f64 / pairs as f64).round() as u32)
}

pub fn compute_loss_pct(history: &[i32]) -> f32 {
    if history.is_empty() {
        return 0.0;
    }
    let loss_count = history.iter().filter(|&&x| x < 0).count();
    let pct = (loss_count as f32 / history.len() as f32) * 100.0;
    (pct * 10.0).round() / 10.0
}

pub fn rollover_usage(state: &mut NetUsageFile, today: NaiveDate) {
    let day_key = today.format("%Y-%m-%d").to_string();
    let month_key = today.format("%Y-%m").to_string();

    if state.month_key != month_key {
        state.month_key = month_key;
        state.day_key = day_key;
        state.today_bytes = 0;
        state.month_bytes = 0;
    } else if state.day_key != day_key {
        state.day_key = day_key;
        state.today_bytes = 0;
    }
}

pub fn save_usage_atomic(path: &Path, usage: &NetUsageFile) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let data = serde_json::to_vec_pretty(usage)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let tmp_path = path.with_extension(format!("tmp.{}", rand::random::<u32>()));
    std::fs::write(&tmp_path, &data)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

pub fn load_usage(path: &Path, today: NaiveDate) -> NetUsageFile {
    let day_key = today.format("%Y-%m-%d").to_string();
    let month_key = today.format("%Y-%m").to_string();

    if let Ok(data) = std::fs::read(path) {
        if let Ok(mut file) = serde_json::from_slice::<NetUsageFile>(&data) {
            rollover_usage(&mut file, today);
            return file;
        }
    }

    NetUsageFile {
        day_key,
        month_key,
        today_bytes: 0,
        month_bytes: 0,
    }
}

async fn probe_tcp_connect() -> i32 {
    let probe_target = |addr: &'static str| async move {
        let start = Instant::now();
        match timeout(Duration::from_millis(1500), TcpStream::connect(addr)).await {
            Ok(Ok(_stream)) => {
                let ms = start.elapsed().as_millis().min(1500) as i32;
                Some(ms)
            }
            _ => None,
        }
    };

    if let Some(ms) = probe_target("1.1.1.1:443").await {
        return ms;
    }
    if let Some(ms) = probe_target("8.8.8.8:443").await {
        return ms;
    }
    -1
}

#[allow(dead_code)]
pub struct NetworkMonitor {
    snapshot: Arc<RwLock<NetworkSnapshot>>,
    _usage_path: PathBuf,
}

#[allow(dead_code)]
impl NetworkMonitor {
    pub fn spawn(usage_path: PathBuf, mut shutdown: watch::Receiver<bool>) -> Self {
        let today = Local::now().date_naive();
        let initial_usage = load_usage(&usage_path, today);

        let initial_snapshot = NetworkSnapshot {
            online: false,
            latency_ms: None,
            jitter_ms: None,
            loss_pct: 0.0,
            history_ms: Vec::new(),
            up_since_ms: None,
            last_drop_ms: None,
            today_bytes: initial_usage.today_bytes,
            month_bytes: initial_usage.month_bytes,
            speed_down_mbps: None,
            speed_tested_at_ms: None,
            testing: false,
        };

        let snapshot = Arc::new(RwLock::new(initial_snapshot));
        let snapshot_bg = Arc::clone(&snapshot);
        let usage_path_bg = usage_path.clone();

        let run_loop = async move {
            let mut usage_file = initial_usage;
            let mut networks = Networks::new_with_refreshed_list();
            let mut history_ms: Vec<i32> = Vec::with_capacity(60);
            let mut prev_online: Option<bool> = None;
            let mut tick_count: u64 = 0;

            loop {
                if *shutdown.borrow() {
                    let _ = save_usage_atomic(&usage_path_bg, &usage_file);
                    return;
                }

                let sample = probe_tcp_connect().await;
                history_ms.push(sample);
                while history_ms.len() > 60 {
                    history_ms.remove(0);
                }

                let online = sample >= 0;
                let latency_ms = if online { Some(sample as u32) } else { None };
                let jitter_ms = compute_jitter_ms(&history_ms);
                let loss_pct = compute_loss_pct(&history_ms);

                let now_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);

                networks.refresh(true);
                let mut delta_bytes: u64 = 0;
                for net in networks.values() {
                    delta_bytes = delta_bytes
                        .saturating_add(net.received())
                        .saturating_add(net.transmitted());
                }

                let current_day = Local::now().date_naive();
                rollover_usage(&mut usage_file, current_day);
                usage_file.today_bytes = usage_file.today_bytes.saturating_add(delta_bytes);
                usage_file.month_bytes = usage_file.month_bytes.saturating_add(delta_bytes);

                if let Ok(mut snap) = snapshot_bg.write() {
                    snap.online = online;
                    snap.latency_ms = latency_ms;
                    snap.jitter_ms = jitter_ms;
                    snap.loss_pct = loss_pct;
                    snap.history_ms = history_ms.clone();
                    snap.today_bytes = usage_file.today_bytes;
                    snap.month_bytes = usage_file.month_bytes;

                    match prev_online {
                        None => {
                            if online {
                                snap.up_since_ms = Some(now_ms);
                                snap.last_drop_ms = None;
                            } else {
                                snap.up_since_ms = None;
                                snap.last_drop_ms = Some(now_ms);
                            }
                        }
                        Some(false) if online => {
                            snap.up_since_ms = Some(now_ms);
                        }
                        Some(true) if !online => {
                            snap.last_drop_ms = Some(now_ms);
                            snap.up_since_ms = None;
                        }
                        _ => {}
                    }
                }
                prev_online = Some(online);

                tick_count = tick_count.wrapping_add(1);
                if tick_count % 30 == 0 {
                    let _ = save_usage_atomic(&usage_path_bg, &usage_file);
                }

                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() {
                            let _ = save_usage_atomic(&usage_path_bg, &usage_file);
                            return;
                        }
                    }
                }
            }
        };

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(run_loop);
        } else {
            let _ = std::thread::Builder::new()
                .name("deskpulse-netmon".to_string())
                .spawn(move || {
                    if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        rt.block_on(run_loop);
                    }
                });
        }

        Self {
            snapshot,
            _usage_path: usage_path,
        }
    }

    pub fn snapshot(&self) -> NetworkSnapshot {
        self.snapshot.read().unwrap().clone()
    }

    pub fn trigger_speedtest(&self) -> Result<NetworkSnapshot, String> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let mut snap = self.snapshot.write().unwrap();
        if snap.testing {
            return Err("Teste de velocidade já em andamento, aguarde...".to_string());
        }
        if let Some(tested_at) = snap.speed_tested_at_ms {
            let elapsed_ms = now_ms.saturating_sub(tested_at);
            if elapsed_ms < 120_000 {
                let remaining_secs = ((120_000 - elapsed_ms) / 1000) + 1;
                return Err(format!(
                    "Aguarde {}s para realizar outro teste de velocidade.",
                    remaining_secs
                ));
            }
        }

        snap.testing = true;
        let ret_snap = snap.clone();
        drop(snap);

        let snapshot_bg = Arc::clone(&self.snapshot);
        let speedtest_task = async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());

            let start = Instant::now();
            let url = "https://speed.cloudflare.com/__down?bytes=10000000";
            let res = client.get(url).send().await;

            let mut total_bytes: usize = 0;
            let mut success = false;
            if let Ok(mut resp) = res {
                if resp.status().is_success() {
                    while let Ok(Some(chunk)) = resp.chunk().await {
                        total_bytes += chunk.len();
                        if total_bytes >= 10_000_000 {
                            break;
                        }
                    }
                    success = total_bytes > 0;
                }
            }

            let elapsed_secs = start.elapsed().as_secs_f64();
            let tested_now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            if let Ok(mut snap) = snapshot_bg.write() {
                snap.testing = false;
                snap.speed_tested_at_ms = Some(tested_now_ms);
                if success && elapsed_secs > 0.05 {
                    let bits = (total_bytes as f64) * 8.0;
                    let mbps = (bits / (elapsed_secs * 1_000_000.0)) as f32;
                    snap.speed_down_mbps = Some((mbps * 10.0).round() / 10.0);
                }
            }
        };

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(speedtest_task);
        } else {
            let _ = std::thread::Builder::new()
                .name("deskpulse-speedtest".to_string())
                .spawn(move || {
                    if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        rt.block_on(speedtest_task);
                    }
                });
        }

        Ok(ret_snap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jitter_and_loss_calculation() {
        let history = vec![-1, 20, 30, 26, -1];
        assert_eq!(compute_loss_pct(&history), 40.0);
        assert_eq!(compute_jitter_ms(&history), Some(7));
    }

    #[test]
    fn test_net_usage_rollover_resets_day_and_month() {
        let mut usage = NetUsageFile {
            day_key: "2026-11-23".to_string(),
            month_key: "2026-11".to_string(),
            today_bytes: 1_000_000,
            month_bytes: 5_000_000,
        };

        // Cross midnight into next day
        let next_day = NaiveDate::from_ymd_opt(2026, 11, 24).unwrap();
        rollover_usage(&mut usage, next_day);
        assert_eq!(usage.day_key, "2026-11-24");
        assert_eq!(usage.month_key, "2026-11");
        assert_eq!(usage.today_bytes, 0);
        assert_eq!(usage.month_bytes, 5_000_000);

        usage.today_bytes = 2_000_000;
        usage.month_bytes += 2_000_000;

        // Cross month
        let next_month = NaiveDate::from_ymd_opt(2026, 12, 1).unwrap();
        rollover_usage(&mut usage, next_month);
        assert_eq!(usage.day_key, "2026-12-01");
        assert_eq!(usage.month_key, "2026-12");
        assert_eq!(usage.today_bytes, 0);
        assert_eq!(usage.month_bytes, 0);

        // Atomic save and load roundtrip
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("net_usage.json");
        usage.today_bytes = 42;
        usage.month_bytes = 100;
        save_usage_atomic(&path, &usage).unwrap();
        let loaded = load_usage(&path, next_month);
        assert_eq!(loaded, usage);
    }

    #[test]
    fn test_speedtest_rate_limit_rejects_within_120s() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("net_usage.json");
        let (tx, rx) = watch::channel(false);
        let mon = NetworkMonitor::spawn(path, rx);

        let first = mon.trigger_speedtest();
        assert!(first.is_ok());

        let second = mon.trigger_speedtest();
        assert!(second.is_err());
        let err_msg = second.unwrap_err().to_lowercase();
        assert!(
            err_msg.contains("aguarde"),
            "Expected 'aguarde' in error message: {}",
            err_msg
        );
        let _ = tx.send(true);
    }
}
