use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::watch;

const ICS_POLL_INTERVAL: Duration = Duration::from_secs(300);
const ICS_HTTP_TIMEOUT: Duration = Duration::from_secs(10);
pub const AGENDA_WINDOW_DAYS: i64 = 7;

static TMP_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AgendaEvent {
    #[serde(default)]
    pub id: String,
    pub title: String,
    pub start_iso: String,
    #[serde(default)]
    pub end_iso: Option<String>,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub day_key: String,
    #[serde(default)]
    pub is_today: bool,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct AgendaSnapshot {
    pub configured: bool,
    pub ics_configured: bool,
    pub last_sync_ms: Option<u64>,
    pub error: Option<String>,
    pub events: Vec<AgendaEvent>,
}

#[derive(Default)]
struct AgendaInner {
    ics_url: Option<String>,
    ics_events: Vec<AgendaEvent>,
    pushed_events: Vec<AgendaEvent>,
    last_sync_ms: Option<u64>,
    error: Option<String>,
}

/// Manages today's Google Calendar events from both background `.ics` polling
/// and persistent `gcalendar` API pushes.
pub struct AgendaStore {
    push_path: PathBuf,
    client: reqwest::Client,
    inner: Mutex<AgendaInner>,
}

impl AgendaStore {
    pub fn new(push_path: PathBuf, initial_ics_url: Option<String>) -> Self {
        let today = Local::now().date_naive();
        let pushed_events = load_pushed_events(&push_path, today);
        let ics_url = normalize_optional_url(initial_ics_url);
        let client = reqwest::Client::builder()
            .timeout(ICS_HTTP_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            push_path,
            client,
            inner: Mutex::new(AgendaInner {
                ics_url,
                ics_events: Vec::new(),
                pushed_events,
                last_sync_ms: None,
                error: None,
            }),
        }
    }

    pub fn spawn_poller(self: &Arc<Self>, mut shutdown: watch::Receiver<bool>) {
        let store = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                if *shutdown.borrow() {
                    break;
                }

                let has_url = store
                    .inner
                    .lock()
                    .ok()
                    .and_then(|g| g.ics_url.clone())
                    .is_some();
                if has_url {
                    let _ = store.refresh_now().await;
                }

                tokio::select! {
                    _ = tokio::time::sleep(ICS_POLL_INTERVAL) => {}
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                }
            }
        });
    }

    pub fn set_ics_url(&self, url: Option<String>) {
        let normalized = normalize_optional_url(url);
        if let Ok(mut guard) = self.inner.lock() {
            if guard.ics_url != normalized {
                guard.ics_url = normalized;
                guard.ics_events.clear();
                guard.last_sync_ms = None;
                guard.error = None;
            }
        }
    }

    pub fn replace_pushed(&self, events: Vec<AgendaEvent>) -> Result<(), String> {
        let today = Local::now().date_naive();
        let normalized = normalize_pushed_events(events, today);

        let mut guard = self
            .inner
            .lock()
            .map_err(|_| "agenda store lock poisoned".to_string())?;
        write_json_atomic(&self.push_path, &normalized)?;
        guard.pushed_events = normalized;
        Ok(())
    }

    pub async fn refresh_now(&self) -> Result<AgendaSnapshot, String> {
        let ics_url = {
            let guard = self
                .inner
                .lock()
                .map_err(|_| "agenda store lock poisoned".to_string())?;
            guard.ics_url.clone()
        };

        let Some(raw_url) = ics_url else {
            return Ok(self.snapshot());
        };

        let fetch_url = to_http_fetch_url(&raw_url);
        let fetch_result = async {
            let resp = self
                .client
                .get(&fetch_url)
                .send()
                .await
                .map_err(|e| format!("failed to fetch ICS feed: {e}"))?;
            let status = resp.status();
            if !status.is_success() {
                return Err(format!("ICS feed returned HTTP {status}"));
            }
            resp.text()
                .await
                .map_err(|e| format!("failed to read ICS body: {e}"))
        }
        .await;

        match fetch_result {
            Ok(body) => {
                let today = Local::now().date_naive();
                let end_date = today + ChronoDuration::days(AGENDA_WINDOW_DAYS - 1);
                let parsed = parse_ics_for_date_range(&body, today, end_date);
                if let Ok(mut guard) = self.inner.lock() {
                    if guard.ics_url.as_deref() == Some(raw_url.as_str()) {
                        guard.ics_events = parsed;
                        guard.last_sync_ms = Some(now_epoch_ms());
                        guard.error = None;
                    }
                }
                Ok(self.snapshot())
            }
            Err(err) => {
                if let Ok(mut guard) = self.inner.lock() {
                    if guard.ics_url.as_deref() == Some(raw_url.as_str()) {
                        guard.error = Some(err.clone());
                    }
                }
                Err(err)
            }
        }
    }

    pub fn snapshot(&self) -> AgendaSnapshot {
        let today = Local::now().date_naive();
        let window_end = today + ChronoDuration::days(AGENDA_WINDOW_DAYS - 1);
        let Ok(mut guard) = self.inner.lock() else {
            return AgendaSnapshot::default();
        };

        guard
            .pushed_events
            .retain(|ev| is_event_in_window(ev, today, window_end));
        guard
            .ics_events
            .retain(|ev| is_event_in_window(ev, today, window_end));

        let ics_configured = guard.ics_url.is_some();
        let configured = ics_configured || !guard.pushed_events.is_empty();
        let events = merge_and_sort_events(&guard.pushed_events, &guard.ics_events, today);

        AgendaSnapshot {
            configured,
            ics_configured,
            last_sync_ms: guard.last_sync_ms,
            error: guard.error.clone(),
            events,
        }
    }
}

/// Parses an iCalendar (`.ics`) payload and returns events occurring on `target_date`.
#[allow(dead_code)]
pub fn parse_ics_for_date(ics: &str, target_date: NaiveDate) -> Vec<AgendaEvent> {
    parse_ics_for_date_range(ics, target_date, target_date)
}

/// Parses an iCalendar (`.ics`) payload and returns events occurring within `start_date ..= end_date`.
pub fn parse_ics_for_date_range(
    ics: &str,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Vec<AgendaEvent> {
    let lines = unfold_ics_lines(ics);
    let mut events = Vec::new();
    let mut current: Option<RawVEvent> = None;
    let mut sub_depth: usize = 0;

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if current.is_none() {
            if trimmed.eq_ignore_ascii_case("BEGIN:VEVENT") {
                current = Some(RawVEvent::default());
                sub_depth = 0;
            }
            continue;
        }

        if trimmed
            .get(..6)
            .is_some_and(|p| p.eq_ignore_ascii_case("BEGIN:"))
        {
            sub_depth += 1;
            continue;
        }

        if trimmed
            .get(..4)
            .is_some_and(|p| p.eq_ignore_ascii_case("END:"))
        {
            if sub_depth > 0 {
                sub_depth -= 1;
            } else if trimmed.eq_ignore_ascii_case("END:VEVENT") {
                if let Some(raw_ev) = current.take() {
                    if let Some(parsed) =
                        raw_ev.into_agenda_event_in_range(start_date, end_date, events.len())
                    {
                        events.push(parsed);
                    }
                }
            }
            continue;
        }

        if sub_depth > 0 {
            continue;
        }

        if let Some((name, params, value)) = parse_ics_property(&line) {
            if let Some(ev) = current.as_mut() {
                if name.eq_ignore_ascii_case("UID") {
                    ev.uid = Some(value.trim().to_string());
                } else if name.eq_ignore_ascii_case("SUMMARY") {
                    ev.summary = Some(unescape_ics_text(value).trim().to_string());
                } else if name.eq_ignore_ascii_case("LOCATION") {
                    let unescaped = unescape_ics_text(value).trim().to_string();
                    if !unescaped.is_empty() {
                        ev.location = Some(unescaped);
                    }
                } else if name.eq_ignore_ascii_case("STATUS") {
                    ev.cancelled = value.trim().eq_ignore_ascii_case("CANCELLED");
                } else if name.eq_ignore_ascii_case("DTSTART") {
                    ev.dtstart = parse_ics_datetime(value, &params);
                } else if name.eq_ignore_ascii_case("DTEND") {
                    ev.dtend = parse_ics_datetime(value, &params);
                }
            }
        }
    }

    sort_events_chronologically(&mut events);
    events
}

#[derive(Default)]
struct RawVEvent {
    uid: Option<String>,
    summary: Option<String>,
    location: Option<String>,
    dtstart: Option<IcsDateTime>,
    dtend: Option<IcsDateTime>,
    cancelled: bool,
}

impl RawVEvent {
    fn into_agenda_event_in_range(
        self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        fallback_idx: usize,
    ) -> Option<AgendaEvent> {
        if self.cancelled {
            return None;
        }

        let dtstart = self.dtstart?;
        let title = self
            .summary
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "(Sem título)".to_string());

        let (start_iso, end_iso, all_day, day_key) = match dtstart {
            IcsDateTime::Date(start_d) => {
                let end_d = match self.dtend {
                    Some(IcsDateTime::Date(d)) => Some(d),
                    Some(IcsDateTime::LocalDateTime(dt)) => Some(dt.date_naive()),
                    None => None,
                };
                let matches = match end_d {
                    Some(ed) if ed > start_d => start_d <= end_date && ed > start_date,
                    _ => start_d >= start_date && start_d <= end_date,
                };
                if !matches {
                    return None;
                }
                (
                    start_d.format("%Y-%m-%d").to_string(),
                    end_d.map(|d| d.format("%Y-%m-%d").to_string()),
                    true,
                    start_d.format("%Y-%m-%d").to_string(),
                )
            }
            IcsDateTime::LocalDateTime(start_dt) => {
                let end_dt = match self.dtend {
                    Some(IcsDateTime::LocalDateTime(ed)) => Some(ed),
                    Some(IcsDateTime::Date(d)) => local_day_start(d),
                    None => None,
                };
                let matches = match end_dt {
                    Some(ed) if ed > start_dt => {
                        let window_start_dt = local_day_start(start_date)?;
                        let window_end_dt = local_day_start(end_date + ChronoDuration::days(1))?;
                        start_dt < window_end_dt && ed > window_start_dt
                    }
                    _ => {
                        let d = start_dt.date_naive();
                        d >= start_date && d <= end_date
                    }
                };
                if !matches {
                    return None;
                }
                (
                    start_dt.to_rfc3339(),
                    end_dt.map(|ed| ed.to_rfc3339()),
                    false,
                    start_dt.date_naive().format("%Y-%m-%d").to_string(),
                )
            }
        };

        let id = self
            .uid
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| format!("ics-{fallback_idx}-{}", start_iso));

        let today_key = Local::now().date_naive().format("%Y-%m-%d").to_string();
        let is_today = day_key == today_key;

        Some(AgendaEvent {
            id,
            title,
            start_iso,
            end_iso,
            all_day,
            source: "ics".to_string(),
            location: self.location,
            day_key,
            is_today,
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum IcsDateTime {
    Date(NaiveDate),
    LocalDateTime(DateTime<Local>),
}

fn unfold_ics_lines(ics: &str) -> Vec<String> {
    let clean = ics.strip_prefix('\u{feff}').unwrap_or(ics);
    let normalized = clean.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = Vec::new();

    for raw_line in normalized.split('\n') {
        if raw_line.starts_with(' ') || raw_line.starts_with('\t') {
            if let Some(prev) = lines.last_mut() {
                prev.push_str(&raw_line[1..]);
                continue;
            }
        }
        lines.push(raw_line.to_string());
    }

    lines
}

type IcsProperty<'a> = (&'a str, Vec<(&'a str, &'a str)>, &'a str);

fn parse_ics_property(line: &str) -> Option<IcsProperty<'_>> {
    let mut in_quotes = false;
    let mut colon_idx = None;
    for (idx, ch) in line.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                colon_idx = Some(idx);
                break;
            }
            _ => {}
        }
    }

    let idx = colon_idx?;
    let prop_part = &line[..idx];
    let value = &line[idx + 1..];

    let mut segments = prop_part.split(';');
    let name = segments.next()?.trim();
    if name.is_empty() {
        return None;
    }

    let mut params = Vec::new();
    for seg in segments {
        if let Some((k, v)) = seg.split_once('=') {
            params.push((k.trim(), v.trim().trim_matches('"')));
        }
    }

    Some((name, params, value))
}

fn unescape_ics_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(',') => out.push(','),
                Some(';') => out.push(';'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn parse_ics_datetime(raw: &str, params: &[(&str, &str)]) -> Option<IcsDateTime> {
    let val = raw.trim();
    if val.is_empty() {
        return None;
    }

    let is_date_param = params
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("VALUE") && v.eq_ignore_ascii_case("DATE"));
    if is_date_param || (val.len() == 8 && val.bytes().all(|b| b.is_ascii_digit())) {
        return NaiveDate::parse_from_str(val, "%Y%m%d")
            .ok()
            .map(IcsDateTime::Date);
    }

    if val.ends_with('Z') || val.ends_with('z') {
        let trimmed = &val[..val.len() - 1];
        if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, "%Y%m%dT%H%M%S") {
            let utc_dt = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
            return Some(IcsDateTime::LocalDateTime(utc_dt.with_timezone(&Local)));
        }
    }

    if let Ok(dt) = DateTime::parse_from_rfc3339(val) {
        return Some(IcsDateTime::LocalDateTime(dt.with_timezone(&Local)));
    }

    if let Ok(naive) = NaiveDateTime::parse_from_str(val, "%Y%m%dT%H%M%S") {
        let is_utc_tzid = params.iter().any(|(k, v)| {
            k.eq_ignore_ascii_case("TZID")
                && (v.eq_ignore_ascii_case("UTC")
                    || v.eq_ignore_ascii_case("Etc/UTC")
                    || v.eq_ignore_ascii_case("GMT"))
        });
        if is_utc_tzid {
            let utc_dt = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
            return Some(IcsDateTime::LocalDateTime(utc_dt.with_timezone(&Local)));
        }
        return local_from_naive(naive).map(IcsDateTime::LocalDateTime);
    }

    None
}

fn local_day_start(date: NaiveDate) -> Option<DateTime<Local>> {
    date.and_hms_opt(0, 0, 0).and_then(local_from_naive)
}

fn local_from_naive(naive: NaiveDateTime) -> Option<DateTime<Local>> {
    Local
        .from_local_datetime(&naive)
        .single()
        .or_else(|| Local.from_local_datetime(&naive).earliest())
        .or_else(|| Local.from_local_datetime(&naive).latest())
}

fn normalize_optional_url(url: Option<String>) -> Option<String> {
    url.map(|u| u.trim().to_string()).filter(|u| !u.is_empty())
}

fn to_http_fetch_url(url: &str) -> String {
    if let Some((scheme, rest)) = url.split_once("://") {
        if scheme.eq_ignore_ascii_case("webcal") || scheme.eq_ignore_ascii_case("webcals") {
            return format!("https://{rest}");
        }
    }
    url.to_string()
}

fn compute_day_key(start_iso: &str) -> String {
    if let Some(parsed) = parse_event_datetime_or_date(start_iso) {
        match parsed {
            IcsDateTime::Date(d) => d.format("%Y-%m-%d").to_string(),
            IcsDateTime::LocalDateTime(dt) => dt.date_naive().format("%Y-%m-%d").to_string(),
        }
    } else if start_iso.len() >= 10 {
        start_iso[..10].to_string()
    } else {
        String::new()
    }
}

fn normalize_pushed_events(events: Vec<AgendaEvent>, today: NaiveDate) -> Vec<AgendaEvent> {
    let window_end = today + ChronoDuration::days(AGENDA_WINDOW_DAYS - 1);
    let today_key = today.format("%Y-%m-%d").to_string();
    let mut out = Vec::new();
    for (idx, mut ev) in events.into_iter().enumerate() {
        ev.title = ev.title.trim().to_string();
        ev.start_iso = ev.start_iso.trim().to_string();
        ev.end_iso = ev
            .end_iso
            .map(|e| e.trim().to_string())
            .filter(|e| !e.is_empty());
        if ev.title.is_empty() || ev.start_iso.is_empty() {
            continue;
        }
        ev.id = ev.id.trim().to_string();
        if ev.id.is_empty() {
            ev.id = format!("push-{idx}-{}", ev.start_iso);
        }
        ev.source = "push".to_string();
        ev.location = ev
            .location
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty());
        ev.day_key = compute_day_key(&ev.start_iso);
        ev.is_today = ev.day_key == today_key;

        if is_event_in_window(&ev, today, window_end) {
            out.push(ev);
        }
    }
    out
}

fn load_pushed_events(path: &Path, today: NaiveDate) -> Vec<AgendaEvent> {
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<Vec<AgendaEvent>>(&raw) else {
        return Vec::new();
    };
    normalize_pushed_events(parsed, today)
}

fn parse_event_datetime_or_date(raw: &str) -> Option<IcsDateTime> {
    let s = raw.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(IcsDateTime::LocalDateTime(dt.with_timezone(&Local)));
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return local_from_naive(naive).map(IcsDateTime::LocalDateTime);
    }
    if s.len() >= 10 {
        if let Ok(d) = NaiveDate::parse_from_str(&s[..10], "%Y-%m-%d") {
            return Some(IcsDateTime::Date(d));
        }
    }
    None
}

fn is_event_in_window(ev: &AgendaEvent, window_start: NaiveDate, window_end: NaiveDate) -> bool {
    let Some(start) = parse_event_datetime_or_date(&ev.start_iso) else {
        return false;
    };

    match start {
        IcsDateTime::Date(d) => {
            if let Some(end_raw) = ev.end_iso.as_deref() {
                if let Some(end) = parse_event_datetime_or_date(end_raw) {
                    let ed = match end {
                        IcsDateTime::Date(ed) => ed,
                        IcsDateTime::LocalDateTime(edt) => edt.date_naive(),
                    };
                    if ed > d {
                        return d <= window_end && ed > window_start;
                    }
                }
            }
            d >= window_start && d <= window_end
        }
        IcsDateTime::LocalDateTime(dt) => {
            if let Some(end_raw) = ev.end_iso.as_deref() {
                if let Some(end) = parse_event_datetime_or_date(end_raw) {
                    if let IcsDateTime::LocalDateTime(edt) = end {
                        if edt > dt {
                            if let (Some(w_start_dt), Some(w_end_dt)) = (
                                local_day_start(window_start),
                                local_day_start(window_end + ChronoDuration::days(1)),
                            ) {
                                return dt < w_end_dt && edt > w_start_dt;
                            }
                        }
                    }
                }
            }
            let d = dt.date_naive();
            d >= window_start && d <= window_end
        }
    }
}

fn merge_and_sort_events(
    pushed: &[AgendaEvent],
    ics: &[AgendaEvent],
    today: NaiveDate,
) -> Vec<AgendaEvent> {
    let mut seen: HashMap<(String, String), usize> = HashMap::new();
    let mut merged: Vec<AgendaEvent> = Vec::with_capacity(pushed.len() + ics.len());
    let today_key = today.format("%Y-%m-%d").to_string();

    for ev in pushed.iter().chain(ics.iter()) {
        let key = (normalize_title(&ev.title), normalize_start_key(&ev.start_iso));
        if let Some(&idx) = seen.get(&key) {
            if merged[idx].location.is_none() && ev.location.is_some() {
                merged[idx].location = ev.location.clone();
            }
        } else {
            let mut event = ev.clone();
            if event.day_key.is_empty() {
                event.day_key = compute_day_key(&event.start_iso);
            }
            event.is_today = event.day_key == today_key;
            seen.insert(key, merged.len());
            merged.push(event);
        }
    }

    sort_events_chronologically(&mut merged);
    merged
}

fn normalize_title(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn normalize_start_key(start_iso: &str) -> String {
    let trimmed = start_iso.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return format!("ts:{}", dt.timestamp());
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S") {
        if let Some(dt) = local_from_naive(naive) {
            return format!("ts:{}", dt.timestamp());
        }
    }
    trimmed.to_lowercase()
}

fn event_sort_timestamp(ev: &AgendaEvent) -> i64 {
    match parse_event_datetime_or_date(&ev.start_iso) {
        Some(IcsDateTime::LocalDateTime(dt)) => dt.timestamp(),
        Some(IcsDateTime::Date(d)) => local_day_start(d).map(|dt| dt.timestamp()).unwrap_or(0),
        None => i64::MAX,
    }
}

fn sort_events_chronologically(events: &mut [AgendaEvent]) {
    events.sort_by(|a, b| {
        event_sort_timestamp(a)
            .cmp(&event_sort_timestamp(b))
            .then_with(|| (!a.all_day).cmp(&(!b.all_day)))
            .then_with(|| a.start_iso.cmp(&b.start_iso))
            .then_with(|| a.title.cmp(&b.title))
    });
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create dir {}: {e}", parent.display()))?;
    }

    let json =
        serde_json::to_string_pretty(value).map_err(|e| format!("failed to serialize json: {e}"))?;
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_name = format!(
        ".{}.tmp.{}.{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("agenda_push.json"),
        std::process::id(),
        seq
    );
    let tmp_path = path.with_file_name(tmp_name);
    fs::write(&tmp_path, json.as_bytes())
        .map_err(|e| format!("failed to write temp file {}: {e}", tmp_path.display()))?;

    let mut last_err = None;
    for attempt in 0..5 {
        match fs::rename(&tmp_path, path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                if attempt < 4 {
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
        }
    }

    let _ = fs::remove_file(&tmp_path);
    Err(format!(
        "failed to rename temp file to {}: {}",
        path.display(),
        last_err.expect("rename failed at least once")
    ))
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn test_parse_ics_handles_unfolding_utc_to_local_all_day_and_missing_dtend() {
        let ics = concat!(
            "BEGIN:VCALENDAR\r\n",
            "VERSION:2.0\r\n",
            "BEGIN:VEVENT\r\n",
            "UID:evt-folded@google.com\r\n",
            "DTSTART:20261118T170000Z\r\n",
            "DTEND:20261118T180000Z\r\n",
            "SUMMARY:Weekly \r\n",
            " Architecture\\, \r\n",
            " Sync\r\n",
            "BEGIN:VALARM\r\n",
            "ACTION:DISPLAY\r\n",
            "SUMMARY:Alarm Summary Should Be Ignored\r\n",
            "TRIGGER:-PT10M\r\n",
            "END:VALARM\r\n",
            "END:VEVENT\r\n",
            "BEGIN:VEVENT\r\n",
            "UID:evt-allday@google.com\r\n",
            "DTSTART;VALUE=DATE:20261118\r\n",
            "DTEND;VALUE=DATE:20261119\r\n",
            "SUMMARY:Feriado Local\r\n",
            "END:VEVENT\r\n",
            "BEGIN:VEVENT\r\n",
            "UID:evt-no-dtend@google.com\r\n",
            "DTSTART:20261118T153000\r\n",
            "SUMMARY:Quick Check-in\r\n",
            "END:VEVENT\r\n",
            "BEGIN:VEVENT\r\n",
            "UID:evt-other-day@google.com\r\n",
            "DTSTART:20261119T120000Z\r\n",
            "DTEND:20261119T130000Z\r\n",
            "SUMMARY:Tomorrow Meeting\r\n",
            "END:VEVENT\r\n",
            "BEGIN:VEVENT\r\n",
            "UID:evt-cancelled@google.com\r\n",
            "DTSTART:20261118T120000Z\r\n",
            "STATUS:CANCELLED\r\n",
            "SUMMARY:Cancelled Event\r\n",
            "END:VEVENT\r\n",
            "END:VCALENDAR\r\n",
        );

        let utc_17 = Utc.with_ymd_and_hms(2026, 11, 18, 17, 0, 0).unwrap();
        let expected_local_17 = utc_17.with_timezone(&Local);
        let target_date = NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();

        let events = parse_ics_for_date(ics, target_date);
        assert_eq!(events.len(), 3, "expected 3 events for 2026-11-18, got {events:?}");

        let allday = events.iter().find(|e| e.id == "evt-allday@google.com").unwrap();
        assert!(allday.all_day);
        assert_eq!(allday.title, "Feriado Local");
        assert_eq!(allday.start_iso, "2026-11-18");
        assert_eq!(allday.source, "ics");

        let folded = events.iter().find(|e| e.id == "evt-folded@google.com").unwrap();
        assert!(!folded.all_day);
        assert_eq!(folded.title, "Weekly Architecture, Sync");
        assert_eq!(folded.start_iso, expected_local_17.to_rfc3339());
        assert!(folded.end_iso.is_some());
        assert_eq!(folded.source, "ics");

        let no_end = events.iter().find(|e| e.id == "evt-no-dtend@google.com").unwrap();
        assert!(!no_end.all_day);
        assert_eq!(no_end.title, "Quick Check-in");
        assert_eq!(no_end.end_iso, None);
        assert!(DateTime::parse_from_rfc3339(&no_end.start_iso).is_ok());
        assert_eq!(no_end.source, "ics");
    }

    #[tokio::test]
    async fn test_merge_deduplicates_and_preserves_pushed_events_across_ics_updates_and_expires_old_days() {
        let dir = TempDir::new().expect("tempdir");
        let push_path = dir.path().join("DeskPulse").join("agenda_push.json");

        let today = Local::now().date_naive();
        let yesterday = today - ChronoDuration::days(1);

        let standup_local = Local
            .from_local_datetime(&today.and_hms_opt(10, 0, 0).unwrap())
            .earliest()
            .unwrap();
        let standup_utc_ics = standup_local
            .with_timezone(&Utc)
            .format("%Y%m%dT%H%M%SZ")
            .to_string();

        let review_local = Local
            .from_local_datetime(&today.and_hms_opt(15, 30, 0).unwrap())
            .earliest()
            .unwrap();

        let sync_local = Local
            .from_local_datetime(&today.and_hms_opt(13, 0, 0).unwrap())
            .earliest()
            .unwrap();
        let sync_utc_ics = sync_local
            .with_timezone(&Utc)
            .format("%Y%m%dT%H%M%SZ")
            .to_string();

        let yesterday_local = Local
            .from_local_datetime(&yesterday.and_hms_opt(9, 0, 0).unwrap())
            .earliest()
            .unwrap();

        let store = AgendaStore::new(push_path.clone(), None);
        let initial = store.snapshot();
        assert!(!initial.configured);
        assert!(!initial.ics_configured);
        assert!(initial.events.is_empty());

        store
            .replace_pushed(vec![
                AgendaEvent {
                    id: "old-1".to_string(),
                    title: "Yesterday Event".to_string(),
                    start_iso: yesterday_local.to_rfc3339(),
                    end_iso: None,
                    all_day: false,
                    source: "push".to_string(),
                    ..Default::default()
                },
                AgendaEvent {
                    id: "push-standup".to_string(),
                    title: "Daily Standup".to_string(),
                    start_iso: standup_local.to_rfc3339(),
                    end_iso: None,
                    all_day: false,
                    source: "gcalendar".to_string(),
                    ..Default::default()
                },
                AgendaEvent {
                    id: "push-review".to_string(),
                    title: "Sprint Review (RRULE)".to_string(),
                    start_iso: review_local.to_rfc3339(),
                    end_iso: None,
                    all_day: false,
                    source: "".to_string(),
                    ..Default::default()
                },
            ])
            .expect("replace_pushed should succeed");

        assert!(push_path.exists());

        let reloaded = AgendaStore::new(push_path.clone(), None);
        let reloaded_snap = reloaded.snapshot();
        assert!(reloaded_snap.configured);
        assert!(!reloaded_snap.ics_configured);
        assert_eq!(reloaded_snap.events.len(), 2);
        assert!(reloaded_snap.events.iter().all(|e| e.source == "push"));
        assert!(!reloaded_snap
            .events
            .iter()
            .any(|e| e.title == "Yesterday Event"));

        let ics_body = format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n\
             BEGIN:VEVENT\r\nUID:ics-standup\r\nDTSTART:{standup_utc_ics}\r\nSUMMARY:daily standup\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:ics-sync\r\nDTSTART:{sync_utc_ics}\r\nSUMMARY:1:1 Sync\r\nEND:VEVENT\r\n\
             END:VCALENDAR\r\n"
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/calendar; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    ics_body.len(),
                    ics_body
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });

        reloaded.set_ics_url(Some(format!("http://{addr}/cal.ics")));
        let refreshed = reloaded.refresh_now().await.expect("refresh_now should succeed");
        let _ = server_task.await;

        assert!(refreshed.configured);
        assert!(refreshed.ics_configured);
        assert!(refreshed.last_sync_ms.is_some());
        assert_eq!(refreshed.error, None);
        assert_eq!(refreshed.events.len(), 3);

        assert_eq!(refreshed.events[0].title, "Daily Standup");
        assert_eq!(refreshed.events[0].source, "push");
        assert_eq!(refreshed.events[1].title, "1:1 Sync");
        assert_eq!(refreshed.events[1].source, "ics");
        assert_eq!(refreshed.events[2].title, "Sprint Review (RRULE)");
        assert_eq!(refreshed.events[2].source, "push");

        let stale_json = serde_json::to_string(&vec![
            AgendaEvent {
                id: "stale".to_string(),
                title: "Stale Past Event".to_string(),
                start_iso: (today - ChronoDuration::days(2))
                    .format("%Y-%m-%d")
                    .to_string(),
                end_iso: None,
                all_day: true,
                source: "push".to_string(),
                ..Default::default()
            },
            AgendaEvent {
                id: "today-allday".to_string(),
                title: "Today All Day".to_string(),
                start_iso: today.format("%Y-%m-%d").to_string(),
                end_iso: None,
                all_day: true,
                source: "push".to_string(),
                ..Default::default()
            },
        ])
        .unwrap();
        fs::write(&push_path, stale_json).unwrap();

        let pruned_store = AgendaStore::new(push_path, None);
        let pruned_snap = pruned_store.snapshot();
        assert_eq!(pruned_snap.events.len(), 1);
        assert_eq!(pruned_snap.events[0].id, "today-allday");
    }

    #[tokio::test]
    async fn test_refresh_now_records_http_error_and_poller_stops_on_shutdown() {
        let dir = TempDir::new().expect("tempdir");
        let push_path = dir.path().join("agenda_push.json");

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await;
                let resp = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });

        let store = Arc::new(AgendaStore::new(
            push_path,
            Some(format!("http://{addr}/broken.ics")),
        ));
        let (tx, rx) = watch::channel(false);
        store.spawn_poller(rx);

        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            let snap = store.snapshot();
            if snap.error.is_some() {
                assert!(snap.ics_configured);
                assert!(snap.configured);
                assert!(snap.error.unwrap().contains("500"));
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("timed out waiting for poller error state");
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        let _ = tx.send(true);
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn test_set_ics_url_clears_last_sync_ms_and_webcal_is_case_insensitive() {
        assert_eq!(
            to_http_fetch_url("WEBCAL://calendar.example.com/feed.ics"),
            "https://calendar.example.com/feed.ics"
        );
        assert_eq!(
            to_http_fetch_url("Webcal://calendar.example.com/feed.ics"),
            "https://calendar.example.com/feed.ics"
        );
        assert_eq!(
            to_http_fetch_url("webcals://calendar.example.com/feed.ics"),
            "https://calendar.example.com/feed.ics"
        );
        assert_eq!(
            to_http_fetch_url("WEBCALS://calendar.example.com/feed.ics"),
            "https://calendar.example.com/feed.ics"
        );

        let dir = TempDir::new().expect("tempdir");
        let push_path = dir.path().join("agenda_push.json");

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await;
                let body = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n";
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/calendar\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });

        let url1 = format!("http://{addr}/cal1.ics");
        let store = AgendaStore::new(push_path, Some(url1.clone()));
        let synced = store.refresh_now().await.expect("initial sync");
        let _ = server_task.await;
        assert!(synced.last_sync_ms.is_some());

        // Setting the exact same URL keeps last_sync_ms intact
        store.set_ics_url(Some(url1));
        assert!(store.snapshot().last_sync_ms.is_some());

        // Changing the URL clears last_sync_ms, error, and ics_events
        store.set_ics_url(Some("http://127.0.0.1:9/cal2.ics".to_string()));
        let after_change = store.snapshot();
        assert!(after_change.ics_configured);
        assert_eq!(after_change.last_sync_ms, None);
        assert_eq!(after_change.error, None);
    }

    #[test]
    fn test_parse_ics_includes_next_7_days_location_and_excludes_day_8() {
        let today = Local::now().date_naive();
        let yesterday = today - ChronoDuration::days(1);
        let day_plus_3 = today + ChronoDuration::days(3);
        let day_plus_8 = today + ChronoDuration::days(8);

        let ics = format!(
            "BEGIN:VCALENDAR\r\n\
             VERSION:2.0\r\n\
             BEGIN:VEVENT\r\n\
             UID:evt-yesterday\r\n\
             DTSTART:{}T100000Z\r\n\
             SUMMARY:Yesterday Meeting\r\n\
             END:VEVENT\r\n\
             BEGIN:VEVENT\r\n\
             UID:evt-today\r\n\
             DTSTART:{}T110000Z\r\n\
             SUMMARY:Today Meeting\r\n\
             LOCATION:Google Meet\r\n\
             END:VEVENT\r\n\
             BEGIN:VEVENT\r\n\
             UID:evt-plus3\r\n\
             DTSTART:{}T140000Z\r\n\
             SUMMARY:Future Meeting\r\n\
             END:VEVENT\r\n\
             BEGIN:VEVENT\r\n\
             UID:evt-plus8\r\n\
             DTSTART:{}T090000Z\r\n\
             SUMMARY:Far Future Meeting\r\n\
             END:VEVENT\r\n\
             END:VCALENDAR\r\n",
            yesterday.format("%Y%m%d"),
            today.format("%Y%m%d"),
            day_plus_3.format("%Y%m%d"),
            day_plus_8.format("%Y%m%d"),
        );

        let events = parse_ics_for_date_range(
            &ics,
            today,
            today + ChronoDuration::days(6),
        );

        assert_eq!(events.len(), 2, "expected only today and +3 days, got: {events:?}");

        let ev_today = &events[0];
        assert_eq!(ev_today.id, "evt-today");
        assert_eq!(ev_today.title, "Today Meeting");
        assert_eq!(ev_today.location.as_deref(), Some("Google Meet"));
        assert_eq!(ev_today.day_key, today.format("%Y-%m-%d").to_string());
        assert!(ev_today.is_today);

        let ev_plus3 = &events[1];
        assert_eq!(ev_plus3.id, "evt-plus3");
        assert_eq!(ev_plus3.title, "Future Meeting");
        assert_eq!(ev_plus3.day_key, day_plus_3.format("%Y-%m-%d").to_string());
        assert!(!ev_plus3.is_today);
    }

    #[test]
    fn test_push_events_auto_populates_day_key_and_is_today() {
        let dir = TempDir::new().expect("tempdir");
        let push_path = dir.path().join("agenda_push.json");
        let store = AgendaStore::new(push_path, None);

        let today = Local::now().date_naive();
        let yesterday = today - ChronoDuration::days(1);
        let day_plus_2 = today + ChronoDuration::days(2);
        let day_plus_6 = today + ChronoDuration::days(6);
        let day_plus_8 = today + ChronoDuration::days(8);

        let json_payload = format!(
            r#"[
                {{
                    "title": "Yesterday Event",
                    "start_iso": "{}T09:00:00"
                }},
                {{
                    "title": "Today Event",
                    "start_iso": "{}T10:00:00"
                }},
                {{
                    "title": "Midweek Event",
                    "start_iso": "{}T14:00:00"
                }},
                {{
                    "title": "Day 6 Window Boundary",
                    "start_iso": "{}T16:00:00"
                }},
                {{
                    "title": "Day 8 Beyond Window",
                    "start_iso": "{}T11:00:00"
                }}
            ]"#,
            yesterday.format("%Y-%m-%d"),
            today.format("%Y-%m-%d"),
            day_plus_2.format("%Y-%m-%d"),
            day_plus_6.format("%Y-%m-%d"),
            day_plus_8.format("%Y-%m-%d"),
        );

        let pushed: Vec<AgendaEvent> =
            serde_json::from_str(&json_payload).expect("deserialize pushed payload");
        store.replace_pushed(pushed).expect("replace_pushed");

        let snap = store.snapshot();
        assert_eq!(
            snap.events.len(),
            3,
            "expected exactly 3 events (today, +2d, +6d), got: {:?}",
            snap.events
        );

        let ev0 = &snap.events[0];
        assert_eq!(ev0.title, "Today Event");
        assert_eq!(ev0.day_key, today.format("%Y-%m-%d").to_string());
        assert!(ev0.is_today);
        assert_eq!(ev0.location, None);

        let ev1 = &snap.events[1];
        assert_eq!(ev1.title, "Midweek Event");
        assert_eq!(ev1.day_key, day_plus_2.format("%Y-%m-%d").to_string());
        assert!(!ev1.is_today);
        assert_eq!(ev1.location, None);

        let ev2 = &snap.events[2];
        assert_eq!(ev2.title, "Day 6 Window Boundary");
        assert_eq!(ev2.day_key, day_plus_6.format("%Y-%m-%d").to_string());
        assert!(!ev2.is_today);
        assert_eq!(ev2.location, None);
    }
}
