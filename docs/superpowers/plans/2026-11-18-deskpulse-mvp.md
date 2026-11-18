# DeskPulse MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build DeskPulse v1 as a single lightweight Rust binary on Windows (`127.0.0.1:8765` + `adb reverse`) serving an embedded mobile-first web dashboard for read-only PC telemetry, Windows media/volume controls, and Antigravity `PreToolUse`/`Stop` notifications & approvals.

**Architecture:** A single Rust executable (`deskpulse`) provides `serve`, `hook pre-tool`, `hook stop`, `install-hooks`, and `uninstall-hooks` subcommands. The `serve` daemon runs `axum` on `127.0.0.1:8765` with strict loopback/Origin/Sec-Fetch-Site/token security, background monitors (`sysinfo` + optional `nvidia-smi`, WinRT GSMTC + Core Audio via `windows`, and `adb reverse`), an in-memory SSE event bus + 45s approval queue, and serves an embedded vanilla HTML/CSS/JS PWA (`web/index.html`) that streams SSE via `fetch()` with `X-DeskPulse-Token`.

**Tech Stack:** Rust (`x86_64-pc-windows-msvc`), `axum`, `tokio`, `tokio-stream`, `serde`/`serde_json`, `sysinfo`, `windows` (WinRT GSMTC + Core Audio), `reqwest` (no TLS, loopback only), `rand`/`hex`/`subtle`, vanilla HTML/CSS/JS.

**Spec:** `docs/superpowers/specs/2026-11-18-deskpulse-design.md`

## Global Constraints

- Platform: Windows 10/11 x64 (`x86_64-pc-windows-msvc`) only for v1.
- Network: bind strictly to `127.0.0.1:8765` (never `0.0.0.0`); USB access only via `adb reverse tcp:8765 tcp:8765`.
- Zero arbitrary shell execution, zero process killing, zero auto-updater, zero third-party cloud integrations.
- Every `/api/*` endpoint requires `Authorization: Bearer <token>` or `X-DeskPulse-Token` (never query-string tokens); `/` serves static HTML without secrets.
- Hook contracts (`~/.gemini/config/hooks.json`): `"timeout": 50` (seconds); `PreToolUse` outputs `{"decision": "allow" | "deny" | "ask"}` (`"reason"` optional); `Stop` outputs `{}`.
- Fail-open chat fallback: if agent is unreachable, no phone client was active in the last 8s, 45s timeout expires, or the hook HTTP request disconnects, return `{"decision": "ask"}` (or `{}` on `Stop`).

## Review Focus

- Native browser `EventSource` cannot send `Authorization` or `X-DeskPulse-Token` headers, and putting tokens in query strings leaks them: `security.rs` must reject query tokens and `web/index.html` must stream `/api/stream` via `fetch()` + `ReadableStream`.
- Antigravity `PreToolUse` must never hang the IDE when the agent is stopped or unreachable: `deskpulse hook pre-tool` must fail open with `{"decision": "ask"}` and exit `0` within 1s on connection/parse error.
- An abandoned approval request (Antigravity turn canceled by the user while the phone prompt is open) must not leave a stale approval card on the phone: dropping the `/api/hooks/pre-tool` request future must remove the pending item and emit `approval-done`.
- `deskpulse install-hooks` / `uninstall-hooks` must preserve unrelated keys/hooks in `C:\Users\iagofelipe\.gemini\config\hooks.json` and create a `.bak` backup before modifying an existing file.
- Missing `nvidia-smi` (non-NVIDIA GPUs) or no active Windows media session must never crash or stall the 1s PC telemetry snapshot or SSE stream.

---

### Task 1: Toolchain Setup (`rustup` + MSVC Build Tools) and Cargo Scaffold

**Files:**
- Create: `D:\Projetos\deskpulse\Cargo.toml`
- Create: `D:\Projetos\deskpulse\src\main.rs`
- Create: `D:\Projetos\deskpulse\.gitignore`

**Interfaces:**
- Produces: runnable Cargo project at `D:\Projetos\deskpulse` with `cargo test` and `cargo build` passing on `x86_64-pc-windows-msvc`.

- [ ] **Step 1: Verify or install MSVC C++ Build Tools and `rustup`**

Run `winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"` if `cl.exe`/`link.exe` are missing, then `winget install --id Rustlang.Rustup` (or `rustup-init.exe -y --default-toolchain stable-x86_64-pc-windows-msvc`). Verify `rustc --version` and `cargo --version` succeed.

- [ ] **Step 2: Initialize Git repo and `.gitignore`**

Run `git init D:\Projetos\deskpulse` and create `.gitignore` ignoring `/target` and `**/*.rs.bk`.

- [ ] **Step 3: Create `Cargo.toml` with pinned dependencies**

Include: `axum = "0.8"`, `tokio = { version = "1", features = ["full"] }`, `tokio-stream = { version = "0.1", features = ["sync"] }`, `futures-util = "0.3"`, `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"`, `sysinfo = "0.33"`, `reqwest = { version = "0.12", default-features = false, features = ["json"] }`, `rand = "0.8"`, `hex = "0.4"`, `subtle = "2.6"`, `base64 = "0.22"`, `windows = { version = "0.58", features = ["Foundation", "Media_Control", "Storage_Streams", "Win32_Foundation", "Win32_Media_Audio", "Win32_Media_Audio_Endpoints", "Win32_System_Com"] }`, and `[dev-dependencies] tower = { version = "0.5", features = ["util"] }`, `http-body-util = "0.1"`, `tempfile = "3"`.

- [ ] **Step 4: Write failing smoke test and minimal `src/main.rs` CLI parser**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_serve_command() {
        assert_eq!(parse_cli(["deskpulse"]).unwrap(), Command::Serve);
        assert_eq!(parse_cli(["deskpulse", "hook", "pre-tool"]).unwrap(), Command::HookPreTool);
        assert_eq!(parse_cli(["deskpulse", "hook", "stop"]).unwrap(), Command::HookStop);
        assert_eq!(parse_cli(["deskpulse", "install-hooks"]).unwrap(), Command::InstallHooks);
        assert_eq!(parse_cli(["deskpulse", "uninstall-hooks"]).unwrap(), Command::UninstallHooks);
    }
}
```

- [ ] **Step 5: Run `cargo test` to verify scaffold compiles and passes**

Run: `cargo test` in `D:\Projetos\deskpulse`
Expected: PASS (`1 passed; 0 failed`)

- [ ] **Step 6: Commit**

```bash
git add .gitignore Cargo.toml src/main.rs docs/superpowers/specs/2026-11-18-deskpulse-design.md docs/superpowers/plans/2026-11-18-deskpulse-mvp.md
git commit -m "chore: scaffold deskpulse Rust project with spec and plan"
```

---

### Task 2: Configuration Store & Security Middleware

**Files:**
- Create: `D:\Projetos\deskpulse\src\config.rs`
- Create: `D:\Projetos\deskpulse\src\security.rs`
- Modify: `D:\Projetos\deskpulse\src\main.rs`

**Interfaces:**
- Produces in `config.rs`:
  - `pub struct AppConfig { pub port: u16, pub token: String, pub approval_timeout_secs: u64 }`
  - `pub fn default_config_path() -> PathBuf` (`%APPDATA%\DeskPulse\config.json`)
  - `pub fn load_or_create(path: &Path) -> Result<AppConfig, String>` (creates 32-byte hex random token on first run, persists atomically)
  - `pub fn generate_token() -> String`
- Produces in `security.rs`:
  - `pub fn constant_time_eq(a: &str, b: &str) -> bool` (via `subtle::ConstantTimeEq`)
  - `pub fn extract_token(headers: &HeaderMap) -> Option<&str>` (`Authorization: Bearer <t>` or `X-DeskPulse-Token: <t>`)
  - `pub async fn guard_middleware(State(state): State<SecurityState>, req: Request, next: Next) -> Response`
  - `pub struct SecurityState { pub port: u16, pub token: String }`

- [ ] **Step 1: Write failing tests for config generation/persistence and security middleware**

In `config.rs`: test `test_load_or_create_generates_64_hex_token_and_reuses_it` (uses `tempfile::TempDir`).
In `security.rs`: write Tower `oneshot` tests covering:
- `test_allows_root_html_without_token_on_localhost` (`GET /`, `Host: 127.0.0.1:8765` -> `200`)
- `test_blocks_dns_rebinding_host` (`Host: evil.com:8765` -> `403`)
- `test_blocks_cross_site_sec_fetch_site` (`Sec-Fetch-Site: cross-site` -> `403`)
- `test_blocks_foreign_origin` (`Origin: https://evil.com` -> `403`)
- `test_rejects_api_without_token_or_with_query_token` (`GET /api/state?token=...` without header -> `401`)
- `test_accepts_bearer_and_custom_header_token` (`Authorization: Bearer ...` and `X-DeskPulse-Token: ...` -> `200`)

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test config security`
Expected: FAIL (modules not defined yet)

- [ ] **Step 3: Implement `src/config.rs` and `src/security.rs`**

Ensure `guard_middleware` enforces `Host` (`127.0.0.1:{port}` or `localhost:{port}`), rejects non-empty `Origin` outside `http://127.0.0.1:{port}` / `http://localhost:{port}`, rejects `Sec-Fetch-Site` of `"cross-site"` or `"same-site"`, and for any path starting with `/api/` validates `extract_token` against `state.token` with `constant_time_eq`.

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test config security`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/security.rs src/main.rs
git commit -m "feat: add config store with 256-bit token and HTTP security guard"
```

---

### Task 3: Real-Time Event Bus & Approval Queue

**Files:**
- Create: `D:\Projetos\deskpulse\src\events.rs`
- Create: `D:\Projetos\deskpulse\src\approvals.rs`
- Modify: `D:\Projetos\deskpulse\src\main.rs`

**Interfaces:**
- Produces in `events.rs`:
  - `pub struct NotificationEvent { pub id: String, pub kind: String, pub title: String, pub body: String, pub level: String, pub created_at_ms: u64 }`
  - `pub enum StreamEvent { Notify(NotificationEvent), ApprovalPending(ApprovalRequest), ApprovalDone { id: String } }`
  - `pub struct EventBus` with `touch_client()`, `is_client_active(window_ms: u64) -> bool`, `emit(StreamEvent)`, `subscribe() -> broadcast::Receiver<StreamEvent>`, `recent_notifications() -> Vec<NotificationEvent>` (capped at 30)
- Produces in `approvals.rs`:
  - `pub enum Decision { Allow, Deny, Ask }`
  - `pub struct ApprovalRequest { pub id: String, pub tool_name: String, pub title: String, pub summary: String, pub cwd: String, pub created_at_ms: u64, pub expires_at_ms: u64 }`
  - `pub struct ApprovalOutcome { pub decision: Decision, pub reason: Option<String> }`
  - `pub struct ApprovalStore` with `new(events: Arc<EventBus>, timeout: Duration)`, `ask(req: ApprovalInput) -> ApprovalOutcome`, `decide(id: &str, outcome: ApprovalOutcome) -> bool`, `snapshot() -> Vec<ApprovalRequest>`

- [ ] **Step 1: Write failing tests for liveness window, approval decision, timeout fallback, and cancellation drop**

Tests in `approvals.rs`:
- `test_returns_ask_immediately_when_no_client_active`
- `test_resolves_allow_when_client_decides_before_timeout`
- `test_returns_ask_when_timeout_expires_and_emits_approval_done`
- `test_cleans_up_pending_approval_when_caller_future_is_dropped` (simulates Antigravity turn cancellation disconnecting HTTP request; verifies `snapshot()` becomes empty and `ApprovalDone` is emitted!)

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test events approvals`
Expected: FAIL

- [ ] **Step 3: Implement `src/events.rs` and `src/approvals.rs`**

Use an RAII drop guard inside `ApprovalStore::ask` so that whether `ask` finishes via `oneshot`, timeout, or caller future cancellation (`tokio::select!` / Axum request abort), `_pending` removes the ID and emits `StreamEvent::ApprovalDone`.

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test events approvals`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/events.rs src/approvals.rs src/main.rs
git commit -m "feat: add SSE event bus and cancellable approval queue"
```

---

### Task 4: Antigravity Hook Protocol & `hooks.json` Installer

**Files:**
- Create: `D:\Projetos\deskpulse\src\hooks.rs`
- Modify: `D:\Projetos\deskpulse\src\main.rs`

**Interfaces:**
- Consumes: `approvals::ApprovalInput`, `approvals::Decision`, `events::NotificationEvent`
- Produces in `hooks.rs`:
  - `pub struct PreToolStdin { ... }` / `pub struct StopStdin { ... }`
  - `pub fn parse_pre_tool(json: &str) -> Result<PreToolAction, String>` (returns `AskQuestion { summary }` or `Approve(ApprovalInput)` or `Passthrough`)
  - `pub fn format_pre_tool_output(decision: Decision, reason: Option<&str>) -> serde_json::Value` (`{"decision": "allow" | "deny" | "ask", ...}`)
  - `pub fn parse_stop(json: &str) -> NotificationEvent`
  - `pub fn install_hooks_to(path: &Path, exe_path: &Path) -> Result<(), String>` (creates `.bak` if file exists, inserts `"deskpulse"` entry with `PreToolUse` matcher `"run_command|write_file|edit_file|call_mcp_tool|ask_question"` and `"timeout": 50`, plus `Stop` hook with `"timeout": 5`)
  - `pub fn uninstall_hooks_from(path: &Path) -> Result<(), String>`
  - `pub async fn run_hook_cli(kind: HookKind, stdin_text: &str, base_url: &str, token: &str) -> String` (fail-open returning `{"decision":"ask"}` or `{}` on error)

- [ ] **Step 1: Write failing tests for hook payloads, decision output, fail-open, and `hooks.json` merge**

Tests in `hooks.rs`:
- `test_parses_run_command_write_edit_mcp_and_ask_question_payloads`
- `test_formats_pre_tool_decisions_matching_antigravity_contract`
- `test_parses_stop_payload_for_no_tool_call_and_error`
- `test_run_hook_cli_fails_open_when_agent_unreachable` (returns `{"decision":"ask"}` for pre-tool and `{}` for stop within <1s)
- `test_install_and_uninstall_hooks_preserve_existing_hooks_and_create_backup` (uses `tempfile::TempDir`)

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test hooks`
Expected: FAIL

- [ ] **Step 3: Implement `src/hooks.rs`**

Use `reqwest::Client` with `no_proxy()` and connect timeout `500ms` (for `pre-tool`, read timeout `48s`; for `stop`, `2s`) so a dead agent never blocks Antigravity. Use quoted executable path in `hooks.json` (`"\"<exe>\" hook pre-tool"`).

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test hooks`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/hooks.rs src/main.rs
git commit -m "feat: implement Antigravity PreToolUse/Stop hook client and hooks.json installer"
```

---

### Task 5: PC System/GPU Monitor & ADB Reverse Bridge

**Files:**
- Create: `D:\Projetos\deskpulse\src\monitors\mod.rs`
- Create: `D:\Projetos\deskpulse\src\monitors\system.rs`
- Create: `D:\Projetos\deskpulse\src\monitors\adb.rs`
- Modify: `D:\Projetos\deskpulse\src\main.rs`

**Interfaces:**
- Produces in `monitors/system.rs`:
  - `pub struct GpuSnapshot { pub name: String, pub util_pct: Option<f32>, pub temp_c: Option<f32>, pub vram_used_mb: Option<f32>, pub vram_total_mb: Option<f32> }`
  - `pub struct DiskSnapshot { pub name: String, pub used_gb: f64, pub total_gb: f64 }`
  - `pub struct PcSnapshot { pub cpu_pct: f32, pub cpu_name: String, pub ram_used_gb: f64, pub ram_total_gb: f64, pub net_down_bps: u64, pub net_up_bps: u64, pub disks: Vec<DiskSnapshot>, pub gpu: Option<GpuSnapshot>, pub uptime_sec: u64 }`
  - `pub fn parse_nvidia_smi_line(line: &str) -> Option<GpuSnapshot>`
  - `pub struct SystemMonitor` (`spawn(ct)`, `snapshot() -> Option<PcSnapshot>`)
- Produces in `monitors/adb.rs`:
  - `pub struct AdbStatus { pub adb_found: bool, pub adb_path: Option<String>, pub devices: Vec<String>, pub reverse_active: bool, pub last_error: Option<String> }`
  - `pub fn parse_adb_devices(output: &str) -> Vec<String>` (only `\tdevice` lines)
  - `pub fn find_adb_path() -> Option<PathBuf>`
  - `pub struct AdbBridge` (`spawn(port: u16, ct)`, `status() -> AdbStatus`)

- [ ] **Step 1: Write failing tests for `nvidia-smi` parsing, disk/network math, and `adb devices` parsing**

Tests:
- `test_parse_nvidia_smi_line_valid_and_na_fields`
- `test_parse_adb_devices_filters_unauthorized_and_offline`
- `test_system_monitor_produces_initial_snapshot_without_panicking`

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test monitors::system monitors::adb`
Expected: FAIL

- [ ] **Step 3: Implement `src/monitors/mod.rs`, `src/monitors/system.rs`, and `src/monitors/adb.rs`**

Run `nvidia-smi` with `CREATE_NO_WINDOW` (`0x08000000`) and non-blocking fallback to `None` if absent. Run `adb.exe` with `CREATE_NO_WINDOW` every `4s` and execute `adb -s <serial> reverse tcp:8765 tcp:8765` for connected `device` serials.

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test monitors::system monitors::adb`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/monitors/mod.rs src/monitors/system.rs src/monitors/adb.rs src/main.rs
git commit -m "feat: add PC telemetry monitor and USB adb reverse bridge"
```

---

### Task 6: Windows Media (WinRT GSMTC) & Core Audio Volume Controller

**Files:**
- Create: `D:\Projetos\deskpulse\src\monitors\media.rs`
- Modify: `D:\Projetos\deskpulse\src\monitors\mod.rs`

**Interfaces:**
- Produces in `monitors/media.rs`:
  - `pub struct MediaSnapshot { pub playing: bool, pub status: String, pub title: String, pub artist: String, pub app: String, pub artwork_data_url: Option<String>, pub volume_pct: u8, pub muted: bool }`
  - `pub enum MediaCommand { PlayPause, Next, Previous, Mute, Unmute, ToggleMute, SetVolume(u8) }`
  - `#[async_trait::async_trait]` or `Pin<Box<dyn Future...>>` trait `pub trait MediaBackend: Send + Sync`: `snapshot(&self) -> MediaSnapshot`, `execute(&self, cmd: MediaCommand) -> Result<MediaSnapshot, String>`
  - `pub struct WindowsMediaBackend` (uses WinRT `GlobalSystemMediaTransportControlsSessionManager` + Core Audio `IMMDeviceEnumerator` / `IAudioEndpointVolume` inside STA/MTA COM-initialized worker thread so COM apartments never leak onto Tokio worker threads)

- [ ] **Step 1: Write failing tests for `MediaCommand` parsing and volume clamping + mock backend behavior**

Tests:
- `test_media_command_parsing_and_volume_clamping` (`volume=150` -> `SetVolume(100)`, unknown action -> `Err`)
- `test_windows_media_backend_snapshot_does_not_panic` (verifies safe fallback when no media player is active)

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test monitors::media`
Expected: FAIL

- [ ] **Step 3: Implement `src/monitors/media.rs`**

Run WinRT/COM calls on a dedicated worker thread (`CoInitializeEx(COINIT_MULTITHREADED)`), caching thumbnail bytes (`<= 128 KB`, base64 data URL) keyed by `(app, title, artist)` to avoid reading the thumbnail stream every second.

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test monitors::media`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/monitors/media.rs src/monitors/mod.rs
git commit -m "feat: add Windows GSMTC media control and Core Audio volume monitor"
```

---

### Task 7: Axum Server, Embedded Web Dashboard (`web/index.html`) & End-to-End Verification

**Files:**
- Create: `D:\Projetos\deskpulse\src\server.rs`
- Create: `D:\Projetos\deskpulse\web\index.html`
- Modify: `D:\Projetos\deskpulse\src\main.rs`

**Interfaces:**
- Consumes: `Config`, `SecurityState`, `EventBus`, `ApprovalStore`, `hooks`, `SystemMonitor`, `AdbBridge`, `Arc<dyn MediaBackend>`
- Produces in `server.rs`:
  - `pub fn build_router(state: AppState) -> Router`
  - Endpoints:
    - `GET /` -> `web/index.html`
    - `GET /api/state` -> JSON `{ pc, media, adb, notifications, approvals }`
    - `GET /api/stream` -> SSE (`state` every 1s, immediate `notify` / `approval` / `approval-done`)
    - `POST /api/media/{action}` -> `play-pause | next | prev | mute | unmute | toggle-mute | volume`
    - `POST /api/hooks/pre-tool` & `POST /api/hooks/stop`
    - `POST /api/approvals/{id}` (`{"decision":"allow"|"deny"|"ask","reason":...}`)

- [ ] **Step 1: Write failing Axum integration tests in `src/server.rs`**

Tests using `tower::ServiceExt::oneshot` with mock `MediaBackend`:
- `test_api_state_requires_token_and_returns_pc_media_adb_payload`
- `test_media_endpoint_executes_command_and_rejects_unknown_action`
- `test_pre_tool_hook_blocks_until_approval_endpoint_decides_allow`
- `test_pre_tool_hook_returns_ask_immediately_when_no_sse_client_connected`
- `test_root_serves_html_without_leaking_token`

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test server`
Expected: FAIL

- [ ] **Step 3: Implement `src/server.rs`, `web/index.html`, and full `src/main.rs` wiring**

In `web/index.html`:
- Read `#token=...` from `location.hash`, store in `localStorage`, strip hash with `history.replaceState(null, "", "/")`, or show a clean token entry card if missing.
- Stream `/api/stream` using `fetch('/api/stream', { headers: { 'X-DeskPulse-Token': token } })` and parse SSE `event:`/`data:` frames.
- Acquire `navigator.wakeLock.request('screen')` (with visual status indicator & re-acquire on `visibilitychange`).
- Play Web Audio API chime + `navigator.vibrate([120, 60, 120])` when an `approval` or `notify` arrives.
- Render mobile-first dark cards: PC Health (CPU, RAM, GPU, Disks, Net, Uptime), Media Player (cover, title/artist/app, Prev/PlayPause/Next, Mute + Volume slider), Antigravity Approvals (tool badge, command/file/MCP preview, 45s countdown bar, Approve / Deny / Answer on PC), and recent AI notifications feed.
- On `deskpulse serve` startup, print the loopback URL (`http://127.0.0.1:8765/#token=<token>`) clearly once in the console for easy opening/ADB testing.

- [ ] **Step 4: Run full test suite & release build**

Run: `cargo test` and `cargo build --release` in `D:\Projetos\deskpulse`
Expected: All unit + integration tests PASS, and `target\release\deskpulse.exe` builds cleanly.

- [ ] **Step 5: Smoke-test live binary + hooks contract**

Run `deskpulse.exe install-hooks` (or test against temp path) and verify `GET /api/state` and `POST /api/media/toggle-mute` locally with token header, plus `adb reverse` when phone is plugged in.

- [ ] **Step 6: Commit**

```bash
git add src/server.rs src/main.rs web/index.html
git commit -m "feat: add Axum SSE server, embedded web dashboard, and CLI wiring"
```
