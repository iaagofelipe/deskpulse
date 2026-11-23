# DeskPulse v2 Appliance — Design Specification

**Date:** 2026-11-23  
**Target Device:** Samsung Galaxy S8+ (`SM-G955F`, 18.5:9 AMOLED, landscape on fixed desk stand under main monitor)  
**Reference Inspiration:** [`zacharyinserra/deskhub`](https://github.com/zacharyinserra/deskhub)  
**Platform / Stack:** Single-binary Rust backend (`axum`, `tokio`, `sysinfo`, `reqwest`, `windows` WinRT/WASAPI/Win32) + USB ADB reverse (`127.0.0.1:8765`) + zero-bundler modular HTML/CSS/ES-module PWA embedded directly into the binary.

---

## 1. Intent & Why

DeskPulse v1 proved the core technical foundation (single-binary Rust over USB `adb reverse`, local security middleware, WinRT/WASAPI media, Antigravity live session monitor, remote approval hooks, `.ics`/push calendar, and persisted To-Do). However, its UI still feels like a dense admin PoC: 4 overcrowded tabs, tiny `10–13px` typography, Unicode icons, text-heavy buttons, and a monolithic 3,224-line `web/index.html`.

DeskPulse v2 transforms DeskPulse into a **finished OLED desk appliance** inspired by `zacharyinserra/deskhub`:
- **Glanceable from 60–80 cm:** deep OLED dark surfaces, hero tabular numbers, uniform SVG icons, circular gauges, real charts, and a clean icon navigation rail.
- **1 tab = 1 purpose:** 9 dedicated screens (`Home`, `PC`, `IA`, `Agenda`, `Clima`, `Mídia`, `GitHub`, `Atalhos`, `Timer`), keeping the Home as a curated, non-scrolling Bento grid.
- **100% real data, zero fake metrics:** including real Antigravity token/model/7-day telemetry parsed from local `transcript.jsonl` files, structured behind a provider-agnostic AI contract so Claude Code (or Codex) can be plugged in later without rewriting the UI.
- **PC-side configuration & daily polish:** all settings (Stream Deck tiles, weather city, GitHub token, Home widgets, night mode) live in `%APPDATA%\DeskPulse\config.json` with live hot-reload, paired with a native Windows System Tray icon, optional startup registration, and an OLED Night Mode / PC-lock clock screensaver.

---

## 2. Goals, Non-Goals & Anti-Goals

### Goals
- **9 dedicated landscape views** accessible from a compact left SVG icon rail (`52px` wide) with active pill highlight, live Timer remaining badge, live AI active/approval badge, and bottom USB/PC connection status indicator.
- **Zero-scroll root shell** (`position: fixed; inset: 0; overflow: hidden`) calibrated for 18.5:9 landscape AMOLED phones (e.g., `658×320`–`846×412` CSS px), with internal smooth scroll only where lists naturally exceed one screen (e.g., Agenda upcoming days, AI session inspector).
- **Provider-agnostic AI view (`IA`):** Antigravity-first (real `input_tokens`, `output_tokens`, `cache_read_tokens`, `model`, `created_at` aggregated across 5h, today, last hour, and 7 days, plus live sessions and remote approval hooks), with optional percentage/reset slots ready for future Claude Code / Codex collectors.
- **Agenda focused on Today:** Today's events take hero priority, upcoming 7-day events follow grouped by day, and the existing To-Do list is minimized into a collapsible extra drawer pill (`To-Do (N)`) that opens on tap.
- **Dedicated Media Hero:** large album art, source app badge, title/artist/album, interactive seek bar (`TimelineProperties` + seek command), large circular playback controls, volume `− / % / +`, and mic mute state.
- **Configurable Stream Deck (`Atalhos`):** 4×2 tile grid (up to 16 tiles across 2 pages) configured solely on the PC (`config.json`), supporting live toggle state (`toggle_mic`, `toggle_mute`), volume/media controls, `lock_pc`, Windows hotkeys (`SendInput`), apps, folders, and URLs.
- **Weather (`Clima`):** Open-Meteo (zero API key) showing current conditions + feels like/humidity/wind, 24-hour strip, and 6-day forecast.
- **GitHub (`GitHub`):** 52-week contribution heatmap, current/longest streak & best day, this week vs. last week comparison bars, and top repositories by commits.
- **PC & Internet (`PC`):** segmented pill (`PC` | `Internet`); `PC` shows 4 circular gauges (CPU, GPU + temp, RAM, VRAM) + 60-second dual CPU/GPU line chart + disks; `Internet` shows online status, latency/jitter/packet loss, 60-probe chart, uptime/last drop, daily/monthly data usage, and on-demand speedtest.
- **Timer (`Timer`):** dedicated countdown/Pomodoro view with circular progress ring, quick presets (`5m`, `15m`, `25m`, `45m`), fullscreen "Tempo esgotado!" alert card, and remaining time badge on the icon rail.
- **Daily appliance polish:** PC-side `config.json` hot-reload, OLED Night Mode / PC-lock clock screensaver (tap to wake temporarily), native Windows System Tray (`Shell_NotifyIconW`), and Windows startup toggle (`HKCU\...\Run`).

### Non-Goals & Anti-Goals
- **No 3D Printer or Steam/Games tabs** (explicitly excluded from scope).
- **No mobile configuration forms** for shortcuts, GitHub tokens, or weather city (configured on the PC in `config.json`; only runtime toggles like Remote Approval, Sound, Fullscreen, and Agenda refresh stay tappable on the phone).
- **No network exposure or arbitrary remote command execution:** server remains strictly `127.0.0.1:<port>` with existing `guard_middleware` (`Host`, `Origin`, `Sec-Fetch-Site`, `X-DeskPulse-Token` / `Bearer`). Shortcut endpoints accept only a tile `id` looked up from PC-local config; secrets like `github.token` are never serialized to `/api/state` or `/api/stream`.
- **No Node.js/npm build dependency:** frontend is modular vanilla CSS + ES modules embedded directly in the Rust binary.
- **No fake metrics:** every displayed number comes from real local telemetry or external APIs; when a source is unconfigured (e.g., GitHub PAT missing), show a clean, honest setup hint referencing `config.json`.

---

## 3. Global Constraints

- **Platform:** Windows 10/11 host (WinRT SMTC, WASAPI render + capture endpoints, Win32 `SendInput`, `OpenInputDesktop`, `Shell_NotifyIconW`, Registry Run key). Non-Windows compilation paths remain `#[cfg]`-safe where applicable.
- **Security:** `127.0.0.1:<port>` bind only; `guard_middleware` protects all `/api/*` with constant-time token check; public routes (`/`, `/assets/*`, `/manifest.webmanifest`, `/icon*`, `/sw.js`) remain guarded by `Host` / `Origin` / `Sec-Fetch-Site`.
- **Backward Compatibility:** Existing `%APPDATA%\DeskPulse\config.json` files must load cleanly with `#[serde(default)]` for all new fields; existing CLI commands (`serve`, `open`, `todo`, `hook`, `install-hooks`, `uninstall-hooks`) and existing `/api/*` endpoints remain functional.
- **Copy & Language:** User-facing UI copy in Portuguese (PT-BR), concise and glanceable.
- **Single Binary:** All `web/` HTML, CSS, and JS files are embedded at compile time (`include_str!`) so `deskpulse.exe` remains completely self-contained.

---

## 4. Architecture & Subsystem Design

### 4.1 Configuration & Hot-Reload (`src/config.rs`)
Extend `AppConfig` with `#[serde(default)]` fields:
- `weather: WeatherConfig { city: String, latitude: Option<f64>, longitude: Option<f64>, unit: String }` (default `unit = "c"`, empty city)
- `github: GithubConfig { username: Option<String>, token: Option<String>, token_env: Option<String> }` (resolves `token`, else env var named by `token_env` or `"GITHUB_TOKEN"`)
- `ai: AiConfig { provider: String ("antigravity"), budget_5h_tokens: Option<u64>, budget_weekly_tokens: Option<u64> }`
- `shortcuts: Vec<ShortcutConfig>` (default 8-tile deck: Mute Mic, Mute Som, Vol -, Vol +, Print `Win+Shift+S`, Terminal `wt.exe`, Downloads, Bloquear PC; max 16 tiles)
- `home_widgets: Vec<String>` (default `["ai", "weather", "agenda", "media", "github"]`)
- `night_mode: NightModeConfig { enabled: bool (true), start_hour: u8 (23), end_hour: u8 (7), dim_when_pc_locked: bool (true) }`

Add a lightweight config hot-reloader (`spawn_config_watcher`) polling `config.json` mtime every 3 seconds: on valid change, updates live stores (`weather`, `github`, `shortcuts`, `ai` budgets, `night_mode`, `home_widgets`, `agenda` ICS URL) without restarting the server. Invalid JSON edits log an error and keep the last valid config active.

New CLI subcommands in `src/main.rs`:
- `deskpulse serve [--no-tray]`
- `deskpulse config-path` — prints `%APPDATA%\DeskPulse\config.json`
- `deskpulse startup <install|uninstall|status>` — manages `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\DeskPulse`

### 4.2 Media & Microphone (`src/monitors/media.rs`)
Extend `MediaSnapshot`:
- `album: String`
- `position_secs: Option<u64>`
- `duration_secs: Option<u64>`
- `can_seek: bool`
- `mic_muted: bool`

Extend `MediaCommand`:
- `Seek(u64)` (uses SMTC `TryChangePlaybackPositionAsync` with 100-nanosecond ticks)
- `ToggleMicMute` (uses WASAPI `GetDefaultAudioEndpoint(eCapture, eConsole)` -> `IAudioEndpointVolume::SetMute`, falling back to `eCommunications` if needed)

### 4.3 PC Telemetry & Internet Monitor (`src/monitors/system.rs`, `src/monitors/network.rs`)
- **`system.rs`:** bump `cpu_history` capacity from 30 to **60** samples, add `gpu_history: Vec<u8>` (60 samples), and add `pc_locked: bool` (Windows: `OpenInputDesktop` check for locked/secure desktop).
- **`network.rs`:** `NetworkMonitor` on its own background task:
  - Every 2s performs a bounded async TCP connect timing probe (`1.1.1.1:443`, fallback `8.8.8.8:443`, 1500ms timeout) to measure `latency_ms`, rolling `jitter_ms` (mean absolute diff of adjacent valid samples), `loss_pct`, and `history_ms: Vec<i32>` (last 60 probes, `-1` = timeout/loss).
  - Tracks `online`, `up_since_ms`, `last_drop_ms`.
  - Tracks cumulative `today_bytes` and `month_bytes` (rx + tx delta from `sysinfo::Networks`), persisted atomically every 60s to `%APPDATA%\DeskPulse\net_usage.json` with automatic day/month rollover.
  - Exposes `run_speedtest()` (`POST /api/network/speedtest`), rate-limited to 1 run per 120s, downloading up to 10 MB from `https://speed.cloudflare.com/__down?bytes=10000000` with 15s timeout to report `speed_down_mbps` and `speed_tested_at_ms`.

### 4.4 Provider-Agnostic AI Telemetry (`src/monitors/ai.rs`, `src/monitors/antigravity.rs`)
Create `src/monitors/ai.rs` defining the provider-agnostic `AiSnapshot`:
- `provider: String` (`"antigravity"`, future `"claude"`, `"codex"`)
- `provider_label: String` (`"Antigravity"`)
- `window_5h: AiWindowMetric { label: String, tokens: u64, budget_tokens: Option<u64>, pct: Option<f32>, reset_label: Option<String> }`
- `weekly: AiWindowMetric { label: String, tokens: u64, budget_tokens: Option<u64>, pct: Option<f32>, reset_label: Option<String> }`
- `today: AiTodayStats { total_tokens: u64, input_tokens: u64, output_tokens: u64, cache_read_tokens: u64, messages: u64, last_hour_tokens: u64 }`
- `last_7_days: Vec<AiDayBucket>` (7 days ending today: `date`, `label`, `total_tokens`)
- `models_today: Vec<AiModelUsage>` (`model`, `display_name`, `total_tokens`, sorted descending, top 5)
- `models_7d: Vec<AiModelUsage>` (fallback display when today has 0 usage)
- `sessions: Vec<AgentSessionSummary>` (existing live Antigravity sessions)

In `src/monitors/antigravity.rs`:
- Maintain an incremental file-stat cache (`PathBuf -> (mtime_ms, len, Vec<TranscriptUsageRecord>)`) over `brain/*/\.system_generated/logs/transcript.jsonl` modified within the last 8 days.
- Parse each JSONL line's `created_at`, `type`, `model`, `input_tokens`, `output_tokens`, `cache_read_tokens`.
- Aggregate cleanly into `AiSnapshot` (pure aggregation function `aggregate_usage_records` for deterministic unit tests).
- If user sets `budget_5h_tokens` / `budget_weekly_tokens` in `config.json`, compute `pct`; otherwise `pct` is `None` and the UI displays clean token volume (zero invented limits).

### 4.5 Multi-Day Agenda (`src/agenda.rs`)
- Expand `.ics` parsing and pushed-event retention from "today only" to a **7-day window** (`today ..= today + 6 days`).
- Add optional fields to `AgendaEvent`: `location: Option<String>` (from `LOCATION:`), `day_key: String` (`YYYY-MM-DD`), `is_today: bool`.
- Preserve full backward compatibility for `POST /api/agenda` (computing `day_key` and `is_today` automatically when omitted).

### 4.6 Weather (`src/monitors/weather.rs`)
- Polls Open-Meteo every 15 minutes (or immediately when city/coords change via config hot-reload).
- Resolves `city` to `(lat, lon, resolved_name)` via `geocoding-api.open-meteo.com` if coordinates are not manually provided.
- Exposes `WeatherSnapshot`: `configured`, `city`, `unit`, `temp`, `feels_like`, `humidity_pct`, `wind_speed`, `weather_code`, `condition`, `today_high`, `today_low`, `hourly` (next 24h), `daily` (6 days), `updated_at_ms`, `error`.

### 4.7 GitHub (`src/monitors/github.rs`)
- Polls GitHub GraphQL API (`https://api.github.com/graphql`) every 15 minutes when a token is available (from `config.json` or `GITHUB_TOKEN` env var).
- Queries `viewer` (or `user(login: $login)` if `username` is configured) for `contributionsCollection` (52-week `contributionCalendar`, `totalCommitContributions`, `totalPullRequestContributions`, `totalIssueContributions`, `restrictedContributionsCount`, `commitContributionsByRepository`).
- Pure computation helper produces `GithubSnapshot`: `configured`, `username`, `total_contributions`, `commits`, `private_contributions`, `prs`, `issues`, `current_streak`, `longest_streak`, `best_day_count`, `best_day_label`, `this_week_total`, `last_week_total`, `this_week_days: [u32; 7]`, `last_week_days: [u32; 7]`, `top_repos` (top 4), `weeks` (52×7 intensity levels `0..=4` + month labels), `updated_at_ms`, `error`.

### 4.8 Stream Deck Shortcuts (`src/shortcuts.rs`)
- Holds current `Vec<ShortcutConfig>` behind `RwLock`.
- Projects safe `Vec<ShortcutTileState>` (`id`, `label`, `icon`, `color`, `active`) to the frontend, where `active` is derived live from `MediaSnapshot` (`toggle_mic` -> `mic_muted`, `toggle_mute` -> `muted`).
- Executes validated actions by `id`:
  - Audio/Media actions delegate directly to `MediaBackend`
  - `lock_pc` invokes `rundll32.exe user32.dll,LockWorkStation`
  - `hotkey` parses key combos (e.g., `Win+Shift+S`, `Ctrl+Alt+T`, `F13`) into Win32 `SendInput` `KEYBDINPUT` down/up sequences
  - `app` spawns executable directly with `args` (no `cmd /C` shell interpolation)
  - `url` validates `http://` or `https://` prefix and launches via `explorer.exe`
  - `folder` expands `%USERPROFILE%` / `%APPDATA%` and opens existing directory via `explorer.exe`

### 4.9 Modular Frontend (`web/` + `src/web_assets.rs`)
Replace the monolithic `web/index.html` with clean embedded ES modules served under `/assets/*`:
- `web/index.html` — minimal shell + overlays (Approval, Token, OLED Night/Lock Screensaver) + left SVG icon rail
- `web/css/tokens.css`, `web/css/layout.css`, `web/css/views.css`
- `web/js/icons.js` — crisp SVG icon registry
- `web/js/charts.js` — SVG arc gauges, dual-line chart, 7-day bar chart, dual weekly comparison bars, 52-week GitHub heatmap, Timer ring
- `web/js/api.js` — auth token, `apiRequest`, SSE stream, audio chime, wake lock
- `web/js/views/{home,pc,ai,agenda,weather,media,github,shortcuts,timer}.js`
- `web/js/app.js` — router, state fan-out, OLED Night Mode / PC-lock clock screensaver (tap to temporarily wake for 30s)

### 4.10 Windows System Tray (`src/tray.rs`)
- Spawns a lightweight native Win32 hidden message-window thread using `Shell_NotifyIconW` (reusing the already-present `windows` crate).
- Context menu items: `Abrir no Celular (ADB)`, `Abrir no Navegador`, `Editar Config (config.json)`, `Iniciar com o Windows` (checked state), `Sair` (triggers graceful `shutdown_tx`).
- Disabled cleanly when `--no-tray` is passed or during tests.

---

## 5. Builder Calls & Open Details

- **Frontend without bundler:** Native ES modules (`<script type="module" src="/assets/js/app.js">`) served from `127.0.0.1` over USB have sub-millisecond load latency and zero Node/Vite toolchain overhead, keeping `cargo build --release` self-contained.
- **No ICMP admin requirement:** Internet latency/loss uses TCP connect probes to `1.1.1.1:443` / `8.8.8.8:443` so DeskPulse never requires elevated privileges on Windows.
- **Native Win32 System Tray:** Implemented directly via `windows` crate Win32 Shell/UI APIs rather than pulling extra GUI dependencies.
- **Screensaver burn-in shift:** When OLED Night Mode / PC-lock screensaver is active, the dim clock gently shifts position by a few pixels every minute to protect the Galaxy S8+ AMOLED panel.
