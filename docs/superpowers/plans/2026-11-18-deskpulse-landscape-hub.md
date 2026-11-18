# DeskPulse Landscape Hub Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform DeskPulse into a zero-scroll, fullscreen-capable horizontal Galaxy S8+ desk hub (Layout 3A) featuring live Antigravity session monitoring, policy-respecting non-blocking hooks, Google Calendar sync (`.ics` + `gcalendar` push), a chat/touch To-Do list, and legible PC telemetry.

**Architecture:** Extend the existing Axum + Tokio Rust binary (`src/`) with three focused state/monitor modules (`monitors/antigravity.rs`, `agenda.rs`, `todos.rs`), extend `AppConfig` and `PcSnapshot`, wire new `/api/*` + PWA asset routes into `server.rs` and CLI subcommands into `main.rs`, and replace `web/index.html` with a fixed `100dvh` landscape SPA matching the 3A design.

**Tech Stack:** Rust (Axum 0.8, Tokio 1, Reqwest 0.12 `rustls-tls`, Chrono 0.4, Sysinfo 0.33, Serde), Windows GNU toolchain (`D:\Rust`), embedded HTML/CSS/JS + PWA assets (`include_str!`/`include_bytes!`).

**Spec:** [docs/superpowers/specs/2026-11-18-deskpulse-landscape-hub-design.md](file:///D:/Projetos/deskpulse/docs/superpowers/specs/2026-11-18-deskpulse-landscape-hub-design.md)

## Global Constraints

- Listen strictly on `127.0.0.1:8765` (USB ADB reverse only; zero `0.0.0.0` exposure).
- Keep existing `guard_middleware` security (`Host`, `Origin`, `Sec-Fetch-Site`, constant-time `X-DeskPulse-Token` / `Bearer` on `/api/*`).
- Root layout locked to `100vw × 100dvh`, `overflow: hidden` (zero vertical scroll on Overview), Layout 3A with 5-button bottom navigation bar (`Visão Geral`, `Agentes IA`, `Agenda & To-Do`, `Monitor PC`, `⛶ Tela Cheia`).
- `PreToolUse` defaults to non-blocking `{"decision":"ask"}` (`remote_approval_enabled: false`) unless explicitly opted in.
- Keep `RUSTUP_HOME`, `CARGO_HOME`, `TEMP`, `TMP`, and build outputs on `D:\Rust` / `D:\Projetos\deskpulse` (`C:\` has minimal free space).

## Review Focus

1. **Legacy `config.json` backward compatibility:** Existing `%APPDATA%\DeskPulse\config.json` lacks `calendar_ics_url` and `remote_approval_enabled`; loading it must default them to `None` and `false` instead of failing startup (pinned in Task 1).
2. **Large or mid-UTF-8 `transcript.jsonl` tails:** Tail-reading only the last ~16 KB can land inside a multi-byte UTF-8 character or partial JSON line; reader must decode lossily and skip partial/malformed lines without panicking or reading full files (pinned in Task 3).
3. **Real-world `.ics` edge cases:** Folded lines (`\r\n `), UTC `Z` vs floating/all-day `YYYYMMDD` dates, and `VEVENT`s missing `DTEND` must parse cleanly into local dates and never panic (pinned in Task 4).
4. **Concurrent CLI vs server writes to `todos.json`:** `deskpulse todo ...` from CLI while `deskpulse serve` runs must reload external disk modifications by mtime and write atomically with Windows lock retries (pinned in Task 5).
5. **PWA installability vs `/api/*` auth boundary:** `/manifest.webmanifest`, `/icon-192.png`, `/icon-512.png`, `/icon.svg`, and `/sw.js` must succeed without `X-DeskPulse-Token` while enforcing `Host`/`Origin`/`Sec-Fetch-Site`, whereas `/api/*` must still return `401` without a valid token (pinned in Task 6).

---

### Task 1: Dependencies & Backward-Compatible Config Extension

**Files:**
- Modify: `Cargo.toml:12-18`
- Modify: `src/config.rs:7-93`

**Interfaces:**
- Produces:
  - `AppConfig { pub port: u16, pub token: String, pub approval_timeout_secs: u64, pub calendar_ics_url: Option<String>, pub remote_approval_enabled: bool }`
  - `pub fn save_config(path: &Path, cfg: &AppConfig) -> Result<(), String>`
  - `pub fn deskpulse_data_dir() -> PathBuf`

- [ ] **Step 1: Add `chrono` and `reqwest` `rustls-tls` in `Cargo.toml`**

Add `rustls-tls` to `reqwest` features and `chrono = { version = "0.4", default-features = false, features = ["clock", "std"] }`.

- [ ] **Step 2: Write the failing tests in `src/config.rs`**

```rust
#[test]
fn test_load_legacy_config_defaults_new_fields_and_save_config_persists() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("DeskPulse").join("config.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        r#"{"port":8765,"token":"abc123","approval_timeout_secs":45}"#,
    )
    .unwrap();

    let mut cfg = load_or_create(&path).expect("legacy config should load");
    assert_eq!(cfg.calendar_ics_url, None);
    assert!(!cfg.remote_approval_enabled);

    cfg.calendar_ics_url = Some("https://calendar.google.com/calendar/ical/test.ics".to_string());
    cfg.remote_approval_enabled = true;
    save_config(&path, &cfg).expect("save_config");

    let reloaded = load_or_create(&path).expect("reload saved config");
    assert_eq!(reloaded, cfg);
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test config::tests`
Expected: FAIL (missing `calendar_ics_url`, `remote_approval_enabled`, `save_config`).

- [ ] **Step 4: Implement `AppConfig` extensions, `deskpulse_data_dir()`, and `save_config()` in `src/config.rs`**

Add `#[serde(default)]` on `calendar_ics_url: Option<String>` and `remote_approval_enabled: bool`, extract atomic write with 5x `25ms` Windows rename retry into `save_config(path: &Path, cfg: &AppConfig) -> Result<(), String>`, and call `save_config` from `load_or_create`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test config::tests`
Expected: PASS (2 passed).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/config.rs
git commit -m "feat: extend AppConfig with calendar URL, remote approval toggle, and save_config"
```

---

### Task 2: Rolling CPU History in `SystemMonitor`

**Files:**
- Modify: `src/monitors/system.rs:28-39,145-238,256-316`

**Interfaces:**
- Produces: `PcSnapshot.cpu_history: Vec<u8>` (rolling window capped at 30 samples, clamped `0..=100`).

- [ ] **Step 1: Write the failing test in `src/monitors/system.rs`**

```rust
#[test]
fn test_push_cpu_history_caps_at_30_and_clamps_values() {
    let mut history = Vec::new();
    for i in 0..35 {
        push_cpu_history(&mut history, (i * 4) as f32);
    }
    assert_eq!(history.len(), 30);
    assert_eq!(history.first().copied(), Some(20));
    assert_eq!(history.last().copied(), Some(100));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test monitors::system::tests`
Expected: FAIL (`push_cpu_history` not defined).

- [ ] **Step 3: Add `cpu_history: Vec<u8>` to `PcSnapshot` and `push_cpu_history(history: &mut Vec<u8>, cpu_pct: f32)`**

Maintain a 30-element ring in the `deskpulse-sysmon` collector loop and attach a clone to each `PcSnapshot`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test monitors::system::tests`
Expected: PASS (4 passed).

- [ ] **Step 5: Commit**

```bash
git add src/monitors/system.rs
git commit -m "feat: add 30-sample cpu_history trend to PcSnapshot"
```

---

### Task 3: Live Antigravity Session Monitor (`src/monitors/antigravity.rs`)

**Files:**
- Create: `src/monitors/antigravity.rs`
- Modify: `src/monitors/mod.rs:1-6`

**Interfaces:**
- Produces:
  - `AgentSessionSummary { pub id: String, pub title: String, pub status: String, pub current_step: String, pub current_tool: Option<String>, pub task_done: usize, pub task_total: usize, pub updated_at_ms: u64 }`
  - `AntigravitySnapshot { pub sessions: Vec<AgentSessionSummary> }`
  - `AntigravityMonitor::spawn(shutdown: watch::Receiver<bool>) -> Self`
  - `AntigravityMonitor::spawn_for_dir(root: PathBuf, shutdown: watch::Receiver<bool>) -> Self`
  - `AntigravityMonitor::snapshot(&self) -> AntigravitySnapshot`
  - `pub fn parse_pbtxt_title(raw: &str) -> Option<String>`
  - `pub fn parse_task_md(raw: &str) -> (usize, usize, Option<String>)`
  - `pub fn parse_transcript_tail(tail_bytes: &[u8]) -> Option<TranscriptTailInfo>`

- [ ] **Step 1: Write the failing unit tests in `src/monitors/antigravity.rs`**

Include tests for:
1. `test_parse_pbtxt_title_and_task_md_counts` (`- [x]`, `- [/]`, `- [ ]` parsing + active item extraction).
2. `test_parse_transcript_tail_skips_partial_first_line_and_non_utf8_prefix` (tail slice starting mid-UTF-8/mid-JSON still extracts last valid JSONL line's `type`, `status`, and latest `tool_calls[0].name`).
3. `test_scan_antigravity_dir_ranks_active_sessions_and_handles_missing_dir` (temp dir with `annotations/*.pbtxt`, `presence/*.lock`, `brain/<id>/task.md`, `transcript.jsonl`, plus non-existent dir returning empty `sessions`).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test monitors::antigravity::tests`
Expected: FAIL (module does not exist).

- [ ] **Step 3: Implement `src/monitors/antigravity.rs` and export in `src/monitors/mod.rs`**

Scan on a `deskpulse-agy` OS thread every `2s` (with `200ms` shutdown slices matching `SystemMonitor`):
- Look under `%USERPROFILE%\.gemini\antigravity` (`brain`, `annotations`, `presence`).
- Collect candidates modified within 24h (up to 5 newest by `transcript.jsonl` mtime).
- Read only the last `16_384` bytes of `transcript.jsonl` via `SeekFrom::End`; decode with `String::from_utf8_lossy` and scan lines from bottom up for valid JSON.
- Infer `status`: `"running"` if lock exists and (`last_status != "DONE"` or age <= `15s`); `"waiting"` if lock exists and idle; `"idle"` otherwise.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test monitors::antigravity::tests`
Expected: PASS (3 passed).

- [ ] **Step 5: Commit**

```bash
git add src/monitors/mod.rs src/monitors/antigravity.rs
git commit -m "feat: add live Antigravity session monitor reading local transcript and task state"
```

---

### Task 4: Google Agenda `.ics` + Push Store (`src/agenda.rs`)

**Files:**
- Create: `src/agenda.rs`
- Modify: `src/main.rs:1-8` (declare `mod agenda;`)

**Interfaces:**
- Produces:
  - `AgendaEvent { pub id: String, pub title: String, pub start_iso: String, pub end_iso: Option<String>, pub all_day: bool, pub source: String }`
  - `AgendaSnapshot { pub configured: bool, pub ics_configured: bool, pub last_sync_ms: Option<u64>, pub error: Option<String>, pub events: Vec<AgendaEvent> }`
  - `AgendaStore::new(push_path: PathBuf, initial_ics_url: Option<String>) -> Self`
  - `AgendaStore::spawn_poller(self: &Arc<Self>, shutdown: watch::Receiver<bool>)`
  - `AgendaStore::set_ics_url(&self, url: Option<String>)`
  - `AgendaStore::replace_pushed(&self, events: Vec<AgendaEvent>) -> Result<(), String>`
  - `AgendaStore::refresh_now(&self) -> Result<AgendaSnapshot, String>` (async)
  - `AgendaStore::snapshot(&self) -> AgendaSnapshot`
  - `pub fn parse_ics_for_date(ics: &str, target_date: chrono::NaiveDate) -> Vec<AgendaEvent>`

- [ ] **Step 1: Write the failing unit tests in `src/agenda.rs`**

Include tests for:
1. `test_parse_ics_handles_unfolding_utc_to_local_all_day_and_missing_dtend` (folded `SUMMARY`, `DTSTART:20261118T170000Z` -> local RFC3339, `DTSTART;VALUE=DATE:20261118` all-day event, and `VEVENT` without `DTEND`).
2. `test_merge_deduplicates_and_preserves_pushed_events_across_ics_updates_and_expires_old_days` (push persisted to `agenda_push.json`, past-day pushed events pruned on load/snapshot, `(title, start_iso)` deduplication preferring `"push"`, `configured` vs `ics_configured` flags).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test agenda::tests`
Expected: FAIL (module does not exist).

- [ ] **Step 3: Implement `src/agenda.rs`**

Implement ICS unfolding (`\r\n ` / `\n `), `VEVENT` parsing with `chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone}`, atomic persistence of `agenda_push.json` with Windows rename retry, `reqwest` HTTPS `.ics` fetch (`10s` timeout), `5 min` background poller, and merge/dedup logic.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test agenda::tests`
Expected: PASS (2 passed).

- [ ] **Step 5: Commit**

```bash
git add src/agenda.rs src/main.rs
git commit -m "feat: add AgendaStore with ICS polling and persistent gcalendar push merge"
```

---

### Task 5: Persisted To-Do Store (`src/todos.rs`)

**Files:**
- Create: `src/todos.rs`
- Modify: `src/main.rs:1-8` (declare `mod todos;`)

**Interfaces:**
- Produces:
  - `TodoItem { pub id: String, pub text: String, pub done: bool, pub created_at_ms: u64 }`
  - `TodoStore::open(path: PathBuf) -> Result<Self, String>`
  - `TodoStore::list(&self) -> Vec<TodoItem>`
  - `TodoStore::add(&self, text: &str) -> Result<TodoItem, String>`
  - `TodoStore::toggle(&self, id_or_index: &str) -> Result<Option<TodoItem>, String>`
  - `TodoStore::set_done(&self, id_or_index: &str, done: bool) -> Result<Option<TodoItem>, String>`
  - `TodoStore::remove(&self, id_or_index: &str) -> Result<bool, String>`
  - `TodoStore::clear_done(&self) -> Result<usize, String>`

- [ ] **Step 1: Write the failing unit tests in `src/todos.rs`**

Include tests for:
1. `test_todo_crud_persists_atomically_and_supports_1based_index_or_id` (`add`, `toggle`, `set_done`, `clear_done`, `remove`, reload from disk).
2. `test_todo_store_reloads_when_external_writer_updates_file` (simulate CLI writing `todos.json` externally; existing `TodoStore::list()` detects mtime/content change and reflects the new item).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test todos::tests`
Expected: FAIL (module does not exist).

- [ ] **Step 3: Implement `src/todos.rs`**

Implement `TodoStore` with `Mutex<TodoInner>` (tracking `last_mtime` + `items`), reloading from disk if mtime changed before reads/writes, rejecting blank text, and writing atomically with 5x `25ms` retry.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test todos::tests`
Expected: PASS (2 passed).

- [ ] **Step 5: Commit**

```bash
git add src/todos.rs src/main.rs
git commit -m "feat: add atomic persisted TodoStore with external-edit reload"
```

---

### Task 6: Server Endpoints, Policy-Respecting Hooks, PWA Assets & CLI (`src/server.rs`, `src/main.rs`, `assets/`)

**Files:**
- Create: `assets/icon-192.png`, `assets/icon-512.png`, `assets/icon.svg`, `assets/manifest.webmanifest`, `assets/sw.js`
- Modify: `src/server.rs:1-530`
- Modify: `src/main.rs:25-184`

**Interfaces:**
- Consumes: `AppConfig`, `AntigravityMonitor`, `AgendaStore`, `TodoStore`.
- Produces:
  - Extended `AppState` and `DashboardState` (`pc`, `media`, `adb`, `antigravity`, `agenda`, `todos`, `notifications`, `approvals`, `remote_approval_enabled`).
  - Public routes: `/`, `/manifest.webmanifest`, `/icon-192.png`, `/icon-512.png`, `/icon.svg`, `/sw.js`.
  - Authenticated `/api/*` routes: `/api/todos`, `/api/todos/{id}/toggle`, `/api/todos/{id}` (`DELETE`), `/api/todos/clear-done`, `/api/agenda`, `/api/agenda/refresh`, `/api/settings`, `/api/system/lock`.
  - CLI commands: `Command::Open`, `Command::Todo(...)` (`list`, `add`, `done`, `rm`).

- [ ] **Step 1: Create static PWA assets (`assets/`)**

Create minimal valid PNG icons (`192x192` and `512x512` dark `#0b0d11` rounded badge with `#4c8df6` pulse accent, generated reproducibly via a small one-shot script into `assets/`), `assets/icon.svg`, `assets/manifest.webmanifest` (`"display": "fullscreen"`, `"orientation": "landscape"`, `"start_url": "/"`), and `assets/sw.js` (`self.addEventListener("fetch", () => {});`).

- [ ] **Step 2: Write the failing server & CLI tests in `src/server.rs` and `src/main.rs`**

Add tests for:
1. `test_pre_tool_hook_defaults_to_non_blocking_ask_and_respects_settings_toggle` (with `remote_approval_enabled: false`, `POST /api/hooks/pre-tool` returns `{"decision":"ask"}` immediately even when SSE client is active; after `POST /api/settings` `{"remote_approval_enabled":true}`, `pre-tool` blocks for phone decision).
2. `test_pwa_assets_accessible_without_token_while_api_remains_protected` (`/manifest.webmanifest`, `/icon-192.png`, `/icon-512.png`, `/icon.svg`, `/sw.js` return `200` on `127.0.0.1:8765` without token and `403` on foreign `Host`; `/api/todos` returns `401` without token).
3. `test_todos_and_agenda_endpoints_mutate_dashboard_state` (`POST /api/todos`, toggle, delete, `POST /api/agenda`, verify reflected in `/api/state`).
4. `test_parse_cli_todo_and_open_subcommands` in `src/main.rs`.

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test`
Expected: FAIL (missing new `AppState` fields, routes, and `Command` variants).

- [ ] **Step 4: Implement new routes, `remote_approval_enabled` gate, `DashboardState`, and CLI commands in `src/server.rs` and `src/main.rs`**

Wire `AntigravityMonitor`, `AgendaStore`, `TodoStore`, `config_path`, and `remote_approval_enabled: Arc<AtomicBool>` into `AppState` and `run_serve()`. Implement `deskpulse open` (reusing `find_adb_path()` / `parse_adb_devices()`) and `deskpulse todo ...`.

- [ ] **Step 5: Run full Rust test suite**

Run: `cargo test`
Expected: PASS (all existing 29 + all new tests pass, 0 warnings).

- [ ] **Step 6: Commit**

```bash
git add assets/ src/server.rs src/main.rs
git commit -m "feat: add PWA assets, non-blocking hook default, agenda/todo APIs, and CLI commands"
```

---

### Task 7: 3A Landscape Frontend (`web/index.html`) + Release Build & Live Verification

**Files:**
- Modify: `web/index.html`

**Interfaces:**
- Consumes: `GET /api/state`, `GET /api/stream`, `POST /api/media/{action}`, `POST /api/todos*`, `POST /api/agenda*`, `POST /api/settings`, `POST /api/system/lock`, `POST /api/approvals/{id}`, `/manifest.webmanifest`, `/sw.js`.

- [ ] **Step 1: Rewrite `web/index.html` implementing the 3A Landscape Hub**

Implement strictly matching the spec:
- Head: `<link rel="manifest" href="/manifest.webmanifest">`, `<link rel="icon" href="/icon.svg">`, `<meta name="mobile-web-app-capable" content="yes">`, Service Worker registration (`/sw.js`).
- Shell: `100vw × 100dvh`, `overflow: hidden`, `#0b0d11` / `#151921` / `#1c222d` palette, 5-button full-width bottom bar (`Visão Geral`, `Agentes IA`, `Agenda & To-Do`, `Monitor PC`, `⛶ Tela Cheia`).
- View 1 (`Visão Geral`): Clock + Date + USB/SSE dot, Agenda Hoje (next 3 with relative countdown), To-Do checkboxes (`done/total`); Media card with 5 large buttons (`⏮ Ant`, `⏸/▶`, `Prox ⏭`, `Vol −`, `Vol +` stepping ±5%); Antigravity Live strip + 2×2 color-coded PC Health grid (`CPU`, `RAM`, `Disco C:`, `Rede`).
- View 2 (`Agentes IA`): Active/recent Antigravity sessions, notification feed, `Aprovação Remota: OFF/ON` toggle, and conditional approval overlay (only when `approvals.length > 0`).
- View 3 (`Agenda & To-Do`): Full today schedule + `Atualizar` + collapsible `.ics` URL input; full interactive To-Do list + quick add + `Limpar feitos`; client-side Focus Timer (`25m` / `5m` / `Reset`).
- View 4 (`Monitor PC`): Large CPU card with 30-point SVG trend line, RAM, all disks, GPU (when present), network, uptime, and quick action buttons (`Mutar / Desmutar`, `Bloquear PC`, `Atualizar Agenda`).
- Preserve token gate (`#token=...` -> `localStorage`), unlocked Web Audio chime on first gesture, and Wake Lock.

- [ ] **Step 2: Run `cargo test` and `cargo build --release`**

Run: `cargo test` and `cargo build --release`
Expected: All tests PASS (0 warnings) and release binary builds cleanly.

- [ ] **Step 3: Restart daemon, reinstall hooks, open on Android, and verify live**

Stop old `deskpulse.exe`, start `deskpulse.exe serve` in background, verify `/manifest.webmanifest`, `/sw.js`, `/icon-192.png`, `/api/state`, add a test todo via `deskpulse.exe todo add`, and open on the connected Galaxy S8+ via `deskpulse.exe open`.

- [ ] **Step 4: Commit**

```bash
git add web/index.html
git commit -m "feat: redesign DeskPulse web UI as 3A zero-scroll landscape PWA hub"
```
