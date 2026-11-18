use crate::approvals::{ApprovalInput, Decision};
use crate::events::{now_ms, NotificationEvent};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookKind {
    PreTool,
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreToolAction {
    Approve(ApprovalInput),
    AskQuestion { summary: String },
    Passthrough,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PreToolCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PreToolStdin {
    pub tool_call: PreToolCall,
    pub cwd: Option<String>,
    pub workspace_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct StopStdin {
    pub execution_num: u64,
    pub termination_reason: String,
    pub error: Option<String>,
    pub fully_idle: bool,
    pub continuation_active: bool,
}

static NEXT_STOP_ID: AtomicU64 = AtomicU64::new(1);

pub fn default_hooks_path() -> PathBuf {
    let home = env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".gemini").join("config").join("hooks.json")
}

fn first_str<'a>(obj: &'a Value, keys: &[&str]) -> Option<&'a str> {
    for k in keys {
        if let Some(s) = obj.get(*k).and_then(Value::as_str) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub fn parse_pre_tool(json: &str) -> Result<PreToolAction, String> {
    let parsed: PreToolStdin =
        serde_json::from_str(json).map_err(|e| format!("invalid PreToolUse payload: {e}"))?;
    let name = parsed.tool_call.name.trim();
    let args = &parsed.tool_call.args;

    let cwd = parsed
        .cwd
        .filter(|s| !s.trim().is_empty())
        .or_else(|| first_str(args, &["Cwd", "cwd"]).map(str::to_string))
        .or_else(|| parsed.workspace_paths.first().cloned())
        .unwrap_or_default();

    match name {
        "ask_question" => {
            let summary = args
                .get("questions")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|q| q.get("question").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join(" | ")
                })
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "Antigravity enviou uma pergunta no chat.".to_string());
            Ok(PreToolAction::AskQuestion {
                summary: truncate_chars(&summary, 400),
            })
        }
        "run_command" => {
            let cmd = first_str(args, &["CommandLine", "command", "cmd"])
                .unwrap_or("(comando sem texto)");
            Ok(PreToolAction::Approve(ApprovalInput {
                tool_name: name.to_string(),
                title: "Executar comando no terminal".to_string(),
                summary: truncate_chars(cmd, 500),
                cwd,
            }))
        }
        "write_file" => {
            let target = first_str(args, &["TargetFile", "Path", "path"]).unwrap_or("(arquivo)");
            Ok(PreToolAction::Approve(ApprovalInput {
                tool_name: name.to_string(),
                title: "Criar ou sobrescrever arquivo".to_string(),
                summary: truncate_chars(target, 400),
                cwd,
            }))
        }
        "edit_file" => {
            let target = first_str(args, &["TargetFile", "Path", "path"]).unwrap_or("(arquivo)");
            let note = first_str(args, &["Instruction", "instruction"]);
            let summary = match note {
                Some(n) => format!("{target} — {n}"),
                None => target.to_string(),
            };
            Ok(PreToolAction::Approve(ApprovalInput {
                tool_name: name.to_string(),
                title: "Editar arquivo".to_string(),
                summary: truncate_chars(&summary, 400),
                cwd,
            }))
        }
        "call_mcp_tool" => {
            let server = first_str(args, &["ServerName", "serverName"]).unwrap_or("mcp");
            let tool = first_str(args, &["ToolName", "toolName"]).unwrap_or("tool");
            Ok(PreToolAction::Approve(ApprovalInput {
                tool_name: name.to_string(),
                title: "Executar ferramenta MCP".to_string(),
                summary: format!("{server} :: {tool}"),
                cwd,
            }))
        }
        _ => Ok(PreToolAction::Passthrough),
    }
}

pub fn format_pre_tool_output(decision: Decision, reason: Option<&str>) -> Value {
    let decision_str = match decision {
        Decision::Allow => "allow",
        Decision::Deny => "deny",
        Decision::Ask => "ask",
    };
    let mut obj = Map::new();
    obj.insert("decision".to_string(), json!(decision_str));
    if let Some(r) = reason.filter(|s| !s.trim().is_empty()) {
        obj.insert("reason".to_string(), json!(r));
    }
    Value::Object(obj)
}

pub fn parse_stop(json: &str) -> NotificationEvent {
    let parsed: StopStdin = serde_json::from_str(json).unwrap_or_default();
    let seq = NEXT_STOP_ID.fetch_add(1, Ordering::Relaxed);
    let created_at_ms = now_ms();
    let reason = parsed.termination_reason.trim();
    let err = parsed.error.unwrap_or_default();

    let (level, title, body) = match reason {
        "ERROR" => (
            "error",
            "Antigravity encontrou um erro".to_string(),
            if err.trim().is_empty() {
                "Execução encerrada com erro.".to_string()
            } else {
                truncate_chars(err.trim(), 400)
            },
        ),
        "NO_TOOL_CALL" => (
            "success",
            "Antigravity finalizou a tarefa".to_string(),
            "Aguardando sua próxima instrução no chat.".to_string(),
        ),
        "MAX_INVOCATIONS" | "MAX_FORCED_INVOCATIONS" | "MAX_TOKEN_BUDGET_EXCEEDED" => (
            "warn",
            "Antigravity atingiu um limite".to_string(),
            format!("Motivo: {reason}"),
        ),
        other => (
            "info",
            if other.is_empty() {
                "Antigravity encerrou o turno".to_string()
            } else {
                format!("Antigravity encerrou ({other})")
            },
            if err.trim().is_empty() {
                "Turno finalizado.".to_string()
            } else {
                truncate_chars(err.trim(), 400)
            },
        ),
    };

    NotificationEvent {
        id: format!("stop-{created_at_ms}-{seq}"),
        kind: "stop".to_string(),
        title,
        body,
        level: level.to_string(),
        created_at_ms,
    }
}

fn read_hooks_map(path: &Path) -> Result<Map<String, Value>, String> {
    let raw = fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(Map::new());
    }
    let val: Value = serde_json::from_str(&raw)
        .map_err(|e| format!("failed to parse existing {}: {e}", path.display()))?;
    val.as_object()
        .cloned()
        .ok_or_else(|| format!("{} must contain a top-level JSON object", path.display()))
}

fn write_json_atomic(path: &Path, val: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    let pretty = serde_json::to_string_pretty(val)
        .map_err(|e| format!("failed to serialize hooks json: {e}"))?;
    let tmp_name = format!(
        ".{}.tmp.{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("hooks.json"),
        std::process::id()
    );
    let tmp = path.with_file_name(tmp_name);
    fs::write(&tmp, pretty.as_bytes())
        .map_err(|e| format!("failed to write temp hooks file: {e}"))?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("failed to replace {}: {e}", path.display()));
    }
    Ok(())
}

pub fn install_hooks_to(path: &Path, exe_path: &Path) -> Result<(), String> {
    let mut root = if path.exists() {
        let bak_name = format!(
            "{}.bak",
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("hooks.json")
        );
        let bak_path = path.with_file_name(bak_name);
        fs::copy(path, &bak_path)
            .map_err(|e| format!("failed to backup {} to {}: {e}", path.display(), bak_path.display()))?;
        read_hooks_map(path)?
    } else {
        Map::new()
    };

    let exe = exe_path.to_string_lossy();
    let entry = json!({
        "PreToolUse": [
            {
                "matcher": "run_command|write_file|edit_file|call_mcp_tool|ask_question",
                "hooks": [
                    {
                        "type": "command",
                        "command": format!("\"{exe}\" hook pre-tool"),
                        "timeout": 50
                    }
                ]
            }
        ],
        "Stop": [
            {
                "type": "command",
                "command": format!("\"{exe}\" hook stop"),
                "timeout": 5
            }
        ]
    });

    root.insert("deskpulse".to_string(), entry);
    write_json_atomic(path, &Value::Object(root))
}

pub fn uninstall_hooks_from(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let mut root = read_hooks_map(path)?;
    root.remove("deskpulse");
    write_json_atomic(path, &Value::Object(root))
}

pub async fn run_hook_cli(
    kind: HookKind,
    stdin_text: &str,
    base_url: &str,
    token: &str,
) -> String {
    let (endpoint, timeout, fallback) = match kind {
        HookKind::PreTool => (
            format!("{}/api/hooks/pre-tool", base_url.trim_end_matches('/')),
            Duration::from_secs(48),
            r#"{"decision":"ask"}"#.to_string(),
        ),
        HookKind::Stop => (
            format!("{}/api/hooks/stop", base_url.trim_end_matches('/')),
            Duration::from_secs(2),
            "{}".to_string(),
        ),
    };

    let Ok(client) = reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_millis(500))
        .timeout(timeout)
        .build()
    else {
        return fallback;
    };

    let resp = client
        .post(endpoint)
        .header("X-DeskPulse-Token", token)
        .header("Content-Type", "application/json")
        .body(stdin_text.to_string())
        .send()
        .await;

    let Ok(resp) = resp else {
        return fallback;
    };
    if !resp.status().is_success() {
        return fallback;
    }
    let Ok(text) = resp.text().await else {
        return fallback;
    };
    if serde_json::from_str::<Value>(&text).is_ok() {
        text
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    #[test]
    fn test_parses_run_command_write_edit_mcp_and_ask_question_payloads() {
        let cmd_json = r#"{
          "toolCall": { "id": "t1", "name": "run_command", "args": { "CommandLine": "cargo test" } },
          "cwd": "D:\\Projetos\\deskpulse"
        }"#;
        match parse_pre_tool(cmd_json).unwrap() {
            PreToolAction::Approve(input) => {
                assert_eq!(input.tool_name, "run_command");
                assert_eq!(input.summary, "cargo test");
                assert_eq!(input.cwd, "D:\\Projetos\\deskpulse");
            }
            other => panic!("unexpected action: {other:?}"),
        }

        let write_json = r#"{
          "toolCall": { "name": "write_file", "args": { "TargetFile": "src/main.rs" } }
        }"#;
        match parse_pre_tool(write_json).unwrap() {
            PreToolAction::Approve(input) => {
                assert_eq!(input.tool_name, "write_file");
                assert!(input.summary.contains("src/main.rs"));
            }
            other => panic!("unexpected action: {other:?}"),
        }

        let edit_json = r#"{
          "toolCall": { "name": "edit_file", "args": { "Path": "D:\\a.rs" } }
        }"#;
        match parse_pre_tool(edit_json).unwrap() {
            PreToolAction::Approve(input) => {
                assert_eq!(input.tool_name, "edit_file");
                assert!(input.summary.contains("D:\\a.rs"));
            }
            other => panic!("unexpected action: {other:?}"),
        }

        let mcp_json = r#"{
          "toolCall": { "name": "call_mcp_tool", "args": { "ServerName": "github", "ToolName": "create_issue" } }
        }"#;
        match parse_pre_tool(mcp_json).unwrap() {
            PreToolAction::Approve(input) => {
                assert_eq!(input.tool_name, "call_mcp_tool");
                assert!(input.summary.contains("github"));
                assert!(input.summary.contains("create_issue"));
            }
            other => panic!("unexpected action: {other:?}"),
        }

        let ask_json = r#"{
          "toolCall": { "name": "ask_question", "args": { "questions": [{ "question": "Continuar via USB?" }] } }
        }"#;
        match parse_pre_tool(ask_json).unwrap() {
            PreToolAction::AskQuestion { summary } => {
                assert!(summary.contains("Continuar via USB?"));
            }
            other => panic!("unexpected action: {other:?}"),
        }

        let read_json = r#"{
          "toolCall": { "name": "read_file", "args": { "Path": "Cargo.toml" } }
        }"#;
        assert_eq!(parse_pre_tool(read_json).unwrap(), PreToolAction::Passthrough);
    }

    #[test]
    fn test_formats_pre_tool_decisions_matching_antigravity_contract() {
        assert_eq!(
            format_pre_tool_output(Decision::Allow, None),
            json!({ "decision": "allow" })
        );
        assert_eq!(
            format_pre_tool_output(Decision::Deny, Some("Bloqueado no celular")),
            json!({ "decision": "deny", "reason": "Bloqueado no celular" })
        );
        assert_eq!(
            format_pre_tool_output(Decision::Ask, None),
            json!({ "decision": "ask" })
        );
    }

    #[test]
    fn test_parses_stop_payload_for_no_tool_call_and_error() {
        let done_ev = parse_stop(r#"{"terminationReason":"NO_TOOL_CALL","fullyIdle":true}"#);
        assert_eq!(done_ev.kind, "stop");
        assert_eq!(done_ev.level, "success");
        assert!(!done_ev.title.is_empty());

        let err_ev = parse_stop(r#"{"terminationReason":"ERROR","error":"Build failed"}"#);
        assert_eq!(err_ev.kind, "stop");
        assert_eq!(err_ev.level, "error");
        assert!(err_ev.body.contains("Build failed"));
    }

    #[tokio::test]
    async fn test_run_hook_cli_fails_open_when_agent_unreachable() {
        let start = Instant::now();
        let pre_out = run_hook_cli(
            HookKind::PreTool,
            r#"{"toolCall":{"name":"run_command","args":{"CommandLine":"echo hi"}}}"#,
            "http://127.0.0.1:59991",
            "tok",
        )
        .await;
        let parsed_pre: Value = serde_json::from_str(&pre_out).expect("valid pre_out json");
        assert_eq!(parsed_pre["decision"], "ask");
        assert!(start.elapsed() < Duration::from_secs(2));

        let stop_out = run_hook_cli(
            HookKind::Stop,
            r#"{"terminationReason":"NO_TOOL_CALL"}"#,
            "http://127.0.0.1:59991",
            "tok",
        )
        .await;
        let parsed_stop: Value = serde_json::from_str(&stop_out).expect("valid stop_out json");
        assert_eq!(parsed_stop, json!({}));
    }

    #[test]
    fn test_install_and_uninstall_hooks_preserve_existing_hooks_and_create_backup() {
        let dir = TempDir::new().unwrap();
        let hooks_path = dir.path().join("hooks.json");
        fs::write(
            &hooks_path,
            r#"{"existing_hook":{"PreToolUse":[{"matcher":"*","hooks":[{"type":"command","command":"echo ok"}]}]}}"#,
        )
        .unwrap();

        let exe = Path::new(r"D:\Projetos\deskpulse\target\release\deskpulse.exe");
        install_hooks_to(&hooks_path, exe).expect("install_hooks_to");

        let bak_path = dir.path().join("hooks.json.bak");
        assert!(bak_path.exists());

        let installed: Value =
            serde_json::from_str(&fs::read_to_string(&hooks_path).unwrap()).unwrap();
        assert!(installed.get("existing_hook").is_some());
        let dp = installed.get("deskpulse").expect("deskpulse entry");
        let pre = &dp["PreToolUse"][0];
        assert_eq!(
            pre["matcher"],
            "run_command|write_file|edit_file|call_mcp_tool|ask_question"
        );
        assert_eq!(pre["hooks"][0]["timeout"], 50);
        assert_eq!(
            pre["hooks"][0]["command"],
            r#""D:\Projetos\deskpulse\target\release\deskpulse.exe" hook pre-tool"#
        );
        assert_eq!(dp["Stop"][0]["timeout"], 5);
        assert_eq!(
            dp["Stop"][0]["command"],
            r#""D:\Projetos\deskpulse\target\release\deskpulse.exe" hook stop"#
        );

        uninstall_hooks_from(&hooks_path).expect("uninstall_hooks_from");
        let after: Value = serde_json::from_str(&fs::read_to_string(&hooks_path).unwrap()).unwrap();
        assert!(after.get("existing_hook").is_some());
        assert!(after.get("deskpulse").is_none());
    }
}
