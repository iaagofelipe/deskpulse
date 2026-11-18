mod approvals;
mod config;
mod events;
mod hooks;
mod security;

use std::env;

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

#[tokio::main]
async fn main() {
    match parse_cli(env::args()) {
        Ok(Command::Serve) => println!("deskpulse serve stub"),
        Ok(cmd) => println!("{cmd:?}"),
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
