# DeskPulse Landscape Hub — Design Specification

**Date:** 2026-11-18  
**Branch:** `feat/deskpulse-mvp`  
**Target Device:** Samsung Galaxy S8+ (`SM-G955F`, 18.5:9 AMOLED, landscape on a fixed desk stand)

---

## 1. Intent & Why

Turn the user's unused Galaxy S8+ into a practical, always-on horizontal desk companion perched under the main monitor on a fragile stand. The current MVP works over USB (`127.0.0.1:8765` + `adb reverse`), but its vertical scrolling layout, tiny text telemetry, browser URL bar, and frequent remote approval popups make it frustrating for daily use.

The redesign shifts DeskPulse from a vertical "approval remote" into a **zero-scroll horizontal desk hub** that is readable from a distance, requires minimal touching, respects Antigravity's native chat approval policies, and surfaces real daily utility: media & volume, readable PC health, live Antigravity agent progress, Google Calendar events, and an AI-managed to-do list.

---

## 2. Goals, Non-Goals & Anti-Goals

### Goals
- **Zero vertical scrolling** on the phone's horizontal screen (`100dvh`, `overflow: hidden` on the root shell).
- **3A Layout & Ergonomics:** Large touch targets (including `Vol −` / `Vol +` buttons instead of fragile slider dragging) and a full-width 5-button bottom navigation bar (`Visão Geral`, `Agentes IA`, `Agenda & To-Do`, `Monitor PC`, `⛶ Tela Cheia`).
- **Fullscreen without browser URL bar:** Serve an installable PWA manifest (`display: "fullscreen"`, `orientation: "landscape"`) plus a one-tap `Tela Cheia` fullscreen/Wake Lock toggle.
- **Legible PC Monitoring:** Color-coded progress bars, clear percentages, real `GB` / `MB/s` units on Overview, and full disk/GPU/CPU history details on `Monitor PC`.
- **Policy-Respecting Antigravity View:** Stop blocking every tool call for phone approval by default (`PreToolUse` returns `{"decision":"ask"}` immediately unless the user opts in). Show live Antigravity conversations, active tools/steps, and `task.md` progress automatically from local files.
- **Google Calendar + Chat-Managed To-Do:** Display today's Google Calendar schedule (via background `.ics` feed polling and/or `POST /api/agenda` push from the `gcalendar` plugin) alongside a persisted To-Do list manageable via CLI (`deskpulse todo ...`), API, and phone tap.

### Non-Goals & Anti-Goals
- **No AI slop:** Clean, grounded dark Material/Nest-Hub aesthetic (`#0b0d11` / `#151921`), high contrast, no decorative neon glows or fake metrics.
- **No network exposure:** Remains strictly `127.0.0.1:8765` over ADB USB reverse with existing `guard_middleware` security (`Host`, `Origin`, `Sec-Fetch-Site`, `X-DeskPulse-Token`).
- **No constant approval spam:** Never interrupt normal IDE chat workflows when remote approval is disabled.

---

## 3. Architecture & Backend Design

### 3.1 Configuration (`src/config.rs`)
Extend `AppConfig` with backward-compatible `#[serde(default)]` fields:
- `calendar_ics_url: Option<String>` (default `None`)
- `remote_approval_enabled: bool` (default `false`)

Add `save_config(path: &Path, cfg: &AppConfig) -> Result<(), String>` using atomic temp-file write + brief Windows lock retry (same pattern as `src/hooks.rs`).

### 3.2 Live Antigravity Monitor (`src/monitors/antigravity.rs`)
Reads `%USERPROFILE%\.gemini\antigravity\` on a lightweight polling interval (`2s`) in a background task/blocking helper (never blocking the Tokio async executor with heavy file scans):
1. Discovers recent conversations by checking `presence/*.lock` and `brain/<id>/.system_generated/logs/transcript.jsonl` modification times (considering conversations modified within the last 24h, capped to the 5 most recent).
2. Reads `annotations/<id>.pbtxt` to extract the conversation `title:"..."`.
3. Reads only the **tail (~16 KB)** of `transcript.jsonl` to inspect the latest JSONL line(s):
   - Extracts `type` (`USER_INPUT`, `PLANNER_RESPONSE`), `status` (`DONE`, `ERROR`, or in-progress), latest `tool_calls` name/target, and timestamp.
   - Infers session state (`"running"` if last step is non-`DONE` or modified within the last `15s` with active lock; `"waiting"` if idle with lock present; `"idle"` otherwise).
4. Reads `brain/<id>/task.md` (if present, capped to reasonable size) to count markdown checkboxes (`- [x]` vs `- [/]` vs `- [ ]`) and identify the active `[/]` item.
5. Exposes `AntigravitySnapshot { sessions: Vec<AgentSessionSummary> }` where each summary has `id`, `title`, `status`, `current_step`, `current_tool`, `task_done`, `task_total`, `updated_at_ms`.

*Builder call:* All filesystem reads fail gracefully (`sessions: []` if directory doesn't exist), and malformed JSONL lines at a tail boundary are skipped cleanly.

### 3.3 Google Agenda Store (`src/agenda.rs`)
Supports two complementary ways to populate today's Google Calendar events with zero friction:
1. **Background `.ics` feed polling (automatic 24/7):** When `calendar_ics_url` is set in `config.json` (or saved from the `Agenda & To-Do` UI), DeskPulse polls the private Google Calendar `.ics` URL every 5 minutes (plus on-demand via `POST /api/agenda/refresh`) using `reqwest`, parses `VEVENT` blocks (`DTSTART`, `DTEND`, `SUMMARY`), converts UTC/`Z` or fixed offsets/dates to local system time, and filters to today's upcoming/active events (tagged with `is_today`).
2. **Push API (`POST /api/agenda`):** Accepts `{ "events": [{ "id"?: string, "title": string, "start_iso": string, "end_iso"?: string, "all_day"?: bool }] }` pushed by Antigravity (using the installed `gcalendar` plugin). Pushed events are persisted atomically to `%APPDATA%\DeskPulse\agenda_push.json` (dropping events from past days automatically) and stored separately from `ics_events`.
3. **Merge & Deduplication:** `AgendaSnapshot` merges `ics_events` and `pushed_events`, deduplicating by normalized `(title, start_iso)` with pushed events taking precedence (so `gcalendar` recurring `RRULE` meetings are preserved across 5-minute `.ics` polls).

`AgendaSnapshot` returns `{ configured: bool, ics_configured: bool, last_sync_ms: Option<u64>, error: Option<String>, events: Vec<AgendaEvent> }` sorted chronologically (`configured` is true when either `.ics` is set or pushed events exist; `ics_configured` lets the UI keep the `.ics` setup drawer accessible).

### 3.4 Persisted To-Do Store & CLI (`src/todos.rs`, `src/main.rs`)
Persisted atomically in `%APPDATA%\DeskPulse\todos.json`:
- `TodoItem { id: String, text: String, done: bool, created_at_ms: u64 }`
- `TodoStore` (`Mutex`-backed in-memory cache synced atomically to disk): `list()`, `add(text)`, `toggle(id)`, `set_done(id, done)`, `remove(id)`, `clear_done()`.
- **CLI subcommands** in `src/main.rs`:
  - `deskpulse todo list`
  - `deskpulse todo add <text...>`
  - `deskpulse todo done <index_or_id>`
  - `deskpulse todo rm <index_or_id>`
  - `deskpulse open` — resolves the local config token and opens `http://127.0.0.1:<port>/#token=<token>` on the first connected ADB device via `adb -s <serial> shell am start -a android.intent.action.VIEW -d <url>`.
  - Note: CLI `todo` mutations hit the running local server (`POST /api/todos...`) if reachable, or modify `todos.json` directly when the server is stopped; the server also reloads `todos.json` if its mtime changes on disk so CLI edits appear live within 1s!

### 3.5 Policy-Respecting Hook Handling (`src/server.rs`)
- `AppState` holds `remote_approval_enabled: Arc<AtomicBool>` (initialized from `AppConfig.remote_approval_enabled`).
- In `pre_tool_hook_handler`:
  - `ask_question`: keeps emitting a non-blocking `ask_question` notification event and returns `{"decision":"ask"}`.
  - `run_command | write_file | edit_file | call_mcp_tool`:
    - Emits a lightweight live activity notification/update if desired (without creating an approval card).
    - **If `remote_approval_enabled` is `false` (default):** returns `{"decision":"ask"}` **immediately** (<5ms), delegating to Antigravity's own chat approval policy without spamming the phone.
    - **If `remote_approval_enabled` is `true`:** routes through `ApprovalStore::ask(input)` as before.
- `stop_hook_handler`: unchanged (emits non-blocking completion/error notification).

### 3.6 System Telemetry History (`src/monitors/system.rs`)
Extend `PcSnapshot` with `cpu_history: Vec<u8>` (last 30 CPU % samples collected by `deskpulse-sysmon`) so the `Monitor PC` tab can render a clean CPU trend bar/sparkline alongside full disk, GPU, RAM, and network metrics.

### 3.7 HTTP API & Security (`src/server.rs`, `src/security.rs`)
- **Public local assets** (protected by Host/Origin/Sec-Fetch-Site in `guard_middleware`, no token required so Chrome Android WebAPK installation works reliably):
  - `GET /` -> `web/index.html`
  - `GET /manifest.webmanifest` -> static JSON (`name`, `short_name`, `start_url: "/"`, `display: "fullscreen"`, `orientation: "landscape"`, `background_color: "#0b0d11"`, `theme_color: "#0b0d11"`, icons `/icon-192.png`, `/icon-512.png`, `/icon.svg`)
  - `GET /icon-192.png` and `GET /icon-512.png` -> static embedded PNG icons (`include_bytes!`, `image/png`) required by Android Chrome for WebAPK installation
  - `GET /icon.svg` -> static SVG icon
  - `GET /sw.js` -> minimal pass-through Service Worker (`application/javascript`, registers fetch listener so Android Chrome meets WebAPK installability criteria without caching authenticated API responses)
- **Authenticated `/api/*` endpoints** (requiring `X-DeskPulse-Token` / `Bearer`):
  - `GET /api/state` & `GET /api/stream` (SSE) — `DashboardState` now includes `antigravity`, `agenda`, `todos`, `remote_approval_enabled`.
  - `POST /api/todos` (`{ text }`), `POST /api/todos/{id}/toggle`, `DELETE /api/todos/{id}`, `POST /api/todos/clear-done`
  - `POST /api/agenda` (`{ events: [...] }`), `POST /api/agenda/refresh`
  - `POST /api/settings` (`{ remote_approval_enabled?: bool, calendar_ics_url?: string }`) — persists to `config.json`.
  - `POST /api/system/lock` — runs `rundll32.exe user32.dll,LockWorkStation` (only when explicitly tapped on `Monitor PC`).

---

## 4. Frontend Design (`web/index.html`)

### 4.1 Shell & Fullscreen PWA
- Root container locked to `100vw × 100dvh`, `overflow: hidden`, dark palette (`#0b0d11` background, `#151921` cards, `#1c222d` sub-cells, `#252b36` crisp 1px borders).
- Includes `<link rel="manifest" href="/manifest.webmanifest">`, `<link rel="icon" href="/icon.svg">`, `<meta name="mobile-web-app-capable" content="yes">`, and registers `/sw.js`.
- Bottom full-width 5-button navigation bar (`44px` height):
  1. `Visão Geral`
  2. `Agentes IA` (with badge count if sessions running)
  3. `Agenda & To-Do`
  4. `Monitor PC`
  5. `⛶ Tela Cheia` (calls `requestFullscreen()` + Wake Lock; updates label when fullscreen).

### 4.2 View 1: `Visão Geral` (3A Layout — Default)
Two-column grid filling 100% of available height above the nav bar, zero vertical scroll:
- **Left Column (~44%):**
  - Top row: Large tabular Clock (`HH:MM`) + Date (`Qua, 18 Nov`) + USB/SSE status dot.
  - Middle: **Agenda Hoje** — next 3 upcoming/current events with start time, title, and relative countdown (`em 25m` / `Agora`), highlighted left border on the next event.
  - Bottom: **To-Do** — top 2–3 open/recent items with directly tappable checkboxes (`done/total` counter).
- **Right Column (~56%):**
  - Top Card: **Mídia & Áudio** — crisp square artwork, title, artist/app + volume %, and **5 large touch buttons** (`⏮ Ant`, `⏸/▶ Pausar`, `Prox ⏭`, `Vol −`, `Vol +`) stepping volume by `±5%` without needing slider dragging.
  - Bottom Card: **Antigravity + PC Agora** —
    - Top strip: Active Antigravity session title, live status badge (`Rodando` / `Idle`), current step/tool, and task progress (`3/5`).
    - 2×2 PC Health Grid: `CPU %`, `RAM used/total GB`, `Disco C: %`, `Rede ↓ / ↑`, each with thick color-coded progress bar (`green <60%`, `amber 60–84%`, `red >=85%`).

### 4.3 View 2: `Agentes IA`
- List of active/recent Antigravity conversations (`title`, `status`, `current_tool`, `current_step`, `task_done/task_total`).
- Recent notification/activity feed (`Stop`, `ask_question`).
- Explicit toggle pill: `Aprovação Remota no Celular: OFF / ON`.
- **Approval Banner Overlay:** Only rendered when `approvals.length > 0` (i.e., when `remote_approval_enabled` is `true` and a tool is waiting), showing large `Permitir`, `Negar`, and `Responder no PC` buttons.

### 4.4 View 3: `Agenda & To-Do`
- Left half: Full today's Google Calendar schedule + `Atualizar` button (plus collapsible `.ics` URL setup box when `configured == false`).
- Right half: Full To-Do list with large tap-to-toggle rows, `Limpar feitos` button, quick add input (optional for touch), and a compact **Focus Timer (`25m Foco` / `5m Pausa` / `Parar`)** that counts down cleanly and optionally chimes via unlocked Web Audio.

### 4.5 View 4: `Monitor PC`
- Readable large metric cards: CPU (`%` + model + 30-sample SVG trend line), RAM (`used / total GB` + `%`), All Disks (`C:\`, `D:\`, etc. with used/total GB bars), GPU (when available), Network (`↓` / `↑`), Uptime.
- Bottom row of large **Quick PC Actions**: `Mutar / Desmutar`, `Bloquear PC`, `Atualizar Agenda`.

---

## 5. Builder Calls & Open Details

- **Client-side Focus Timer:** Implemented purely in browser JS (`web/index.html`) so it adds zero backend state complexity while staying useful on the desk.
- **`.ics` Recurring Events (`RRULE`):** Standard single-occurrence `VEVENT`s and simple `DTSTART` dates are parsed directly in `src/agenda.rs`; complex recurring `RRULE` expansion is out of scope for the minimal parser (the `gcalendar` plugin push via `POST /api/agenda` covers recurring Google Calendar meetings seamlessly).
- **Testing:** Unit tests for `src/monitors/antigravity.rs` (tail parsing, `.pbtxt`, `task.md`), `src/agenda.rs` (ICS parsing + timezone/today filtering + push merge), `src/todos.rs` (CRUD + atomic persistence), `src/config.rs` (backward-compatible deserialization + `save_config`), and `src/server.rs` (non-blocking `PreToolUse` default + opt-in toggle + new endpoints + `/manifest.webmanifest`).
