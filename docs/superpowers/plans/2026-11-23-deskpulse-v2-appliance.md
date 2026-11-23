# DeskPulse v2 Appliance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform DeskPulse into a 9-tab OLED desk appliance inspired by `zacharyinserra/deskhub` (Home Bento, PC/Internet, provider-agnostic AI with real Antigravity usage analytics, Today-focused Agenda + collapsible To-Do, Weather, Media Hero + seek/mic mute, GitHub heatmap/streaks, configurable Stream Deck shortcuts, Timer, OLED Night/Lock screensaver, PC config hot-reload, and Windows System Tray).

**Architecture:** Extend the existing single-binary Rust (`axum` + `tokio` + `windows`) backend with modular background monitors (`network`, `ai`, `weather`, `github`), a PC-config-driven `shortcuts` runner, WinRT timeline/WASAPI mic support, config hot-reload, and a Win32 System Tray. Replace the monolithic `web/index.html` with zero-bundler embedded CSS + ES modules served under `/assets/*` via `src/web_assets.rs`.

**Tech Stack:** Rust 2021, `axum 0.8`, `tokio 1`, `sysinfo 0.33`, `reqwest 0.12`, `chrono 0.4`, `windows 0.58` (WinRT SMTC, WASAPI, Win32 Shell/UI/Keyboard/Registry), vanilla CSS + native ES Modules + inline SVG charts.

**Spec:** [docs/superpowers/specs/2026-11-23-deskpulse-v2-appliance-design.md](file:///d:/Projetos/deskpulse/docs/superpowers/specs/2026-11-23-deskpulse-v2-appliance-design.md)

## Global Constraints

- **Platform:** Windows 10/11 host; non-Windows paths must remain `#[cfg]`-safe.
- **Security:** Bind strictly to `127.0.0.1:<port>`; `guard_middleware` enforces `Host`, `Origin`, `Sec-Fetch-Site` on all routes and constant-time `X-DeskPulse-Token` / `Bearer` on `/api/*`. Never serialize secrets (`github.token`, `token`) or raw PC command paths into `/api/state` or `/api/stream`.
- **Backward Compatibility:** Existing `config.json` files must deserialize via `#[serde(default)]`; existing CLI subcommands (`serve`, `open`, `todo`, `hook`, `install-hooks`, `uninstall-hooks`) and existing `/api/*` endpoints stay compatible.
- **Zero Bundler & Single Binary:** All `web/` files are embedded at compile time via `include_str!` — no Node.js/npm toolchain required.
- **Copy & Real Data:** All UI copy in Portuguese (PT-BR); never fabricate metrics when a data source is unconfigured or offline.

## Review Focus

1. **Corrupted or half-saved `config.json` during live PC editing** — hot-reload must reject invalid JSON without crashing or overwriting the last good live configuration (pinned in Task 1).
2. **GitHub token leakage or missing credentials** — `github.token` must never appear in `DashboardState` JSON and unconfigured credentials must return `configured: false` cleanly without errors (pinned in Task 7 and Task 9).
3. **Malformed/truncated `transcript.jsonl` lines & missing `model`/token fields across 500+ Antigravity brain dirs** — usage aggregator must skip bad lines, cache unchanged files by `(mtime, len)`, and keep totals exact (pinned in Task 4).
4. **Shortcut action injection or invalid hotkey/URL strings from `config.json`** — `shortcuts` must reject non-`http(s)` URLs, unknown hotkey tokens, and unknown tile IDs with `400`/`404` without invoking a shell (pinned in Task 8).
5. **PC locked / OLED screensaver vs. active remote approval alert** — when an urgent remote approval arrives while the OLED Night/Lock screensaver is active, the approval overlay must stay visible above the screensaver (pinned in Task 10).

---

### Task 1: Config v2, Hot-Reload & Windows Auto-Start

**Files:**
- Modify: `src/config.rs`
- Modify: `src/main.rs`
- Modify: `Cargo.toml` (add `Win32_System_Registry` feature to `windows` if needed for startup, or use `reg.exe` / Win32 API cleanly)

**Interfaces:**
- Consumes: existing `AppConfig`, `load_or_create`, `save_config`, `deskpulse_data_dir` in `src/config.rs`.
- Produces:
  - `WeatherConfig { city: String, latitude: Option<f64>, longitude: Option<f64>, unit: String }`
  - `GithubConfig { username: Option<String>, token: Option<String>, token_env: Option<String> }` + `GithubConfig::resolve_token(&self) -> Option<String>`
  - `AiConfig { provider: String, budget_5h_tokens: Option<u64>, budget_weekly_tokens: Option<u64> }`
  - `ShortcutAction` enum (`ToggleMic`, `ToggleMute`, `VolUp`, `VolDown`, `MediaPlayPause`, `MediaNext`, `MediaPrev`, `LockPc`, `Hotkey { keys: String }`, `App { program: String, args: Vec<String> }`, `Url { url: String }`, `Folder { path: String }`)
  - `ShortcutConfig { id: String, label: String, icon: String, color: String, action: ShortcutAction }` + `default_shortcuts() -> Vec<ShortcutConfig>`
  - `NightModeConfig { enabled: bool, start_hour: u8, end_hour: u8, dim_when_pc_locked: bool }`
  - `AppConfig` extended with `weather`, `github`, `ai`, `shortcuts`, `home_widgets: Vec<String>`, `night_mode`
  - `ConfigWatcher::spawn(path: PathBuf, initial: AppConfig, shutdown: watch::Receiver<bool>) -> (Arc<RwLock<AppConfig>>, watch::Receiver<AppConfig>)`
  - CLI `Command::Serve { no_tray: bool }`, `Command::ConfigPath`, `Command::Startup(StartupCommand)` (`Install | Uninstall | Status`)

- [ ] **Step 1: Write failing tests in `src/config.rs` and `src/main.rs`**

Add tests in `src/config.rs`:
- `test_v2_defaults_populate_8_shortcuts_and_home_widgets_on_legacy_json`: deserializes legacy `{"port":8765,"token":"abc","approval_timeout_secs":45}`, asserts `cfg.shortcuts.len() == 8`, `cfg.home_widgets == vec!["ai","weather","agenda","media","github"]`, `cfg.weather.unit == "c"`, `cfg.ai.provider == "antigravity"`, `cfg.night_mode.enabled == true`, `cfg.night_mode.dim_when_pc_locked == true`.
- `test_github_resolve_token_prefers_explicit_then_env`: checks explicit `token` overrides env and trims blanks to `None`.
- `test_config_watcher_reloads_valid_changes_and_ignores_invalid_json`: writes valid config update to temp file, verifies watcher receiver updates; then writes `{broken json`, verifies last valid config is preserved without panic (pins Review Focus #1).

Add test in `src/main.rs`:
- `test_parse_cli_v2_subcommands`: asserts `serve --no-tray`, `config-path`, `startup install`, `startup uninstall`, `startup status` parse cleanly and invalid flags return `Err`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test config::tests` and `cargo test test_parse_cli_v2_subcommands`
Expected: FAIL with missing types/fields/variants.

- [ ] **Step 3: Implement v2 config structs, `ConfigWatcher`, and CLI startup/config-path commands**

- Add `WeatherConfig`, `GithubConfig`, `AiConfig`, `ShortcutAction`, `ShortcutConfig`, `NightModeConfig`, `default_shortcuts()`, `default_home_widgets()`, and `ConfigWatcher::spawn` (3s mtime poll using `watch::channel`) in `src/config.rs`.
- Add `Command::Serve { no_tray: bool }`, `Command::ConfigPath`, `Command::Startup(StartupCommand)` in `src/main.rs`, implementing Windows `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\DeskPulse` via `reg.exe` (`CREATE_NO_WINDOW`) pointing to `<current_exe> serve`.
- Update `src/server.rs` `update_settings_handler` so saving settings preserves all v2 config fields rather than constructing a partial `AppConfig`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: All existing + new config/CLI tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/main.rs src/server.rs
git commit -m "feat(config): add v2 appliance schema, hot-reload watcher, and startup CLI"
```

---

### Task 2: Enhanced Media & Microphone Control (Timeline, Seek & WASAPI Mic Mute)

**Files:**
- Modify: `src/monitors/media.rs`

**Interfaces:**
- Consumes: existing `MediaSnapshot`, `MediaCommand`, `parse_media_command`, `WindowsMediaBackend` in `src/monitors/media.rs`.
- Produces:
  - `MediaSnapshot` extended with `album: String`, `position_secs: Option<u64>`, `duration_secs: Option<u64>`, `can_seek: bool`, `mic_muted: bool`
  - `MediaCommand` extended with `Seek(u64)`, `ToggleMicMute`
  - `parse_media_command(action: &str, value: Option<i32>) -> Result<MediaCommand, String>` handling `"seek"` (clamped `>= 0`) and `"toggle-mic"` / `"toggle-mic-mute"`
  - `ticks_to_secs(ticks: i64) -> Option<u64>` and `secs_to_ticks(secs: u64) -> i64` (100-ns WinRT `TimeSpan` conversions)

- [ ] **Step 1: Write failing tests in `src/monitors/media.rs`**

Add unit tests:
- `test_parse_seek_and_mic_commands`: asserts `parse_media_command("seek", Some(75)) == Ok(MediaCommand::Seek(75))`, `parse_media_command("seek", None).is_err()`, `parse_media_command("seek", Some(-5)) == Ok(MediaCommand::Seek(0))`, `parse_media_command("toggle-mic", None) == Ok(MediaCommand::ToggleMicMute)`.
- `test_winrt_timespan_tick_conversions`: asserts `ticks_to_secs(750_000_000) == Some(75)`, `ticks_to_secs(-1) == None`, `secs_to_ticks(120) == 1_200_000_000`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test monitors::media::tests`
Expected: FAIL with missing variants/functions.

- [ ] **Step 3: Implement timeline/album reading, SMTC seek, and WASAPI capture mic mute in `src/monitors/media.rs`**

- Read `media_props.AlbumTitle()` into `MediaSnapshot.album`.
- Read `session.GetTimelineProperties()` for `Position()` and `EndTime()`, populating `position_secs`, `duration_secs`, and `can_seek = duration_secs.is_some_and(|d| d > 0)`.
- Implement `MediaCommand::Seek(secs)` via `session.TryChangePlaybackPositionAsync(secs_to_ticks(secs))`.
- Implement `capture_endpoint_volume()` (`GetDefaultAudioEndpoint(eCapture, eConsole)` with fallback `eCommunications`) to populate `mic_muted` in `read_audio_state()` and execute `MediaCommand::ToggleMicMute`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test monitors::media::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/monitors/media.rs
git commit -m "feat(media): add timeline seek, album metadata, and WASAPI mic mute"
```

---

### Task 3: PC Telemetry Upgrade (60s Dual History + Lock Detection) & Internet Monitor

**Files:**
- Modify: `Cargo.toml` (add `Win32_System_StationsAndDesktops` feature to `windows`)
- Modify: `src/monitors/system.rs`
- Create: `src/monitors/network.rs`
- Modify: `src/monitors/mod.rs`

**Interfaces:**
- Consumes: `PcSnapshot`, `SystemMonitor` in `src/monitors/system.rs`; `deskpulse_data_dir()` in `src/config.rs`.
- Produces:
  - `PcSnapshot` extended with `gpu_history: Vec<u8>` (cap 60), `cpu_history` bumped to cap 60, `pc_locked: bool`
  - `NetworkSnapshot { online: bool, latency_ms: Option<u32>, jitter_ms: Option<u32>, loss_pct: f32, history_ms: Vec<i32>, up_since_ms: Option<u64>, last_drop_ms: Option<u64>, today_bytes: u64, month_bytes: u64, speed_down_mbps: Option<f32>, speed_tested_at_ms: Option<u64>, testing: bool }`
  - `NetworkMonitor::spawn(usage_path: PathBuf, shutdown: watch::Receiver<bool>) -> NetworkMonitor`
  - `NetworkMonitor::snapshot(&self) -> NetworkSnapshot`
  - `NetworkMonitor::trigger_speedtest(&self) -> Result<NetworkSnapshot, String>` (rate-limited 120s)
  - Pure helpers: `compute_jitter_ms(history: &[i32]) -> Option<u32>`, `compute_loss_pct(history: &[i32]) -> f32`, `rollover_usage(state: &mut NetUsageFile, today: NaiveDate)`

- [ ] **Step 1: Write failing tests in `src/monitors/system.rs` and `src/monitors/network.rs`**

In `src/monitors/system.rs`:
- Update/add `test_push_history_caps_at_60_and_clamps_values` verifying 60-sample cap for both `cpu_history` and `gpu_history`.

In `src/monitors/network.rs`:
- `test_jitter_and_loss_calculation`: given `[-1, 20, 30, 26, -1]`, asserts `loss_pct == 40.0` and `jitter_ms == Some(7)` (`(|30-20| + |26-30|) / 2`).
- `test_net_usage_rollover_resets_day_and_month`: verifies crossing midnight resets `today_bytes` while keeping `month_bytes`, and crossing month resets both, plus atomic save/load roundtrip.
- `test_speedtest_rate_limit_rejects_within_120s`: asserts a second speedtest attempt within cooldown returns `Err` containing `"aguarde"`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test monitors::system::tests` and `cargo test monitors::network::tests`
Expected: FAIL (missing `gpu_history`, `pc_locked`, `monitors::network`).

- [ ] **Step 3: Implement `system.rs` upgrades and `src/monitors/network.rs`**

- Add `Win32_System_StationsAndDesktops` to `Cargo.toml`.
- In `src/monitors/system.rs`, generalize `push_history(history: &mut Vec<u8>, pct: f32)` with cap 60 for CPU and GPU, and add `is_workstation_locked() -> bool` (`OpenInputDesktop(0, false, DESKTOP_SWITCHDESKTOP)` failing -> locked, `CloseDesktop` on handle).
- Create `src/monitors/network.rs`: TCP connect probe every 2s (`1.1.1.1:443`, fallback `8.8.8.8:443`, 1500ms timeout), byte counters from `sysinfo::Networks` with atomic persistence to `net_usage.json`, and bounded 10 MB Cloudflare speedtest (`15s` timeout, `120s` cooldown).
- Export `pub mod network;` in `src/monitors/mod.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test monitors::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/monitors/mod.rs src/monitors/system.rs src/monitors/network.rs
git commit -m "feat(monitors): add 60s CPU/GPU history, PC lock detection, and Internet monitor"
```

---

### Task 4: Provider-Agnostic AI Telemetry & Real Antigravity Usage Aggregator

**Files:**
- Create: `src/monitors/ai.rs`
- Modify: `src/monitors/antigravity.rs`
- Modify: `src/monitors/mod.rs`

**Interfaces:**
- Consumes: `AgentSessionSummary`, `AntigravitySnapshot`, `AntigravityMonitor` in `src/monitors/antigravity.rs`; `AiConfig` in `src/config.rs`.
- Produces:
  - `AiWindowMetric { label: String, tokens: u64, budget_tokens: Option<u64>, pct: Option<f32>, reset_label: Option<String> }`
  - `AiTodayStats { total_tokens: u64, input_tokens: u64, output_tokens: u64, cache_read_tokens: u64, messages: u64, last_hour_tokens: u64 }`
  - `AiDayBucket { date: String, label: String, total_tokens: u64 }`
  - `AiModelUsage { model: String, display_name: String, total_tokens: u64 }`
  - `AiSnapshot { provider: String, provider_label: String, window_5h: AiWindowMetric, weekly: AiWindowMetric, today: AiTodayStats, last_7_days: Vec<AiDayBucket>, models_today: Vec<AiModelUsage>, models_7d: Vec<AiModelUsage>, sessions: Vec<AgentSessionSummary> }`
  - `UsageRecord { created_at_ms: u64, model: String, input_tokens: u64, output_tokens: u64, cache_read_tokens: u64, is_message: bool }`
  - `aggregate_usage(provider: &str, provider_label: &str, records: &[UsageRecord], sessions: Vec<AgentSessionSummary>, now_ms: u64, budget_5h: Option<u64>, budget_weekly: Option<u64>) -> AiSnapshot`
  - `humanize_model_name(raw: &str) -> String`
  - `AntigravityMonitor::ai_snapshot(&self) -> AiSnapshot` and `AntigravityMonitor::update_budgets(&self, budget_5h: Option<u64>, budget_weekly: Option<u64>)`

- [ ] **Step 1: Write failing tests in `src/monitors/ai.rs` and `src/monitors/antigravity.rs`**

In `src/monitors/ai.rs`:
- `test_aggregate_usage_computes_today_last_hour_5h_weekly_7days_and_models`: feeds synthetic `UsageRecord`s spanning 8 days (1 outside 7d window, several inside 5h/1h/today), asserts exact sums for `today`, `window_5h.tokens`, `weekly.tokens`, `last_7_days.len() == 7`, model sorting, and `pct` = `None` without budgets vs. exact `%` with budgets.
- `test_humanize_model_name_formats_antigravity_and_claude_models`: asserts `"gemini-4-neon-high"` -> `"Neon High"`, `"gemini-3.8-flash-medium"` -> `"Flash"`, `"claude-opus-4-6"` -> `"Opus"`.

In `src/monitors/antigravity.rs`:
- `test_parse_usage_records_skips_malformed_lines_and_caches_by_mtime`: writes valid + malformed JSONL lines (including entries missing `model` or token fields), asserts exact parsed records and cache hit on unchanged file (pins Review Focus #3).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test monitors::ai::tests` and `cargo test test_parse_usage_records`
Expected: FAIL.

- [ ] **Step 3: Implement `src/monitors/ai.rs` and incremental usage scanner in `src/monitors/antigravity.rs`**

- Create `src/monitors/ai.rs` with `AiSnapshot` types, `humanize_model_name`, and `aggregate_usage` (using local dates for `last_7_days` buckets with PT-BR weekday labels `"Seg"`, `"Ter"`, ..., `"Hoje"`).
- In `src/monitors/antigravity.rs`, maintain `HashMap<PathBuf, (u64, u64, Vec<UsageRecord>)>` inside the background thread scanning `brain/*/.system_generated/logs/transcript.jsonl` modified within the last 8 days (refreshing usage aggregation every 10s while keeping live session tail polling at 2s).
- Expose `AntigravityMonitor::ai_snapshot()` and `update_budgets()`.
- Export `pub mod ai;` in `src/monitors/mod.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test monitors::ai` and `cargo test monitors::antigravity`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/monitors/mod.rs src/monitors/ai.rs src/monitors/antigravity.rs
git commit -m "feat(ai): add provider-agnostic AI snapshot and real Antigravity usage aggregator"
```

---

### Task 5: Multi-Day Agenda Upgrade (Today Hero + 7 Days + Location)

**Files:**
- Modify: `src/agenda.rs`

**Interfaces:**
- Consumes: existing `AgendaEvent`, `AgendaSnapshot`, `AgendaStore` in `src/agenda.rs`.
- Produces:
  - `AgendaEvent` extended with `#[serde(default)] pub location: Option<String>`, `#[serde(default)] pub day_key: String`, `#[serde(default)] pub is_today: bool`
  - `AgendaStore` retains and merges events across `today ..= today + 6 days`, tagging `is_today` and `day_key` (`YYYY-MM-DD`) and extracting ICS `LOCATION:` property.

- [ ] **Step 1: Write failing tests in `src/agenda.rs`**

Add unit tests:
- `test_parse_ics_includes_next_7_days_location_and_excludes_day_8`: parses ICS with events yesterday, today (with `LOCATION:Google Meet`), +3 days, and +8 days; asserts only today (`is_today == true`, `location == Some("Google Meet")`) and +3 days (`is_today == false`) are returned in chronological order.
- `test_push_events_auto_populates_day_key_and_is_today`: pushes `AgendaEvent` JSON without `day_key`/`is_today`, asserts `snapshot()` populates them accurately and keeps future events up to 7 days.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test agenda::tests`
Expected: FAIL (missing `location`/`day_key`/`is_today` and today-only filtering).

- [ ] **Step 3: Implement 7-day window, `LOCATION` parsing, and `day_key`/`is_today` enrichment in `src/agenda.rs`**

- Parse `LOCATION:` inside `VEVENT` (unescaping `\,`, `\;`, `\n`).
- Filter ICS and pushed events to `event_date >= today && event_date <= today + 6 days` (keeping all-day/multi-day events overlapping that window).
- Enrich every event in `merge_events` with `day_key` (`YYYY-MM-DD`) and `is_today = (day_key == today_key)`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test agenda::tests`
Expected: PASS (all existing + new agenda tests).

- [ ] **Step 5: Commit**

```bash
git add src/agenda.rs
git commit -m "feat(agenda): expand calendar window to 7 days with Today tag and location"
```

---

### Task 6: Weather Monitor via Open-Meteo

**Files:**
- Create: `src/monitors/weather.rs`
- Modify: `src/monitors/mod.rs`

**Interfaces:**
- Consumes: `WeatherConfig` from `src/config.rs`.
- Produces:
  - `WeatherHour { time_label: String, temp: f32, weather_code: u16 }`
  - `WeatherDay { day_label: String, high: f32, low: f32, precip_pct: u8, weather_code: u16 }`
  - `WeatherSnapshot { configured: bool, city: String, unit: String, temp: Option<f32>, feels_like: Option<f32>, humidity_pct: Option<u8>, wind_speed: Option<f32>, weather_code: Option<u16>, condition: String, today_high: Option<f32>, today_low: Option<f32>, hourly: Vec<WeatherHour>, daily: Vec<WeatherDay>, updated_at_ms: Option<u64>, error: Option<String> }`
  - `WeatherMonitor::spawn(initial: WeatherConfig, shutdown: watch::Receiver<bool>) -> WeatherMonitor`
  - `WeatherMonitor::snapshot(&self) -> WeatherSnapshot`
  - `WeatherMonitor::update_config(&self, cfg: WeatherConfig)`
  - Pure helpers: `wmo_condition_pt(code: u16) -> &'static str`, `parse_open_meteo_response(city: &str, unit: &str, json: &serde_json::Value, now_local_iso: &str) -> Result<WeatherSnapshot, String>`

- [ ] **Step 1: Write failing tests in `src/monitors/weather.rs`**

Add unit tests:
- `test_parse_open_meteo_response_extracts_current_next_24h_and_6_days`: parses realistic Open-Meteo JSON fixture, asserts `temp`, `feels_like`, `humidity_pct`, `wind_speed`, PT-BR `condition`, `hourly.len() == 24` starting at current hour, and `daily.len() == 6` with `"Hoje"` first.
- `test_unconfigured_weather_returns_clean_empty_snapshot`: empty city + no coords returns `configured: false, error: None`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test monitors::weather::tests`
Expected: FAIL.

- [ ] **Step 3: Implement `src/monitors/weather.rs` and export in `src/monitors/mod.rs`**

- Poll Open-Meteo every 15 minutes (or immediately via `Notify` when `update_config` changes city/coords/unit).
- Geocode `city` via `https://geocoding-api.open-meteo.com/v1/search?name=...&count=1&language=pt` when `latitude`/`longitude` are `None` and cache resolved coordinates in memory.
- Map WMO codes (`0..=99`) to clean PT-BR labels (`"Céu limpo"`, `"Parcialmente nublado"`, `"Nublado"`, `"Chuva"`, `"Tempestade"`, etc.).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test monitors::weather::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/monitors/mod.rs src/monitors/weather.rs
git commit -m "feat(weather): add Open-Meteo weather monitor with 24h and 6-day forecast"
```

---

### Task 7: GitHub Contributions, Streaks & Top Repos Monitor

**Files:**
- Create: `src/monitors/github.rs`
- Modify: `src/monitors/mod.rs`

**Interfaces:**
- Consumes: `GithubConfig` from `src/config.rs`.
- Produces:
  - `GithubDayCell { date: String, count: u32, level: u8 }`
  - `GithubRepoStat { name: String, commits: u32 }`
  - `GithubSnapshot { configured: bool, username: String, total_contributions: u32, commits: u32, private_contributions: u32, prs: u32, issues: u32, current_streak: u32, longest_streak: u32, best_day_count: u32, best_day_label: String, this_week_total: u32, last_week_total: u32, this_week_days: [u32; 7], last_week_days: [u32; 7], top_repos: Vec<GithubRepoStat>, weeks: Vec<Vec<GithubDayCell>>, updated_at_ms: Option<u64>, error: Option<String> }`
  - `GithubMonitor::spawn(initial: GithubConfig, shutdown: watch::Receiver<bool>) -> GithubMonitor`
  - `GithubMonitor::snapshot(&self) -> GithubSnapshot`
  - `GithubMonitor::update_config(&self, cfg: GithubConfig)`
  - Pure helper: `parse_github_graphql_response(json: &serde_json::Value) -> Result<GithubSnapshot, String>`

- [ ] **Step 1: Write failing tests in `src/monitors/github.rs`**

Add unit tests:
- `test_parse_github_graphql_computes_streaks_best_day_weekly_delta_and_top_repos`: feeds a GraphQL fixture with multiple weeks (including a zero today that preserves yesterday's active streak, a longer historical streak, best day, and 5 repos), asserts exact `current_streak`, `longest_streak`, `best_day_count`, `this_week_total`, `last_week_total`, and top 4 repos sorted descending.
- `test_github_snapshot_never_contains_token_and_handles_unconfigured`: verifies missing token returns `configured: false, error: None` and serialized `GithubSnapshot` JSON contains zero secret fields (pins Review Focus #2).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test monitors::github::tests`
Expected: FAIL.

- [ ] **Step 3: Implement `src/monitors/github.rs` and export in `src/monitors/mod.rs`**

- Query GitHub GraphQL (`viewer` when `username` is empty, `user(login: $login)` when set) every 15 minutes (or on config update).
- Map `contributionLevel` (`NONE`..`FOURTH_QUARTILE`) to `0..=4`.
- Compute `current_streak` (allowing today to be 0 if yesterday > 0), `longest_streak`, `best_day`, `this_week_days` vs. `last_week_days` (Sunday..Saturday).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test monitors::github::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/monitors/mod.rs src/monitors/github.rs
git commit -m "feat(github): add GitHub GraphQL contributions, streaks, and top repos monitor"
```

---

### Task 8: Stream Deck Shortcuts Engine & Windows Hotkey/Action Runner

**Files:**
- Modify: `Cargo.toml` (add `Win32_UI_Input_KeyboardAndMouse` feature to `windows`)
- Create: `src/shortcuts.rs`
- Modify: `src/main.rs` (`mod shortcuts;`)

**Interfaces:**
- Consumes: `ShortcutConfig`, `ShortcutAction` from `src/config.rs`; `MediaBackend`, `MediaCommand`, `MediaSnapshot` from `src/monitors/media.rs`.
- Produces:
  - `ShortcutTileState { id: String, label: String, icon: String, color: String, active: bool }`
  - `ShortcutRunner::new(initial: Vec<ShortcutConfig>) -> ShortcutRunner`
  - `ShortcutRunner::update_config(&self, tiles: Vec<ShortcutConfig>)`
  - `ShortcutRunner::tile_states(&self, media: &MediaSnapshot) -> Vec<ShortcutTileState>` (never exposes raw commands/paths)
  - `ShortcutRunner::trigger(&self, id: &str, media: &Arc<dyn MediaBackend>) -> Result<ShortcutTriggerResult, ShortcutError>`
  - Pure validators: `parse_hotkey_vks(combo: &str) -> Result<Vec<u16>, String>`, `validate_url(url: &str) -> Result<(), String>`, `expand_env_vars(path: &str) -> String`

- [ ] **Step 1: Write failing tests in `src/shortcuts.rs`**

Add unit tests:
- `test_parse_hotkey_vks_valid_and_rejects_unknown_keys`: asserts `"Win+Shift+S"` -> `[VK_LWIN, VK_SHIFT, 'S']`, `"Ctrl+Alt+F13"` succeeds, and `""` / `"Ctrl+BogusKey"` returns `Err` (pins Review Focus #4).
- `test_validate_url_allows_http_https_and_rejects_file_or_cmd`: asserts `https://github.com` passes while `file:///C:/Windows/cmd.exe` and `javascript:alert(1)` return `Err` (pins Review Focus #4).
- `test_tile_states_reflect_live_mic_and_sound_mute_without_leaking_actions`: asserts `toggle_mic` tile sets `active: true` when `media.mic_muted == true`, `toggle_mute` sets `active: true` when `media.muted == true`, and serialized `ShortcutTileState` contains no `program`/`path`/`keys`.
- `test_trigger_unknown_id_returns_not_found`: asserts triggering `"nonexistent"` returns `ShortcutError::NotFound`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test shortcuts::tests`
Expected: FAIL.

- [ ] **Step 3: Implement `src/shortcuts.rs`**

- Add `Win32_UI_Input_KeyboardAndMouse` to `Cargo.toml`.
- Implement `parse_hotkey_vks` (modifiers `Win`/`Ctrl`/`Alt`/`Shift` + `A-Z`, `0-9`, `F1-F24`, `Tab`, `Enter`, `Esc`, `Space`, `PrintScreen`, arrow keys) and Windows `SendInput` press-all/release-reverse sequence.
- Implement `trigger` delegating audio/media actions to `MediaBackend`, `LockPc` to `rundll32.exe`, `App` to `Command::new(program).args(args)` directly (no shell), `Url` (validated `http://`/`https://`) and `Folder` (env-expanded existing path) via `explorer.exe`.
- Cap tile list at 16 entries.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test shortcuts::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/shortcuts.rs src/main.rs
git commit -m "feat(shortcuts): add Stream Deck shortcut engine with Win32 hotkeys and live toggle states"
```

---

### Task 9: API/SSE Integration, Modular Static Asset Server & Config Hot-Reload Wiring

**Files:**
- Create: `src/web_assets.rs`
- Modify: `src/server.rs`
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `NetworkMonitor`, `AiSnapshot`, `WeatherMonitor`, `GithubMonitor`, `ShortcutRunner`, `ConfigWatcher`, `NightModeConfig`.
- Produces:
  - `src/web_assets.rs`: `serve_web_asset(path: &str) -> Option<(&'static str, &'static [u8])>` serving `/assets/css/*`, `/assets/js/*`, `/assets/js/views/*` cleanly from `include_str!` (initially stubbed placeholders for new web files until Tasks 10–11 populate them, or wired cleanly so `cargo test` passes!).
  - `DashboardState` extended with:
    - `network: NetworkSnapshot`
    - `ai: AiSnapshot`
    - `weather: WeatherSnapshot`
    - `github: GithubSnapshot`
    - `shortcuts: Vec<ShortcutTileState>`
    - `home_widgets: Vec<String>`
    - `night_mode: NightModeConfig`
  - New authenticated API routes in `src/server.rs`:
    - `POST /api/shortcuts/{id}/trigger`
    - `POST /api/network/speedtest`
    - `GET /assets/{*path}` (public local route under `guard_middleware`, no token required, matching `/` and `/manifest.webmanifest`)
  - Extended `MediaActionBody` accepting `level: Option<i32>` or `value: Option<i32>` so `/api/media/seek` and `/api/media/toggle-mic` work seamlessly.

- [ ] **Step 1: Create minimal valid placeholder files for modular `web/` assets so `include_str!` compiles cleanly**

Create stub files in `web/css/{tokens,layout,views}.css` and `web/js/{icons,charts,api,app}.js` and `web/js/views/{home,pc,ai,agenda,weather,media,github,shortcuts,timer}.js` (to be fully implemented in Tasks 10 & 11).

- [ ] **Step 2: Write failing integration tests in `src/server.rs`**

Add tests in `src/server.rs`:
- `test_v2_api_state_includes_all_appliance_sections_and_never_leaks_github_token`: asserts `/api/state` returns `network`, `ai`, `weather`, `github`, `shortcuts`, `home_widgets`, `night_mode`, and asserts response body does NOT contain the configured secret GitHub token (pins Review Focus #2).
- `test_shortcut_trigger_and_speedtest_endpoints_require_token_and_handle_unknown_id`: asserts `POST /api/shortcuts/unknown/trigger` returns `404`, `POST /api/shortcuts/mute_mic/trigger` executes `ToggleMicMute` and returns `200`, and unauthenticated calls return `401`.
- `test_modular_web_assets_served_with_correct_mime_and_guard`: asserts `GET /assets/js/app.js` returns `200` with `application/javascript` on `127.0.0.1` and `403` on foreign `Host`.

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test server::tests`
Expected: FAIL.

- [ ] **Step 4: Implement `src/web_assets.rs`, wire `AppState`/`DashboardState`/routes in `src/server.rs`, and wire monitors + `ConfigWatcher` in `src/main.rs`**

- Create `src/web_assets.rs` mapping relative asset paths to `(content_type, bytes)`.
- Add `network`, `weather`, `github`, `shortcuts`, `config_live: Arc<RwLock<AppConfig>>` to `AppState`.
- Wire config hot-reload task in `run_serve` propagating `ConfigWatcher` updates to `weather`, `github`, `shortcuts`, `antigravity` budgets, `agenda` ICS URL, and `remote_approval_enabled`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test`
Expected: All unit + integration tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/web_assets.rs src/server.rs src/main.rs web/
git commit -m "feat(server): wire v2 monitors, shortcuts/speedtest APIs, config hot-reload, and modular asset serving"
```

---

### Task 10: OLED Appliance Frontend — Design System, Icon Rail, Charts, Screensaver & Core Views

**Files:**
- Modify: `web/index.html`
- Modify: `web/css/tokens.css`, `web/css/layout.css`, `web/css/views.css`
- Modify: `web/js/icons.js`, `web/js/charts.js`, `web/js/api.js`, `web/js/app.js`
- Modify: `web/js/views/home.js`, `web/js/views/pc.js`, `web/js/views/media.js`, `web/js/views/shortcuts.js`

**Interfaces:**
- Consumes: `/api/state`, `/api/stream` (`DashboardState` v2), `/api/media/{action}`, `/api/shortcuts/{id}/trigger`, `/api/network/speedtest`, `/api/approvals/{id}/decide`.
- Produces:
  - OLED dark design tokens (`#090b0e` bg, `#111619` surface, `#182026` elevated, `#1e2830` subtle borders, crisp tabular typography scaled for 18.5:9 landscape S8+ readability at 60–80cm).
  - Left 52px SVG icon rail (`nav-rail`) with 9 tab buttons (`home`, `pc`, `ai`, `agenda`, `weather`, `media`, `github`, `shortcuts`, `timer`), active rounded pill, live Timer badge, AI active/approval badge, fullscreen toggle, and bottom USB/PC status icon (green/amber/red).
  - Reusable SVG chart library (`web/js/charts.js`): `renderArcGauge`, `renderDualLineChart`, `renderBarChart`, `renderCompareBars`, `renderGithubHeatmap`, `renderTimerRing`.
  - Complete interactive views:
    - **`home.js`:** Top hero clock (`HH:MM` huge) + date, configurable Bento grid (`ai` 5h/weekly strip, `weather` hero temp+icon, `agenda` "A SEGUIR" single next event with color bar, `media` compact cover + title + circular play/pause, `github` 52-week heatmap + total + streak). Tapping any Bento card navigates directly to its dedicated tab.
    - **`pc.js`:** Segmented pill (`PC` | `Internet`); `PC` shows 4 circular arc gauges (`CPU`, `GPU` + °C, `RAM` GB, `VRAM` GB) + 60s dual CPU/GPU line chart + network rate + disks summary; `Internet` shows online pill, latency/jitter/loss KPIs, 60-probe chart, uptime/last drop, today/month data, and `Testar velocidade` button.
    - **`media.js`:** Dedicated hero player — big rounded album art on left, source app badge, large title/artist/album, interactive seek bar (`input[type="range"]` styled cleanly) with `mm:ss / mm:ss`, huge circular Prev / Play-Pause (white hero) / Next buttons, and clean volume `− / % / +` + Mute Mic pill.
    - **`shortcuts.js`:** 4×2 Stream Deck tile grid (auto-paginated with page dots when >8 tiles), large colored SVG icons, live illuminated active border/glow for `toggle_mic` / `toggle_mute`, tactile press animation calling `POST /api/shortcuts/{id}/trigger`.
  - **OLED Night Mode & PC-Lock Screensaver (`app.js`):** Automatically engages dim black `#000000` clock overlay with subtle 1-minute anti-burn-in pixel shift when `night_mode.enabled` matches current hour or `night_mode.dim_when_pc_locked && pc.pc_locked`. Tap anywhere wakes the screen for 30 seconds. **Approval overlay has higher `z-index` than screensaver** so urgent approvals always break through (pins Review Focus #5).

- [ ] **Step 1: Add frontend contract/regression tests in `src/server.rs`**

Add test `test_frontend_shell_contains_9_tabs_and_screensaver_zindex_hierarchy`:
- Asserts `web/index.html` contains all 9 view mounts (`view-home`, `view-pc`, `view-ai`, `view-agenda`, `view-weather`, `view-media`, `view-github`, `view-shortcuts`, `view-timer`), the screensaver overlay (`screensaverOverlay`), and the approval overlay (`approvalOverlay`).
- Asserts `web/css/layout.css` defines `z-index` for `.approval-overlay` strictly higher than `.screensaver-overlay` (pins Review Focus #5).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_frontend_shell_contains_9_tabs`
Expected: FAIL.

- [ ] **Step 3: Implement `web/index.html`, `web/css/*`, `web/js/{icons,charts,api,app}.js`, and `web/js/views/{home,pc,media,shortcuts}.js`**

- Build clean, semantic, zero-scroll-root OLED appliance UI calibrated for Galaxy S8+ landscape (`658×320` to `846×412` CSS px).
- Preserve Wake Lock, Web Audio chime unlock, token gate overlay, and remote approval overlay behavior from v1.

- [ ] **Step 4: Run `cargo test` and verify JS syntax with `node --check` (if Node available) or browser check**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/ src/server.rs
git commit -m "feat(ui): build OLED appliance shell, SVG charts, screensaver, and Home/PC/Media/Shortcuts views"
```

---

### Task 11: OLED Appliance Frontend — Productivity & Dev Views (`IA`, `Agenda`, `Clima`, `GitHub`, `Timer`)

**Files:**
- Modify: `web/js/views/ai.js`
- Modify: `web/js/views/agenda.js`
- Modify: `web/js/views/weather.js`
- Modify: `web/js/views/github.js`
- Modify: `web/js/views/timer.js`
- Modify: `web/css/views.css` (any view-specific polish)

**Interfaces:**
- Consumes: `state.ai`, `state.antigravity`, `state.agenda`, `state.todos`, `state.weather`, `state.github`, `state.notifications`, `state.remote_approval_enabled`, `charts.js`, `icons.js`, `api.js`.
- Produces:
  - **`ai.js`:** Segmented pill (`Uso` | `Sessões` | `Eventos`) + right-side runtime toggles (`Aprovação: OFF/ON`, `Som: ON/OFF`):
    - *Uso:* Top 3 cards (`JANELA 5H` with real tokens + optional `%`/reset bar, `SEMANAL` with real tokens + optional `%`/reset bar, `HOJE` table: Tokens / Output / Mensagens / Última hora); Bottom card split into `ÚLTIMOS 7 DIAS` coral bar chart (with peak label) + `MODELOS` horizontal proportional bars.
    - *Sessões:* Master–detail inspector for live Antigravity conversations (title, status badge, active tool/thinking, `task.md` checklist, recent actions, last prompt).
    - *Eventos:* Live hook notification feed.
  - **`agenda.js`:** Hero header `"Agenda"` + refresh button + compact collapsible pill button `"To-Do (open/total)"` that opens an overlay/drawer right inside the view without stealing space from Today's events. Main stage highlights **`HOJE • <data>`** first with vertical color accent bars, start–end times, title, and location; upcoming days (next 6 days) follow cleanly grouped below.
  - **`weather.js`:** Top hero card (big weather SVG icon, huge `XX°`, condition, city; right column: Sensação, Umidade, Vento, Atualizado); Middle 24-hour horizontal strip; Bottom 6-day forecast cards with high/low and rain probability %. Clean setup hint (`config.json`) when `configured === false`.
  - **`github.js`:** Top full-width `ÚLTIMO ANO` card (summary counts: contributions, commits, privados, issues + 52-week heatmap with month & weekday labels); Bottom 3 cards: `STREAK` (hero days, recorde, melhor dia), `ESTA SEMANA` (hero count, delta vs. semana passada, 7-day dual comparison bars), `TOP REPOS` (horizontal green bars). Clean setup hint when `configured === false`.
  - **`timer.js`:** Dedicated Pomodoro/countdown view with large SVG circular progress ring, big tabular `MM:SS`, quick presets (`5m`, `15m`, `25m`, `45m`), Start/Pause/Reset buttons, full-screen `"Tempo esgotado!"` dismissible card with chime/vibration, and live callback updating the Timer icon badge in the left rail whenever running.

- [ ] **Step 1: Add frontend module completeness check in `src/server.rs`**

Add test `test_all_9_view_modules_export_render_function`:
- Verifies all 9 embedded view JS modules served via `serve_web_asset` are non-empty and export `render`.

- [ ] **Step 2: Run test to verify it fails (for views still stubbed)**

Run: `cargo test test_all_9_view_modules_export_render_function`
Expected: FAIL until all 5 remaining views are implemented.

- [ ] **Step 3: Implement `ai.js`, `agenda.js`, `weather.js`, `github.js`, and `timer.js`**

- Match the visual density, spacing, color harmony, and glanceability of `zacharyinserra/deskhub` while preserving all DeskPulse-specific features (Antigravity live sessions, remote approval toggle, Today-first Agenda, collapsible To-Do drawer, PT-BR copy).

- [ ] **Step 4: Run `cargo test` and validate all JS modules**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/ src/server.rs
git commit -m "feat(ui): implement AI, Today-focused Agenda, Weather, GitHub, and Timer views"
```

---

### Task 12: Windows System Tray, Auto-Start Polish & Final Verification

**Files:**
- Modify: `Cargo.toml` (add `Win32_UI_Shell`, `Win32_UI_WindowsAndMessaging`, `Win32_Graphics_Gdi`, `Win32_System_LibraryLoader` features to `windows`)
- Create: `src/tray.rs`
- Modify: `src/main.rs`
- Modify: `README.md`

**Interfaces:**
- Consumes: `config_path`, `port`, `token`, `shutdown_tx: watch::Sender<bool>`, `run_open()`, startup registry helpers in `src/main.rs`.
- Produces:
  - `TrayHandle` (RAII handle removing tray icon & exiting message loop on drop)
  - `spawn_tray(opts: TrayOptions) -> Result<TrayHandle, String>` (Windows native `Shell_NotifyIconW` popup menu: `Abrir no Celular (ADB)`, `Abrir no Navegador`, `Editar Config (config.json)`, `Iniciar com o Windows`, `Sair`; no-op stub on non-Windows).

- [ ] **Step 1: Write unit test in `src/tray.rs` for menu command mapping and config-open path handling**

Add tests:
- `test_tray_menu_id_mapping_covers_all_actions`: verifies each Win32 menu command ID maps unambiguously to `TrayAction::{OpenPhone, OpenBrowser, EditConfig, ToggleStartup, Quit}` and unknown IDs return `None`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test tray::tests`
Expected: FAIL.

- [ ] **Step 3: Implement `src/tray.rs`, wire into `run_serve` in `src/main.rs`, and update `README.md`**

- Implement hidden Win32 window + `Shell_NotifyIconW` (`NIF_ICON | NIF_MESSAGE | NIF_TIP`) using default application/shield icon (`IDI_APPLICATION`) or embedded icon, right-click `TrackPopupMenu`, and graceful `shutdown_tx.send(true)` on `Sair` (also waking `axum::serve` graceful shutdown via `shutdown_rx` so quitting from tray terminates cleanly without Ctrl+C!).
- Wire `--no-tray` flag in `run_serve` to skip spawning tray when requested.
- Update `README.md` documenting all 9 views, `config.json` v2 example (Weather, GitHub, Shortcuts, Night Mode, AI budgets), `config-path`, `startup`, and `--no-tray`.

- [ ] **Step 4: Run full verification suite (`cargo test` + `cargo build --release` + `cargo clippy` if available)**

Run:
```powershell
cargo test
cargo build --release
```
Expected: 100% tests PASS, zero warnings/errors, `target/release/deskpulse.exe` builds cleanly.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/tray.rs src/main.rs README.md
git commit -m "feat(tray): add Windows System Tray, graceful tray shutdown, and v2 docs"
```
