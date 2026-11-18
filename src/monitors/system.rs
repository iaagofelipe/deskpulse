use serde::Serialize;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use sysinfo::{Disks, Networks, System};
use tokio::sync::watch;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GpuSnapshot {
    pub name: String,
    pub util_pct: Option<f32>,
    pub temp_c: Option<f32>,
    pub vram_used_mb: Option<f32>,
    pub vram_total_mb: Option<f32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DiskSnapshot {
    pub name: String,
    pub used_gb: f64,
    pub total_gb: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PcSnapshot {
    pub cpu_pct: f32,
    pub cpu_name: String,
    pub ram_used_gb: f64,
    pub ram_total_gb: f64,
    pub net_down_bps: u64,
    pub net_up_bps: u64,
    pub disks: Vec<DiskSnapshot>,
    pub gpu: Option<GpuSnapshot>,
    pub uptime_sec: u64,
}

fn parse_optional_f32(raw: &str) -> Option<f32> {
    let t = raw.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("N/A") || t.eq_ignore_ascii_case("[N/A]") {
        return None;
    }
    t.parse::<f32>().ok()
}

pub fn parse_nvidia_smi_line(line: &str) -> Option<GpuSnapshot> {
    let parts: Vec<&str> = line.split(',').collect();
    if parts.len() < 5 {
        return None;
    }
    let name = parts[0].trim();
    if name.is_empty() {
        return None;
    }
    Some(GpuSnapshot {
        name: name.to_string(),
        util_pct: parse_optional_f32(parts[1]),
        temp_c: parse_optional_f32(parts[2]),
        vram_used_mb: parse_optional_f32(parts[3]),
        vram_total_mb: parse_optional_f32(parts[4]),
    })
}

fn query_nvidia_gpu_blocking() -> Option<GpuSnapshot> {
    let mut cmd = Command::new("nvidia-smi");
    cmd.args([
        "--query-gpu=name,utilization.gpu,temperature.gpu,memory.used,memory.total",
        "--format=csv,noheader,nounits",
    ]);
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(crate::monitors::CREATE_NO_WINDOW);

    let mut child = cmd.spawn().ok()?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut stdout = String::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_string(&mut stdout);
                }
                return stdout.lines().find_map(parse_nvidia_smi_line);
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn collect_pc_snapshot(
    sys: &mut System,
    disks: &mut Disks,
    networks: &mut Networks,
    elapsed_secs: f64,
    gpu: Option<GpuSnapshot>,
) -> PcSnapshot {
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    disks.refresh(true);
    networks.refresh(true);

    let cpu_pct = sys.global_cpu_usage();
    let cpu_name = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "CPU".to_string());

    let gib = 1024.0 * 1024.0 * 1024.0;
    let ram_used_gb = (sys.used_memory() as f64) / gib;
    let ram_total_gb = (sys.total_memory() as f64) / gib;

    let mut down_bytes: u64 = 0;
    let mut up_bytes: u64 = 0;
    for (_name, net) in networks.iter() {
        down_bytes = down_bytes.saturating_add(net.received());
        up_bytes = up_bytes.saturating_add(net.transmitted());
    }
    let dt = elapsed_secs.max(0.2);
    let net_down_bps = ((down_bytes as f64) / dt).round() as u64;
    let net_up_bps = ((up_bytes as f64) / dt).round() as u64;

    let disk_snaps = disks
        .iter()
        .filter(|d| d.total_space() > 0)
        .map(|d| {
            let total = (d.total_space() as f64) / gib;
            let avail = (d.available_space() as f64) / gib;
            let used = (total - avail).max(0.0);
            let mount = d.mount_point().to_string_lossy().to_string();
            let name = if mount.is_empty() {
                d.name().to_string_lossy().to_string()
            } else {
                mount
            };
            DiskSnapshot {
                name,
                used_gb: (used * 10.0).round() / 10.0,
                total_gb: (total * 10.0).round() / 10.0,
            }
        })
        .collect();

    PcSnapshot {
        cpu_pct: (cpu_pct * 10.0).round() / 10.0,
        cpu_name,
        ram_used_gb: (ram_used_gb * 100.0).round() / 100.0,
        ram_total_gb: (ram_total_gb * 100.0).round() / 100.0,
        net_down_bps,
        net_up_bps,
        disks: disk_snaps,
        gpu,
        uptime_sec: System::uptime(),
    }
}

pub struct SystemMonitor {
    latest: Arc<RwLock<Option<PcSnapshot>>>,
    collector_thread: Arc<RwLock<Option<String>>>,
}

impl SystemMonitor {
    pub fn spawn(shutdown: watch::Receiver<bool>) -> Self {
        let latest = Arc::new(RwLock::new(None));
        let collector_thread = Arc::new(RwLock::new(None));
        let latest_bg = Arc::clone(&latest);
        let thread_bg = Arc::clone(&collector_thread);

        let _ = std::thread::Builder::new()
            .name("deskpulse-sysmon".to_string())
            .spawn(move || {
                if let Ok(mut name_w) = thread_bg.write() {
                    *name_w = std::thread::current().name().map(str::to_string);
                }

                let mut sys = System::new_all();
                let mut disks = Disks::new_with_refreshed_list();
                let mut networks = Networks::new_with_refreshed_list();
                let mut cached_gpu = query_nvidia_gpu_blocking();

                let initial =
                    collect_pc_snapshot(&mut sys, &mut disks, &mut networks, 1.0, cached_gpu.clone());
                if let Ok(mut w) = latest_bg.write() {
                    *w = Some(initial);
                }

                let mut last_tick = Instant::now();
                let mut gpu_probe_tick: u64 = 1;

                loop {
                    for _ in 0..5 {
                        if *shutdown.borrow() {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    if *shutdown.borrow() {
                        return;
                    }

                    if gpu_probe_tick % 2 == 0 {
                        cached_gpu = query_nvidia_gpu_blocking();
                    }
                    gpu_probe_tick = gpu_probe_tick.wrapping_add(1);

                    let dt = last_tick.elapsed().as_secs_f64();
                    last_tick = Instant::now();
                    let snap = collect_pc_snapshot(
                        &mut sys,
                        &mut disks,
                        &mut networks,
                        dt,
                        cached_gpu.clone(),
                    );
                    if let Ok(mut w) = latest_bg.write() {
                        *w = Some(snap);
                    }
                }
            });

        Self {
            latest,
            collector_thread,
        }
    }

    pub fn snapshot(&self) -> Option<PcSnapshot> {
        self.latest.read().ok().and_then(|g| g.clone())
    }

    #[allow(dead_code)]
    pub fn collector_thread_name(&self) -> Option<String> {
        self.collector_thread.read().ok().and_then(|g| g.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_nvidia_smi_line_valid_and_na_fields() {
        let valid = parse_nvidia_smi_line("NVIDIA GeForce RTX 4070, 42, 61, 3210, 12282").unwrap();
        assert_eq!(valid.name, "NVIDIA GeForce RTX 4070");
        assert_eq!(valid.util_pct, Some(42.0));
        assert_eq!(valid.temp_c, Some(61.0));
        assert_eq!(valid.vram_used_mb, Some(3210.0));
        assert_eq!(valid.vram_total_mb, Some(12282.0));

        let with_na = parse_nvidia_smi_line("Quadro T1000, [N/A], N/A, 512, 4096").unwrap();
        assert_eq!(with_na.name, "Quadro T1000");
        assert_eq!(with_na.util_pct, None);
        assert_eq!(with_na.temp_c, None);
        assert_eq!(with_na.vram_used_mb, Some(512.0));
        assert_eq!(with_na.vram_total_mb, Some(4096.0));

        assert!(parse_nvidia_smi_line("").is_none());
        assert!(parse_nvidia_smi_line("bad").is_none());
    }

    #[tokio::test]
    async fn test_system_monitor_produces_initial_snapshot_without_panicking() {
        let (tx, rx) = watch::channel(false);
        let mon = SystemMonitor::spawn(rx);
        let deadline = Instant::now() + Duration::from_secs(3);
        let snap = loop {
            if let Some(s) = mon.snapshot() {
                break s;
            }
            if Instant::now() >= deadline {
                panic!("initial snapshot should exist");
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        assert!(snap.ram_total_gb > 0.1);
        assert!(!snap.disks.is_empty());
        let _ = tx.send(true);
    }

    #[tokio::test]
    async fn test_system_monitor_runs_off_tokio_worker_thread() {
        let (tx, rx) = watch::channel(false);
        let mon = SystemMonitor::spawn(rx);
        let deadline = Instant::now() + Duration::from_secs(3);
        let thread_name = loop {
            if let Some(name) = mon.collector_thread_name() {
                break name;
            }
            if Instant::now() >= deadline {
                panic!("timed out waiting for collector thread telemetry");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        assert_eq!(thread_name, "deskpulse-sysmon");
        let _ = tx.send(true);
    }
}
