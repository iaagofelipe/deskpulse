// DeskPulse v2 Appliance
mod agenda;
mod approvals;
mod config;
mod events;
mod hooks;
mod monitors;
mod security;
mod server;
mod shortcuts;
mod todos;
mod web_assets;

use std::env;
use std::io::Read;
use std::net::SocketAddr;
use std::process::Command as StdCommand;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

use crate::agenda::AgendaStore;
use crate::approvals::ApprovalStore;
use crate::events::EventBus;
use crate::hooks::{
    default_hooks_path, install_hooks_to, run_hook_cli, uninstall_hooks_from, HookKind,
};
use crate::monitors::adb::{find_adb_path, parse_adb_devices, AdbBridge};
use crate::monitors::antigravity::AntigravityMonitor;
use crate::monitors::media::WindowsMediaBackend;
use crate::monitors::system::SystemMonitor;
use crate::server::{build_router, AppState};
use crate::todos::TodoStore;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(Debug, PartialEq, Eq)]
pub enum TodoCommand {
    List,
    Add(String),
    Done(String),
    Rm(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum StartupCommand {
    Install,
    Uninstall,
    Status,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Serve { no_tray: bool },
    ConfigPath,
    Startup(StartupCommand),
    HookPreTool,
    HookStop,
    InstallHooks,
    UninstallHooks,
    Open,
    Todo(TodoCommand),
}

pub fn parse_cli<I, S>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let parts: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();
    match parts.get(1).map(String::as_str) {
        None => Ok(Command::Serve { no_tray: false }),
        Some("serve") => {
            if parts.len() == 2 {
                Ok(Command::Serve { no_tray: false })
            } else if parts.len() == 3 && parts[2] == "--no-tray" {
                Ok(Command::Serve { no_tray: true })
            } else {
                Err("usage: deskpulse serve [--no-tray]".to_string())
            }
        }
        Some("config-path") => {
            if parts.len() == 2 {
                Ok(Command::ConfigPath)
            } else {
                Err("usage: deskpulse config-path".to_string())
            }
        }
        Some("startup") => match parts.get(2).map(String::as_str) {
            Some("install") if parts.len() == 3 => Ok(Command::Startup(StartupCommand::Install)),
            Some("uninstall") if parts.len() == 3 => {
                Ok(Command::Startup(StartupCommand::Uninstall))
            }
            Some("status") if parts.len() == 3 => Ok(Command::Startup(StartupCommand::Status)),
            _ => Err("usage: deskpulse startup <install|uninstall|status>".to_string()),
        },
        Some("hook") => match parts.get(2).map(String::as_str) {
            Some("pre-tool") if parts.len() == 3 => Ok(Command::HookPreTool),
            Some("stop") if parts.len() == 3 => Ok(Command::HookStop),
            _ => Err("usage: deskpulse hook <pre-tool|stop>".to_string()),
        },
        Some("install-hooks") if parts.len() == 2 => Ok(Command::InstallHooks),
        Some("uninstall-hooks") if parts.len() == 2 => Ok(Command::UninstallHooks),
        Some("open") if parts.len() == 2 => Ok(Command::Open),
        Some("todo") => match parts.get(2).map(String::as_str) {
            Some("list") if parts.len() == 3 => Ok(Command::Todo(TodoCommand::List)),
            Some("add") if parts.len() >= 4 => {
                let text = parts[3..].join(" ");
                if text.trim().is_empty() {
                    Err("usage: deskpulse todo add <text...>".to_string())
                } else {
                    Ok(Command::Todo(TodoCommand::Add(text)))
                }
            }
            Some("done") if parts.len() == 4 => {
                Ok(Command::Todo(TodoCommand::Done(parts[3].clone())))
            }
            Some("rm") if parts.len() == 4 => Ok(Command::Todo(TodoCommand::Rm(parts[3].clone()))),
            _ => Err(
                "usage: deskpulse todo <list|add <text...>|done <id|index>|rm <id|index>>"
                    .to_string(),
            ),
        },
        _ => Err(
            "usage: deskpulse [serve [--no-tray]|config-path|startup <install|uninstall|status>|open|todo <list|add|done|rm>|hook <pre-tool|stop>|install-hooks|uninstall-hooks]"
                .to_string(),
        ),
    }
}

async fn run_serve() -> Result<(), String> {
    let cfg_path = config::default_config_path();
    let initial_cfg = config::load_or_create(&cfg_path)?;

    let addr: SocketAddr = ([127, 0, 0, 1], initial_cfg.port).into();
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("failed to bind {addr}: {e}"))?;

    let data_dir = config::deskpulse_data_dir();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let events = Arc::new(EventBus::new());
    let approvals = Arc::new(ApprovalStore::new(
        Arc::clone(&events),
        Duration::from_secs(initial_cfg.approval_timeout_secs),
    ));
    let system = Arc::new(SystemMonitor::spawn(shutdown_rx.clone()));
    let adb = Arc::new(AdbBridge::spawn(initial_cfg.port, shutdown_rx.clone()));
    let antigravity = Arc::new(AntigravityMonitor::spawn(shutdown_rx.clone()));
    let agenda = Arc::new(AgendaStore::new(
        data_dir.join("agenda_push.json"),
        initial_cfg.calendar_ics_url.clone(),
    ));
    agenda.spawn_poller(shutdown_rx.clone());
    let todos = Arc::new(TodoStore::open(data_dir.join("todos.json"))?);
    let media = Arc::new(WindowsMediaBackend::new());

    let (config_live, mut cfg_watcher) = config::ConfigWatcher::spawn(
        cfg_path.clone(),
        initial_cfg.clone(),
        shutdown_rx.clone(),
    );

    antigravity.update_budgets(
        initial_cfg.ai.budget_5h_tokens,
        initial_cfg.ai.budget_weekly_tokens,
    );

    let network = Arc::new(crate::monitors::network::NetworkMonitor::spawn(data_dir.join("network_usage.json"), shutdown_rx.clone()));
    let weather = Arc::new(crate::monitors::weather::WeatherMonitor::spawn(
        initial_cfg.weather.clone(),
        shutdown_rx.clone(),
    ));
    let github = Arc::new(crate::monitors::github::GithubMonitor::spawn(
        initial_cfg.github.clone(),
        shutdown_rx.clone(),
    ));
    let shortcuts = Arc::new(crate::shortcuts::ShortcutRunner::new(initial_cfg.shortcuts.clone()));
    let remote_approval_enabled = Arc::new(AtomicBool::new(initial_cfg.remote_approval_enabled));
    
    // Background watcher
    let mut shutdown_rx_bg = shutdown_rx.clone();
    let weather_clone = Arc::clone(&weather);
    let github_clone = Arc::clone(&github);
    let shortcuts_clone = Arc::clone(&shortcuts);
    let antigravity_clone = Arc::clone(&antigravity);
    let agenda_clone = Arc::clone(&agenda);
    let remote_approval_enabled_clone = Arc::clone(&remote_approval_enabled);
    
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown_rx_bg.changed() => break,
                res = cfg_watcher.changed() => {
                    if let Ok(_) = res {
                        let new_cfg = cfg_watcher.borrow_and_update().clone();
                        weather_clone.update_config(new_cfg.weather.clone());
                        github_clone.update_config(new_cfg.github.clone());
                        shortcuts_clone.update_config(new_cfg.shortcuts.clone());
                        antigravity_clone.update_budgets(new_cfg.ai.budget_5h_tokens, new_cfg.ai.budget_weekly_tokens);
                        agenda_clone.set_ics_url(new_cfg.calendar_ics_url.clone());
                        remote_approval_enabled_clone.store(new_cfg.remote_approval_enabled, std::sync::atomic::Ordering::Relaxed);
                    } else {
                        break;
                    }
                }
            }
        }
    });

    let app = build_router(AppState {
        token: initial_cfg.token.clone(),
        port: initial_cfg.port,
        config_path: cfg_path.clone(),
        events,
        approvals,
        system,
        adb,
        media,
        antigravity,
        agenda,
        todos,
        remote_approval_enabled,
        calendar_ics_url: Arc::new(Mutex::new(initial_cfg.calendar_ics_url.clone())),
        network,
        weather,
        github,
        shortcuts,
        config_live,
    });

    println!(
        "DeskPulse rodando em http://127.0.0.1:{}/#token={}",
        initial_cfg.port, initial_cfg.token
    );
    println!("Config: {}", cfg_path.display());
    println!(
        "Plugue o celular Android via USB (Depuracao USB ativa) e abra http://127.0.0.1:{}/#token={}",
        initial_cfg.port, initial_cfg.token
    );

    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|e| format!("server error: {e}"));

    let _ = shutdown_tx.send(true);
    result
}

fn run_adb_sync(adb_path: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = StdCommand::new(adb_path);
    cmd.args(args);
    #[cfg(windows)]
    cmd.creation_flags(crate::monitors::CREATE_NO_WINDOW);

    let out = cmd
        .output()
        .map_err(|e| format!("failed to run adb: {e}"))?;
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

fn run_open() -> Result<(), String> {
    let cfg = config::load_or_create(&config::default_config_path())?;
    let adb_path = find_adb_path()
        .ok_or_else(|| "adb.exe nao encontrado no PATH ou Android SDK".to_string())?;
    let devices_out = run_adb_sync(&adb_path, &["devices"])?;
    let devices = parse_adb_devices(&devices_out);
    let serial = devices
        .first()
        .ok_or_else(|| "nenhum dispositivo Android conectado via USB (adb devices vazio)".to_string())?;

    let url = format!("http://127.0.0.1:{}/#token={}", cfg.port, cfg.token);
    run_adb_sync(
        &adb_path,
        &[
            "-s",
            serial,
            "shell",
            "am",
            "start",
            "-a",
            "android.intent.action.VIEW",
            "-d",
            &url,
        ],
    )?;
    println!(
        "DeskPulse aberto no dispositivo {serial} (http://127.0.0.1:{}/)",
        cfg.port
    );
    Ok(())
}

fn run_todo(sub: TodoCommand) -> Result<(), String> {
    let path = config::deskpulse_data_dir().join("todos.json");
    let store = TodoStore::open(path)?;
    match sub {
        TodoCommand::List => {
            let items = store.list();
            if items.is_empty() {
                println!("Nenhum to-do.");
            } else {
                for (i, item) in items.iter().enumerate() {
                    let mark = if item.done { "x" } else { " " };
                    println!("{}. [{}] {} ({})", i + 1, mark, item.text, item.id);
                }
            }
        }
        TodoCommand::Add(text) => {
            let item = store.add(&text)?;
            println!("Adicionado: [ ] {} ({})", item.text, item.id);
        }
        TodoCommand::Done(id_or_idx) => match store.set_done(&id_or_idx, true)? {
            Some(item) => println!("Concluido: {} ({})", item.text, item.id),
            None => return Err(format!("to-do nao encontrado: {id_or_idx}")),
        },
        TodoCommand::Rm(id_or_idx) => {
            if store.remove(&id_or_idx)? {
                println!("Removido: {id_or_idx}");
            } else {
                return Err(format!("to-do nao encontrado: {id_or_idx}"));
            }
        }
    }
    Ok(())
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
        Ok(Command::Serve { no_tray }) => {
            let _ = no_tray;
            if let Err(err) = run_serve().await {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        Ok(Command::ConfigPath) => {
            println!("{}", config::default_config_path().display());
        }
        Ok(Command::Startup(StartupCommand::Install)) => {
            if let Err(err) = config::install_startup() {
                eprintln!("{err}");
                std::process::exit(1);
            }
            println!("Auto-start do DeskPulse instalado com sucesso.");
        }
        Ok(Command::Startup(StartupCommand::Uninstall)) => {
            if let Err(err) = config::uninstall_startup() {
                eprintln!("{err}");
                std::process::exit(1);
            }
            println!("Auto-start do DeskPulse removido com sucesso.");
        }
        Ok(Command::Startup(StartupCommand::Status)) => {
            let status = config::startup_status();
            println!("Auto-start: {}", if status { "ativo" } else { "inativo" });
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
        Ok(Command::Open) => {
            if let Err(err) = run_open() {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        Ok(Command::Todo(sub)) => {
            if let Err(err) = run_todo(sub) {
                eprintln!("{err}");
                std::process::exit(1);
            }
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
        assert_eq!(
            parse_cli(["deskpulse"]).unwrap(),
            Command::Serve { no_tray: false }
        );
        assert_eq!(
            parse_cli(["deskpulse", "serve"]).unwrap(),
            Command::Serve { no_tray: false }
        );
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

    #[test]
    fn test_parse_cli_todo_and_open_subcommands() {
        assert_eq!(parse_cli(["deskpulse", "open"]).unwrap(), Command::Open);
        assert!(parse_cli(["deskpulse", "open", "extra"]).is_err());

        assert_eq!(
            parse_cli(["deskpulse", "todo", "list"]).unwrap(),
            Command::Todo(TodoCommand::List)
        );
        assert_eq!(
            parse_cli(["deskpulse", "todo", "add", "Comprar", "cafe"]).unwrap(),
            Command::Todo(TodoCommand::Add("Comprar cafe".to_string()))
        );
        assert_eq!(
            parse_cli(["deskpulse", "todo", "done", "1"]).unwrap(),
            Command::Todo(TodoCommand::Done("1".to_string()))
        );
        assert_eq!(
            parse_cli(["deskpulse", "todo", "rm", "abc-1"]).unwrap(),
            Command::Todo(TodoCommand::Rm("abc-1".to_string()))
        );

        assert!(parse_cli(["deskpulse", "todo"]).is_err());
        assert!(parse_cli(["deskpulse", "todo", "add"]).is_err());
        assert!(parse_cli(["deskpulse", "todo", "done"]).is_err());
        assert!(parse_cli(["deskpulse", "todo", "rm"]).is_err());
    }

    #[test]
    fn test_parse_cli_v2_subcommands() {
        assert_eq!(
            parse_cli(["deskpulse", "serve", "--no-tray"]).unwrap(),
            Command::Serve { no_tray: true }
        );
        assert_eq!(
            parse_cli(["deskpulse", "config-path"]).unwrap(),
            Command::ConfigPath
        );
        assert_eq!(
            parse_cli(["deskpulse", "startup", "install"]).unwrap(),
            Command::Startup(StartupCommand::Install)
        );
        assert_eq!(
            parse_cli(["deskpulse", "startup", "uninstall"]).unwrap(),
            Command::Startup(StartupCommand::Uninstall)
        );
        assert_eq!(
            parse_cli(["deskpulse", "startup", "status"]).unwrap(),
            Command::Startup(StartupCommand::Status)
        );

        // Invalid flags / unknown subcommands return Err
        assert!(parse_cli(["deskpulse", "serve", "--unknown"]).is_err());
        assert!(parse_cli(["deskpulse", "config-path", "extra"]).is_err());
        assert!(parse_cli(["deskpulse", "startup"]).is_err());
        assert!(parse_cli(["deskpulse", "startup", "invalid"]).is_err());
        assert!(parse_cli(["deskpulse", "startup", "install", "extra"]).is_err());
    }
}
