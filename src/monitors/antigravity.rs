use serde::Serialize;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::watch;

const TRANSCRIPT_TAIL_BYTES: u64 = 16_384;
const TASK_MD_MAX_BYTES: u64 = 65_536;
const PBTXT_MAX_BYTES: u64 = 16_384;
const MAX_SESSIONS: usize = 5;
const MAX_SESSION_AGE: Duration = Duration::from_secs(24 * 3600);
const ACTIVE_LOCK_WINDOW: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AgentSessionSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub current_step: String,
    pub current_tool: Option<String>,
    pub task_done: usize,
    pub task_total: usize,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct AntigravitySnapshot {
    pub sessions: Vec<AgentSessionSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptTailInfo {
    pub step_type: String,
    pub status: String,
    pub latest_tool: Option<String>,
}

pub fn default_antigravity_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".gemini").join("antigravity")
}

fn read_capped_string(path: &Path, max_bytes: u64) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut buf = Vec::new();
    file.take(max_bytes).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

pub fn parse_pbtxt_title(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut i = 0usize;
    while i + 5 < bytes.len() {
        let boundary_ok = i == 0 || bytes[i - 1].is_ascii_whitespace();
        if boundary_ok && &bytes[i..i + 5] == b"title" {
            let mut j = i + 5;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b':' {
                j += 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'"' {
                    let rest = &raw[j + 1..];
                    let mut out = String::new();
                    let mut chars = rest.chars();
                    while let Some(ch) = chars.next() {
                        match ch {
                            '"' => {
                                let trimmed = out.trim();
                                return if trimmed.is_empty() {
                                    None
                                } else {
                                    Some(trimmed.to_string())
                                };
                            }
                            '\\' => match chars.next() {
                                Some('n') => out.push('\n'),
                                Some('r') => out.push('\r'),
                                Some('t') => out.push('\t'),
                                Some(escaped) => out.push(escaped),
                                None => break,
                            },
                            other => out.push(other),
                        }
                    }
                    return None;
                }
            }
        }
        i += 1;
    }
    None
}

pub fn parse_task_md(raw: &str) -> (usize, usize, Option<String>) {
    let mut done = 0usize;
    let mut total = 0usize;
    let mut active_item: Option<String> = None;

    for line in raw.lines() {
        let trimmed = line.trim_start();
        let after_bullet = if let Some(rest) = trimmed.strip_prefix('-') {
            rest.trim_start()
        } else if let Some(rest) = trimmed.strip_prefix('*') {
            rest.trim_start()
        } else {
            continue;
        };

        if after_bullet.starts_with("[x]") || after_bullet.starts_with("[X]") {
            done += 1;
            total += 1;
        } else if let Some(rest) = after_bullet.strip_prefix("[/]") {
            total += 1;
            if active_item.is_none() {
                let item = rest.trim();
                if !item.is_empty() {
                    active_item = Some(item.to_string());
                }
            }
        } else if after_bullet.starts_with("[ ]") {
            total += 1;
        }
    }

    (done, total, active_item)
}

fn extract_first_tool_name(obj: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    obj.get("tool_calls")?
        .as_array()?
        .first()?
        .get("name")?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn parse_transcript_tail(tail_bytes: &[u8]) -> Option<TranscriptTailInfo> {
    if tail_bytes.is_empty() {
        return None;
    }

    let text = String::from_utf8_lossy(tail_bytes);
    let mut step_type: Option<String> = None;
    let mut status: Option<String> = None;
    let mut latest_tool: Option<String> = None;

    for line in text.lines().rev() {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') {
            continue;
        }
        let Ok(serde_json::Value::Object(obj)) = serde_json::from_str::<serde_json::Value>(trimmed)
        else {
            continue;
        };

        let entry_type = obj.get("type").and_then(|v| v.as_str()).map(str::to_string);
        let entry_status = obj
            .get("status")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        if entry_type.is_none() && entry_status.is_none() {
            continue;
        }

        if step_type.is_none() {
            step_type = Some(entry_type.clone().unwrap_or_else(|| "UNKNOWN".to_string()));
            status = Some(entry_status.unwrap_or_else(|| "UNKNOWN".to_string()));
            latest_tool = extract_first_tool_name(&obj);
            if latest_tool.is_some() || step_type.as_deref() == Some("USER_INPUT") {
                break;
            }
        } else {
            if entry_type.as_deref() == Some("USER_INPUT") {
                break;
            }
            if let Some(tool) = extract_first_tool_name(&obj) {
                latest_tool = Some(tool);
                break;
            }
        }
    }

    Some(TranscriptTailInfo {
        step_type: step_type?,
        status: status?,
        latest_tool,
    })
}

fn read_transcript_tail(path: &Path) -> Option<TranscriptTailInfo> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let tail_len = len.min(TRANSCRIPT_TAIL_BYTES);
    file.seek(SeekFrom::End(-(tail_len as i64))).ok()?;
    let mut buf = Vec::with_capacity(tail_len as usize);
    file.take(TRANSCRIPT_TAIL_BYTES).read_to_end(&mut buf).ok()?;
    parse_transcript_tail(&buf)
}

fn resolve_transcript_path(brain_dir: &Path) -> Option<PathBuf> {
    let nested = brain_dir
        .join(".system_generated")
        .join("logs")
        .join("transcript.jsonl");
    if nested.is_file() {
        return Some(nested);
    }
    let flat = brain_dir.join("transcript.jsonl");
    if flat.is_file() {
        return Some(flat);
    }
    None
}

struct SessionCandidate {
    id: String,
    brain_dir: PathBuf,
    tx_path: Option<PathBuf>,
    mtime: SystemTime,
    age: Duration,
}

fn status_rank(status: &str) -> u8 {
    match status {
        "running" => 0,
        "waiting" => 1,
        _ => 2,
    }
}

pub fn scan_antigravity_dir(root: &Path) -> AntigravitySnapshot {
    scan_antigravity_dir_at(root, SystemTime::now())
}

fn scan_antigravity_dir_at(root: &Path, now: SystemTime) -> AntigravitySnapshot {
    let brain_root = root.join("brain");
    let Ok(entries) = fs::read_dir(&brain_root) else {
        return AntigravitySnapshot::default();
    };

    let annotations_root = root.join("annotations");
    let presence_root = root.join("presence");

    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        if id.is_empty() || id.starts_with('.') {
            continue;
        }

        let brain_dir = entry.path();
        let tx_path = resolve_transcript_path(&brain_dir);
        let task_path = brain_dir.join("task.md");
        let lock_path = presence_root.join(format!("{id}.lock"));

        let mtime = tx_path
            .as_deref()
            .and_then(|p| fs::metadata(p).ok()?.modified().ok())
            .or_else(|| fs::metadata(&task_path).ok()?.modified().ok())
            .or_else(|| fs::metadata(&lock_path).ok()?.modified().ok());

        let Some(mtime) = mtime else {
            continue;
        };

        let age = now.duration_since(mtime).unwrap_or(Duration::ZERO);
        if age > MAX_SESSION_AGE {
            continue;
        }

        candidates.push(SessionCandidate {
            id,
            brain_dir,
            tx_path,
            mtime,
            age,
        });
    }

    let mut sessions = Vec::with_capacity(candidates.len());
    for cand in candidates {
        let has_lock = presence_root.join(format!("{}.lock", cand.id)).is_file();

        let title = read_capped_string(
            &annotations_root.join(format!("{}.pbtxt", cand.id)),
            PBTXT_MAX_BYTES,
        )
        .and_then(|raw| parse_pbtxt_title(&raw))
        .unwrap_or_else(|| cand.id.clone());

        let (task_done, task_total, active_item) =
            read_capped_string(&cand.brain_dir.join("task.md"), TASK_MD_MAX_BYTES)
                .map(|raw| parse_task_md(&raw))
                .unwrap_or((0, 0, None));

        let tail = cand.tx_path.as_deref().and_then(read_transcript_tail);
        let last_status = tail.as_ref().map(|t| t.status.as_str()).unwrap_or("");

        let status = if has_lock {
            if (!last_status.is_empty() && !last_status.eq_ignore_ascii_case("DONE"))
                || cand.age <= ACTIVE_LOCK_WINDOW
            {
                "running"
            } else {
                "waiting"
            }
        } else {
            "idle"
        }
        .to_string();

        let current_step = active_item
            .or_else(|| tail.as_ref().map(|t| t.step_type.clone()))
            .unwrap_or_default();
        let current_tool = tail.and_then(|t| t.latest_tool);
        let updated_at_ms = cand
            .mtime
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        sessions.push(AgentSessionSummary {
            id: cand.id,
            title,
            status,
            current_step,
            current_tool,
            task_done,
            task_total,
            updated_at_ms,
        });
    }

    sessions.sort_by(|a, b| {
        status_rank(&a.status)
            .cmp(&status_rank(&b.status))
            .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
            .then_with(|| a.id.cmp(&b.id))
    });
    sessions.truncate(MAX_SESSIONS);

    AntigravitySnapshot { sessions }
}

pub struct AntigravityMonitor {
    latest: Arc<RwLock<AntigravitySnapshot>>,
    collector_thread: Arc<RwLock<Option<String>>>,
}

impl AntigravityMonitor {
    pub fn spawn(shutdown: watch::Receiver<bool>) -> Self {
        Self::spawn_for_dir(default_antigravity_dir(), shutdown)
    }

    pub fn spawn_for_dir(root: PathBuf, shutdown: watch::Receiver<bool>) -> Self {
        let latest = Arc::new(RwLock::new(AntigravitySnapshot::default()));
        let collector_thread = Arc::new(RwLock::new(None));
        let latest_bg = Arc::clone(&latest);
        let thread_bg = Arc::clone(&collector_thread);

        let _ = std::thread::Builder::new()
            .name("deskpulse-agy".to_string())
            .spawn(move || {
                if let Ok(mut name_w) = thread_bg.write() {
                    *name_w = std::thread::current().name().map(str::to_string);
                }

                let initial = scan_antigravity_dir(&root);
                if let Ok(mut w) = latest_bg.write() {
                    *w = initial;
                }

                loop {
                    for _ in 0..10 {
                        if *shutdown.borrow() {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    if *shutdown.borrow() {
                        return;
                    }

                    let snap = scan_antigravity_dir(&root);
                    if let Ok(mut w) = latest_bg.write() {
                        *w = snap;
                    }
                }
            });

        Self {
            latest,
            collector_thread,
        }
    }

    pub fn snapshot(&self) -> AntigravitySnapshot {
        self.latest
            .read()
            .ok()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    #[allow(dead_code)]
    pub fn collector_thread_name(&self) -> Option<String> {
        self.collector_thread.read().ok().and_then(|g| g.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_pbtxt_title_and_task_md_counts() {
        let pbtxt = r#"title:"DeskPulse \"Landscape\" Hub"  last_user_view_time:{seconds:1795012489  nanos:515000000}"#;
        assert_eq!(
            parse_pbtxt_title(pbtxt).as_deref(),
            Some(r#"DeskPulse "Landscape" Hub"#)
        );
        assert_eq!(
            parse_pbtxt_title(r#"subtitle:"nope" title: "Spaced Title""#).as_deref(),
            Some("Spaced Title")
        );
        assert_eq!(parse_pbtxt_title("last_user_view_time:{seconds:1}"), None);

        let task_md = "\
# Tasks
- [x] Done step one
  - [X] Done nested step
- [/] Implement Antigravity monitor
- [ ] Wire server endpoints
";
        assert_eq!(
            parse_task_md(task_md),
            (2, 4, Some("Implement Antigravity monitor".to_string()))
        );
        assert_eq!(parse_task_md("no checkboxes here"), (0, 0, None));
    }

    #[test]
    fn test_parse_transcript_tail_skips_partial_first_line_and_non_utf8_prefix() {
        let mut tail = Vec::new();
        // Leading mid-UTF-8 continuation bytes + partial JSON fragment
        tail.extend_from_slice(b"\x80\xbf\"content\":\"truncated\"}\n");
        tail.extend_from_slice(
            br#"{"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","tool_calls":[{"name":"read_file","args":{}}]}"#,
        );
        tail.push(b'\n');
        tail.extend_from_slice(
            br#"{"step_index":2,"type":"PLANNER_RESPONSE","status":"RUNNING","tool_calls":[{"name":"run_command","args":{"CommandLine":"cargo test"}}]}"#,
        );
        tail.push(b'\n');
        // Incomplete trailing line should also be ignored
        tail.extend_from_slice(br#"{"step_index":3,"type":"PLANNER"#);

        let info = parse_transcript_tail(&tail).expect("should parse valid tail line");
        assert_eq!(info.step_type, "PLANNER_RESPONSE");
        assert_eq!(info.status, "RUNNING");
        assert_eq!(info.latest_tool.as_deref(), Some("run_command"));

        // When last line is GENERIC after a tool_call, still extracts latest tool_calls[0].name
        let multi_tail = concat!(
            r#"{"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE","tool_calls":[{"name":"edit_file"}]}"#,
            "\n",
            r#"{"step_index":2,"type":"GENERIC","status":"DONE"}"#,
            "\n"
        );
        let multi_info = parse_transcript_tail(multi_tail.as_bytes()).expect("multi line");
        assert_eq!(multi_info.step_type, "GENERIC");
        assert_eq!(multi_info.status, "DONE");
        assert_eq!(multi_info.latest_tool.as_deref(), Some("edit_file"));

        assert_eq!(parse_transcript_tail(b"\xff\xfe{not json"), None);
        assert_eq!(parse_transcript_tail(b""), None);
    }

    #[test]
    fn test_scan_antigravity_dir_ranks_active_sessions_and_handles_missing_dir() {
        let dir = TempDir::new().expect("tempdir");
        let missing = dir.path().join("does-not-exist");
        assert!(scan_antigravity_dir(&missing).sessions.is_empty());

        let root = dir.path().join("antigravity");
        fs::create_dir_all(root.join("annotations")).unwrap();
        fs::create_dir_all(root.join("presence")).unwrap();

        // Session A: active running (lock exists, transcript > 16KB in .system_generated/logs/)
        let brain_a = root.join("brain").join("sess-a");
        fs::create_dir_all(brain_a.join(".system_generated").join("logs")).unwrap();
        fs::write(
            root.join("annotations").join("sess-a.pbtxt"),
            r#"title:"Active Hub Task" last_user_view_time:{seconds:100}"#,
        )
        .unwrap();
        fs::write(root.join("presence").join("sess-a.lock"), "").unwrap();
        fs::write(
            brain_a.join("task.md"),
            "- [x] Config\n- [/] Antigravity Monitor\n- [ ] Frontend\n",
        )
        .unwrap();

        let mut big_transcript = String::new();
        for i in 0..300 {
            big_transcript.push_str(&format!(
                r#"{{"step_index":{i},"type":"GENERIC","status":"DONE","content":"padding-line-0123456789abcdef0123456789abcdef"}}"#
            ));
            big_transcript.push('\n');
        }
        big_transcript.push_str(
            r#"{"step_index":301,"type":"PLANNER_RESPONSE","status":"IN_PROGRESS","tool_calls":[{"name":"run_command"}]}"#,
        );
        big_transcript.push('\n');
        fs::write(
            brain_a
                .join(".system_generated")
                .join("logs")
                .join("transcript.jsonl"),
            big_transcript,
        )
        .unwrap();

        // Session B: waiting (lock exists, DONE, > 15s old)
        let brain_b = root.join("brain").join("sess-b");
        fs::create_dir_all(&brain_b).unwrap();
        fs::write(
            root.join("annotations").join("sess-b.pbtxt"),
            r#"title:"Waiting Session""#,
        )
        .unwrap();
        fs::write(root.join("presence").join("sess-b.lock"), "").unwrap();
        let tx_b_path = brain_b.join("transcript.jsonl");
        fs::write(
            &tx_b_path,
            r#"{"step_index":2,"type":"PLANNER_RESPONSE","status":"DONE"}"#,
        )
        .unwrap();
        let old_60s = SystemTime::now() - Duration::from_secs(60);
        fs::File::options()
            .write(true)
            .open(&tx_b_path)
            .unwrap()
            .set_modified(old_60s)
            .unwrap();

        // Session C: idle (no lock, DONE)
        let brain_c = root.join("brain").join("sess-c");
        fs::create_dir_all(&brain_c).unwrap();
        fs::write(
            root.join("annotations").join("sess-c.pbtxt"),
            r#"title:"Idle Session""#,
        )
        .unwrap();
        let tx_c_path = brain_c.join("transcript.jsonl");
        fs::write(
            &tx_c_path,
            r#"{"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE"}"#,
        )
        .unwrap();
        let old_30s = SystemTime::now() - Duration::from_secs(30);
        fs::File::options()
            .write(true)
            .open(&tx_c_path)
            .unwrap()
            .set_modified(old_30s)
            .unwrap();

        // Session D: expired (>24h old) should be excluded
        let brain_d = root.join("brain").join("sess-d");
        fs::create_dir_all(&brain_d).unwrap();
        let tx_d_path = brain_d.join("transcript.jsonl");
        fs::write(
            &tx_d_path,
            r#"{"step_index":1,"type":"USER_INPUT","status":"DONE"}"#,
        )
        .unwrap();
        let old_25h = SystemTime::now() - Duration::from_secs(25 * 3600);
        fs::File::options()
            .write(true)
            .open(&tx_d_path)
            .unwrap()
            .set_modified(old_25h)
            .unwrap();

        let snap = scan_antigravity_dir(&root);
        assert_eq!(snap.sessions.len(), 3);
        assert_eq!(snap.sessions[0].id, "sess-a");
        assert_eq!(snap.sessions[0].title, "Active Hub Task");
        assert_eq!(snap.sessions[0].status, "running");
        assert_eq!(snap.sessions[0].current_step, "Antigravity Monitor");
        assert_eq!(snap.sessions[0].current_tool.as_deref(), Some("run_command"));
        assert_eq!(snap.sessions[0].task_done, 1);
        assert_eq!(snap.sessions[0].task_total, 3);

        assert_eq!(snap.sessions[1].id, "sess-b");
        assert_eq!(snap.sessions[1].status, "waiting");

        assert_eq!(snap.sessions[2].id, "sess-c");
        assert_eq!(snap.sessions[2].status, "idle");
    }

    #[test]
    fn test_antigravity_monitor_spawn_for_dir_populates_snapshot_and_thread_name() {
        let dir = TempDir::new().expect("tempdir");
        let root = dir.path().join("agy");
        let brain = root.join("brain").join("s1");
        fs::create_dir_all(&brain).unwrap();
        fs::create_dir_all(root.join("presence")).unwrap();
        fs::write(root.join("presence").join("s1.lock"), "").unwrap();
        fs::write(
            brain.join("transcript.jsonl"),
            r#"{"step_index":1,"type":"USER_INPUT","status":"DONE"}"#,
        )
        .unwrap();

        let (tx, rx) = watch::channel(false);
        let monitor = AntigravityMonitor::spawn_for_dir(root, rx);

        // Give the background thread a moment to complete its initial scan
        let mut got_sessions = false;
        for _ in 0..40 {
            if !monitor.snapshot().sessions.is_empty() {
                got_sessions = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(got_sessions, "monitor should populate initial snapshot");
        assert_eq!(
            monitor.collector_thread_name().as_deref(),
            Some("deskpulse-agy")
        );
        assert_eq!(monitor.snapshot().sessions[0].status, "running");

        let _ = tx.send(true);
    }

    #[test]
    fn test_scan_antigravity_dir_truncates_after_status_ranking() {
        let dir = TempDir::new().expect("tempdir");
        let root = dir.path().join("antigravity");
        fs::create_dir_all(root.join("annotations")).unwrap();
        fs::create_dir_all(root.join("presence")).unwrap();

        let now = SystemTime::now();

        // Create 6 newer idle sessions (no .lock)
        for i in 0..6 {
            let id = format!("idle-{i}");
            let brain = root.join("brain").join(&id);
            fs::create_dir_all(&brain).unwrap();
            let tx_path = brain.join("transcript.jsonl");
            fs::write(
                &tx_path,
                r#"{"step_index":1,"type":"PLANNER_RESPONSE","status":"DONE"}"#,
            )
            .unwrap();
            let mtime = now - Duration::from_secs(10 + i as u64);
            fs::File::options()
                .write(true)
                .open(&tx_path)
                .unwrap()
                .set_modified(mtime)
                .unwrap();
        }

        // Create 1 older-mtime active running session (has .lock)
        let active_id = "active-older";
        let active_brain = root.join("brain").join(active_id);
        fs::create_dir_all(&active_brain).unwrap();
        fs::write(root.join("presence").join(format!("{active_id}.lock")), "").unwrap();
        let active_tx = active_brain.join("transcript.jsonl");
        fs::write(
            &active_tx,
            r#"{"step_index":1,"type":"PLANNER_RESPONSE","status":"IN_PROGRESS"}"#,
        )
        .unwrap();
        let older_mtime = now - Duration::from_secs(3600);
        fs::File::options()
            .write(true)
            .open(&active_tx)
            .unwrap()
            .set_modified(older_mtime)
            .unwrap();

        let snap = scan_antigravity_dir_at(&root, now);
        assert_eq!(snap.sessions.len(), MAX_SESSIONS);
        assert_eq!(snap.sessions[0].id, active_id);
        assert_eq!(snap.sessions[0].status, "running");
    }
}
