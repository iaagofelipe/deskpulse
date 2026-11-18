mod approvals;
mod config;
mod events;
mod hooks;
mod monitors;
mod security;
mod server;

use std::env;
use std::io::Read;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

use crate::approvals::ApprovalStore;
use crate::events::EventBus;
use crate::hooks::{
    default_hooks_path, install_hooks_to, run_hook_cli, uninstall_hooks_from, HookKind,
};
use crate::monitors::adb::AdbBridge;
use crate::monitors::media::WindowsMediaBackend;
use crate::monitors::system::SystemMonitor;
use crate::server::{build_router, AppState};

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Serve,
    HookPreTool,
    HookStop,
    InstallHooks,
    UninstallHooks,
}

pub fn parse_cli<I, S>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let parts: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();
    match parts.get(1).map(String::as_str) {
        None | Some("serve") if parts.len() <= 2 => Ok(Command::Serve),
        Some("hook") => match parts.get(2).map(String::as_str) {
            Some("pre-tool") if parts.len() == 3 => Ok(Command::HookPreTool),
            Some("stop") if parts.len() == 3 => Ok(Command::HookStop),
            _ => Err("usage: deskpulse hook <pre-tool|stop>".to_string()),
        },
        Some("install-hooks") if parts.len() == 2 => Ok(Command::InstallHooks),
        Some("uninstall-hooks") if parts.len() == 2 => Ok(Command::UninstallHooks),
        _ => Err("usage: deskpulse [serve|hook <pre-tool|stop>|install-hooks|uninstall-hooks]".to_string()),
    }
}

async fn run_serve() -> Result<(), String> {
    let cfg_path = config::default_config_path();
    let cfg = config::load_or_create(&cfg_path)?;

    let addr: SocketAddr = ([127, 0, 0, 1], cfg.port).into();
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("failed to bind {addr}: {e}"))?;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let events = Arc::new(EventBus::new());
    let approvals = Arc::new(ApprovalStore::new(
        Arc::clone(&events),
        Duration::from_secs(cfg.approval_timeout_secs),
    ));
    let system = Arc::new(SystemMonitor::spawn(shutdown_rx.clone()));
    let adb = Arc::new(AdbBridge::spawn(cfg.port, shutdown_rx));
    let media = Arc::new(WindowsMediaBackend::new());

    let app = build_router(AppState {
        token: cfg.token.clone(),
        port: cfg.port,
        events,
        approvals,
        system,
        adb,
        media,
    });

    println!("DeskPulse rodando em http://127.0.0.1:{}/#token={}", cfg.port, cfg.token);
    println!("Config: {}", cfg_path.display());
    println!("Plugue o celular Android via USB (Depuracao USB ativa) e abra http://127.0.0.1:{}/#token={}", cfg.port, cfg.token);

    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|e| format!("server error: {e}"));

    let _ = shutdown_tx.send(true);
    result
}

async fn run_hook_command(kind: HookKind) {
    let fallback = match kind {
        HookKind::PreTool => r#"{"decision":"ask"}"#.to_string(),
        HookKind::Stop => "{}".to_string(),
    };

    let mut stdin_text = String::new();
    if std::io::stdin().read_to_string(&mut stdin_text).is_err() {
        println!("{fallback}");
        return;
    }

    let Ok(cfg) = config::load_or_create(&config::default_config_path()) else {
        println!("{fallback}");
        return;
    };

    let base_url = format!("http://127.0.0.1:{}", cfg.port);
    let out = run_hook_cli(kind, &stdin_text, &base_url, &cfg.token).await;
    println!("{out}");
}

#[tokio::main]
async fn main() {
    match parse_cli(env::args()) {
        Ok(Command::Serve) => {
            if let Err(err) = run_serve().await {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        Ok(Command::HookPreTool) => run_hook_command(HookKind::PreTool).await,
        Ok(Command::HookStop) => run_hook_command(HookKind::Stop).await,
        Ok(Command::InstallHooks) => {
            let exe = env::current_exe().unwrap_or_else(|e| {
                eprintln!("failed to resolve current executable: {e}");
                std::process::exit(1);
            });
            let path = default_hooks_path();
            if let Err(err) = install_hooks_to(&path, &exe) {
                eprintln!("{err}");
                std::process::exit(1);
            }
            println!("Hooks do DeskPulse instalados em {}", path.display());
        }
        Ok(Command::UninstallHooks) => {
            let path = default_hooks_path();
            if let Err(err) = uninstall_hooks_from(&path) {
                eprintln!("{err}");
                std::process::exit(1);
            }
            println!("Hooks do DeskPulse removidos de {}", path.display());
        }
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_serve_command() {
        assert_eq!(parse_cli(["deskpulse"]).unwrap(), Command::Serve);
        assert_eq!(parse_cli(["deskpulse", "serve"]).unwrap(), Command::Serve);
        assert_eq!(
            parse_cli(["deskpulse", "hook", "pre-tool"]).unwrap(),
            Command::HookPreTool
        );
        assert_eq!(
            parse_cli(["deskpulse", "hook", "stop"]).unwrap(),
            Command::HookStop
        );
        assert_eq!(
            parse_cli(["deskpulse", "install-hooks"]).unwrap(),
            Command::InstallHooks
        );
        assert_eq!(
            parse_cli(["deskpulse", "uninstall-hooks"]).unwrap(),
            Command::UninstallHooks
        );
        assert!(parse_cli(["deskpulse", "unknown"]).is_err());
    }
}
