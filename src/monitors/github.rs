use crate::config::GithubConfig;
use chrono::Datelike;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{watch, Notify};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GithubDayCell {
    pub date: String,
    pub count: u32,
    pub level: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GithubRepoStat {
    pub name: String,
    pub commits: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct GithubSnapshot {
    pub configured: bool,
    pub username: String,
    pub total_contributions: u32,
    pub commits: u32,
    pub private_contributions: u32,
    pub prs: u32,
    pub issues: u32,
    pub current_streak: u32,
    pub longest_streak: u32,
    pub best_day_count: u32,
    pub best_day_label: String,
    pub this_week_total: u32,
    pub last_week_total: u32,
    pub this_week_days: [u32; 7],
    pub last_week_days: [u32; 7],
    pub top_repos: Vec<GithubRepoStat>,
    pub weeks: Vec<Vec<GithubDayCell>>,
    pub updated_at_ms: Option<u64>,
    pub error: Option<String>,
}

#[allow(dead_code)]
pub struct GithubMonitor {
    snapshot: Arc<RwLock<GithubSnapshot>>,
    config: Arc<RwLock<GithubConfig>>,
    notify: Arc<Notify>,
}

#[allow(dead_code)]
impl GithubMonitor {
    pub fn spawn(initial: GithubConfig, shutdown: watch::Receiver<bool>) -> Self {
        let is_configured = initial.resolve_token().is_some();
        let snap = GithubSnapshot {
            configured: is_configured,
            username: initial.username.clone().unwrap_or_default(),
            ..Default::default()
        };

        let snapshot = Arc::new(RwLock::new(snap));
        let config = Arc::new(RwLock::new(initial));
        let notify = Arc::new(Notify::new());

        let snapshot_bg = Arc::clone(&snapshot);
        let config_bg = Arc::clone(&config);
        let notify_bg = Arc::clone(&notify);
        let mut shutdown_bg = shutdown;

        tokio::spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap_or_default();
            let poll_interval = Duration::from_secs(15 * 60);

            loop {
                if *shutdown_bg.borrow() {
                    break;
                }

                let current_cfg = config_bg.read().unwrap().clone();
                let token = current_cfg.resolve_token();

                match token {
                    None => {
                        if let Ok(mut snap) = snapshot_bg.write() {
                            *snap = GithubSnapshot {
                                configured: false,
                                username: current_cfg.username.clone().unwrap_or_default(),
                                error: None,
                                ..Default::default()
                            };
                        }
                    }
                    Some(token_str) => {
                        let has_username = current_cfg
                            .username
                            .as_deref()
                            .map(str::trim)
                            .map(|s| !s.is_empty())
                            .unwrap_or(false);

                        let (query, variables) = if has_username {
                            let login = current_cfg.username.as_deref().unwrap().trim();
                            (
                                r#"query($login: String!) {
  user(login: $login) {
    login
    contributionsCollection {
      totalCommitContributions
      restrictedContributionsCount
      totalPullRequestContributions
      totalIssueContributions
      contributionCalendar {
        totalContributions
        weeks {
          contributionDays {
            date
            contributionCount
            contributionLevel
            weekday
          }
        }
      }
      commitContributionsByRepository(maxRepositories: 20) {
        repository {
          nameWithOwner
          name
        }
        contributions {
          totalCount
        }
      }
    }
  }
}"#,
                                serde_json::json!({ "login": login }),
                            )
                        } else {
                            (
                                r#"query {
  viewer {
    login
    contributionsCollection {
      totalCommitContributions
      restrictedContributionsCount
      totalPullRequestContributions
      totalIssueContributions
      contributionCalendar {
        totalContributions
        weeks {
          contributionDays {
            date
            contributionCount
            contributionLevel
            weekday
          }
        }
      }
      commitContributionsByRepository(maxRepositories: 20) {
        repository {
          nameWithOwner
          name
        }
        contributions {
          totalCount
        }
      }
    }
  }
}"#,
                                serde_json::json!({}),
                            )
                        };

                        let body = serde_json::json!({
                            "query": query,
                            "variables": variables,
                        });

                        let request_res = client
                            .post("https://api.github.com/graphql")
                            .header("User-Agent", "DeskPulse-Dashboard")
                            .header("Authorization", format!("Bearer {token_str}"))
                            .json(&body)
                            .send()
                            .await;

                        match request_res {
                            Ok(resp) => {
                                let status = resp.status();
                                if !status.is_success() {
                                    let err_msg = if status.as_u16() == 401 {
                                        "Erro de autenticação no GitHub: token inválido ou expirado".to_string()
                                    } else if status.as_u16() == 403 {
                                        "Acesso negado pela API do GitHub: verifique permissões ou rate limit".to_string()
                                    } else {
                                        format!("Erro da API do GitHub: HTTP {status}")
                                    };
                                    if let Ok(mut snap) = snapshot_bg.write() {
                                        snap.configured = true;
                                        snap.error = Some(err_msg);
                                    }
                                } else {
                                    match resp.json::<serde_json::Value>().await {
                                        Ok(json_resp) => {
                                            match parse_github_graphql_response(&json_resp) {
                                                Ok(mut snap) => {
                                                    snap.configured = true;
                                                    snap.updated_at_ms = Some(
                                                        std::time::SystemTime::now()
                                                            .duration_since(std::time::UNIX_EPOCH)
                                                            .unwrap_or_default()
                                                            .as_millis() as u64,
                                                    );
                                                    snap.error = None;
                                                    if let Ok(mut cur) = snapshot_bg.write() {
                                                        *cur = snap;
                                                    }
                                                }
                                                Err(err) => {
                                                    if let Ok(mut snap) = snapshot_bg.write() {
                                                        snap.configured = true;
                                                        snap.error = Some(format!(
                                                            "Falha ao interpretar dados do GitHub: {err}"
                                                        ));
                                                    }
                                                }
                                            }
                                        }
                                        Err(err) => {
                                            if let Ok(mut snap) = snapshot_bg.write() {
                                                snap.configured = true;
                                                snap.error = Some(format!(
                                                    "Falha ao decodificar resposta JSON do GitHub: {err}"
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                            Err(err) => {
                                if let Ok(mut snap) = snapshot_bg.write() {
                                    snap.configured = true;
                                    snap.error = Some(format!("Falha ao conectar com o GitHub: {err}"));
                                }
                            }
                        }
                    }
                }

                tokio::select! {
                    _ = tokio::time::sleep(poll_interval) => {}
                    _ = notify_bg.notified() => {}
                    changed = shutdown_bg.changed() => {
                        if changed.is_err() || *shutdown_bg.borrow() {
                            break;
                        }
                    }
                }
            }
        });

        Self {
            snapshot,
            config,
            notify,
        }
    }

    pub fn snapshot(&self) -> GithubSnapshot {
        self.snapshot.read().unwrap().clone()
    }

    pub fn update_config(&self, cfg: GithubConfig) {
        let changed = {
            let mut cur = self.config.write().unwrap();
            if *cur != cfg {
                *cur = cfg;
                true
            } else {
                false
            }
        };
        if changed {
            self.notify.notify_one();
        }
    }
}

fn parse_level(val: Option<&serde_json::Value>) -> u8 {
    match val {
        Some(serde_json::Value::String(s)) => match s.as_str() {
            "FIRST_QUARTILE" => 1,
            "SECOND_QUARTILE" => 2,
            "THIRD_QUARTILE" => 3,
            "FOURTH_QUARTILE" => 4,
            _ => 0,
        },
        Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0).min(4) as u8,
        _ => 0,
    }
}

#[allow(dead_code)]
pub fn parse_github_graphql_response(json: &serde_json::Value) -> Result<GithubSnapshot, String> {
    if let Some(errors) = json.get("errors").and_then(|e| e.as_array()) {
        if let Some(first) = errors.first() {
            let msg = first
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Erro retornado pela API GraphQL do GitHub");
            return Err(msg.to_string());
        }
    }

    let target = if let Some(data) = json.get("data") {
        data.get("viewer")
            .or_else(|| data.get("user"))
            .unwrap_or(data)
    } else if let Some(viewer) = json.get("viewer") {
        viewer
    } else if let Some(user) = json.get("user") {
        user
    } else {
        json
    };

    let username = target
        .get("login")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let collection = target
        .get("contributionsCollection")
        .ok_or_else(|| "Resposta GraphQL não contém 'contributionsCollection'".to_string())?;

    let total_commits = collection
        .get("totalCommitContributions")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let private_contributions = collection
        .get("restrictedContributionsCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let prs = collection
        .get("totalPullRequestContributions")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let issues = collection
        .get("totalIssueContributions")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let calendar = collection.get("contributionCalendar");

    let mut weeks: Vec<Vec<GithubDayCell>> = Vec::new();
    let mut weeks_weekdays: Vec<Vec<usize>> = Vec::new();

    if let Some(cal_weeks) = calendar.and_then(|c| c.get("weeks")).and_then(|w| w.as_array()) {
        for week_val in cal_weeks {
            let mut week_cells = Vec::new();
            let mut week_wdays = Vec::new();
            if let Some(days) = week_val.get("contributionDays").and_then(|d| d.as_array()) {
                for (idx, day_val) in days.iter().enumerate() {
                    let date = day_val
                        .get("date")
                        .and_then(|d| d.as_str())
                        .unwrap_or("")
                        .to_string();
                    let count = day_val
                        .get("contributionCount")
                        .and_then(|c| c.as_u64())
                        .unwrap_or(0) as u32;
                    let level = parse_level(day_val.get("contributionLevel"));

                    let weekday = day_val
                        .get("weekday")
                        .and_then(|w| w.as_u64())
                        .map(|w| (w as usize).min(6))
                        .or_else(|| {
                            chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                                .ok()
                                .map(|d| d.weekday().num_days_from_sunday() as usize)
                        })
                        .unwrap_or_else(|| idx.min(6));

                    week_cells.push(GithubDayCell { date, count, level });
                    week_wdays.push(weekday);
                }
            }
            weeks.push(week_cells);
            weeks_weekdays.push(week_wdays);
        }
    }

    let all_days: Vec<&GithubDayCell> = weeks.iter().flatten().collect();

    let total_contributions = calendar
        .and_then(|c| c.get("totalContributions"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or_else(|| all_days.iter().map(|c| c.count).sum());

    // Compute current_streak (allowing today to be 0 if yesterday > 0)
    let n = all_days.len();
    let current_streak = if n == 0 {
        0
    } else if all_days[n - 1].count > 0 {
        let mut streak = 0;
        for i in (0..n).rev() {
            if all_days[i].count > 0 {
                streak += 1;
            } else {
                break;
            }
        }
        streak
    } else if n >= 2 && all_days[n - 2].count > 0 {
        let mut streak = 0;
        for i in (0..=n - 2).rev() {
            if all_days[i].count > 0 {
                streak += 1;
            } else {
                break;
            }
        }
        streak
    } else {
        0
    };

    // Compute longest_streak across all days
    let mut longest_streak = 0;
    let mut current_run = 0;
    for day in &all_days {
        if day.count > 0 {
            current_run += 1;
            if current_run > longest_streak {
                longest_streak = current_run;
            }
        } else {
            current_run = 0;
        }
    }

    // Compute best_day
    let mut best_day_count = 0;
    let mut best_day_label = String::new();
    for day in &all_days {
        if day.count > best_day_count {
            best_day_count = day.count;
            best_day_label = day.date.clone();
        }
    }

    // Compute this_week_days vs last_week_days (Sunday..Saturday)
    let mut this_week_days = [0u32; 7];
    let mut last_week_days = [0u32; 7];

    let w_count = weeks.len();
    if w_count > 0 {
        for (cell, &wday) in weeks[w_count - 1].iter().zip(&weeks_weekdays[w_count - 1]) {
            this_week_days[wday] = cell.count;
        }
    }
    if w_count >= 2 {
        for (cell, &wday) in weeks[w_count - 2].iter().zip(&weeks_weekdays[w_count - 2]) {
            last_week_days[wday] = cell.count;
        }
    }
    let this_week_total = this_week_days.iter().sum();
    let last_week_total = last_week_days.iter().sum();

    // Top 4 repos sorted descending
    let mut top_repos = Vec::new();
    if let Some(repos) = collection
        .get("commitContributionsByRepository")
        .and_then(|r| r.as_array())
    {
        for r in repos {
            let name = r
                .get("repository")
                .and_then(|repo| {
                    repo.get("nameWithOwner")
                        .or_else(|| repo.get("name"))
                        .and_then(|n| n.as_str())
                        .or_else(|| repo.as_str())
                })
                .or_else(|| r.get("name").and_then(|n| n.as_str()))
                .unwrap_or("")
                .trim()
                .to_string();

            if name.is_empty() {
                continue;
            }

            let commits = r
                .get("contributions")
                .and_then(|c| {
                    c.get("totalCount")
                        .and_then(|v| v.as_u64())
                        .or_else(|| c.as_u64())
                })
                .or_else(|| r.get("commits").and_then(|v| v.as_u64()))
                .unwrap_or(0) as u32;

            top_repos.push(GithubRepoStat { name, commits });
        }
    }
    top_repos.sort_by(|a, b| b.commits.cmp(&a.commits).then_with(|| a.name.cmp(&b.name)));
    top_repos.truncate(4);

    Ok(GithubSnapshot {
        configured: true,
        username,
        total_contributions,
        commits: total_commits,
        private_contributions,
        prs,
        issues,
        current_streak,
        longest_streak,
        best_day_count,
        best_day_label,
        this_week_total,
        last_week_total,
        this_week_days,
        last_week_days,
        top_repos,
        weeks,
        updated_at_ms: None,
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_github_graphql_computes_streaks_best_day_weekly_delta_and_top_repos() {
        let fixture = json!({
            "data": {
                "viewer": {
                    "login": "octocat",
                    "contributionsCollection": {
                        "totalCommitContributions": 85,
                        "restrictedContributionsCount": 20,
                        "totalPullRequestContributions": 10,
                        "totalIssueContributions": 5,
                        "contributionCalendar": {
                            "totalContributions": 120,
                            "weeks": [
                                {
                                    "contributionDays": [
                                        { "date": "2026-11-01", "contributionCount": 2, "contributionLevel": "FIRST_QUARTILE", "weekday": 0 },
                                        { "date": "2026-11-02", "contributionCount": 3, "contributionLevel": "FIRST_QUARTILE", "weekday": 1 },
                                        { "date": "2026-11-03", "contributionCount": 4, "contributionLevel": "SECOND_QUARTILE", "weekday": 2 },
                                        { "date": "2026-11-04", "contributionCount": 15, "contributionLevel": "FOURTH_QUARTILE", "weekday": 3 },
                                        { "date": "2026-11-05", "contributionCount": 2, "contributionLevel": "FIRST_QUARTILE", "weekday": 4 },
                                        { "date": "2026-11-06", "contributionCount": 1, "contributionLevel": "FIRST_QUARTILE", "weekday": 5 },
                                        { "date": "2026-11-07", "contributionCount": 1, "contributionLevel": "FIRST_QUARTILE", "weekday": 6 }
                                    ]
                                },
                                {
                                    "contributionDays": [
                                        { "date": "2026-11-08", "contributionCount": 0, "contributionLevel": "NONE", "weekday": 0 },
                                        { "date": "2026-11-09", "contributionCount": 1, "contributionLevel": "FIRST_QUARTILE", "weekday": 1 },
                                        { "date": "2026-11-10", "contributionCount": 2, "contributionLevel": "FIRST_QUARTILE", "weekday": 2 },
                                        { "date": "2026-11-11", "contributionCount": 0, "contributionLevel": "NONE", "weekday": 3 },
                                        { "date": "2026-11-12", "contributionCount": 3, "contributionLevel": "SECOND_QUARTILE", "weekday": 4 },
                                        { "date": "2026-11-13", "contributionCount": 0, "contributionLevel": "NONE", "weekday": 5 },
                                        { "date": "2026-11-14", "contributionCount": 4, "contributionLevel": "SECOND_QUARTILE", "weekday": 6 }
                                    ]
                                },
                                {
                                    "contributionDays": [
                                        { "date": "2026-11-15", "contributionCount": 0, "contributionLevel": "NONE", "weekday": 0 },
                                        { "date": "2026-11-16", "contributionCount": 0, "contributionLevel": "NONE", "weekday": 1 },
                                        { "date": "2026-11-17", "contributionCount": 0, "contributionLevel": "NONE", "weekday": 2 },
                                        { "date": "2026-11-18", "contributionCount": 0, "contributionLevel": "NONE", "weekday": 3 },
                                        { "date": "2026-11-19", "contributionCount": 2, "contributionLevel": "FIRST_QUARTILE", "weekday": 4 },
                                        { "date": "2026-11-20", "contributionCount": 5, "contributionLevel": "THIRD_QUARTILE", "weekday": 5 },
                                        { "date": "2026-11-21", "contributionCount": 0, "contributionLevel": "NONE", "weekday": 6 }
                                    ]
                                }
                            ]
                        },
                        "commitContributionsByRepository": [
                            {
                                "repository": { "nameWithOwner": "user/repo-alpha", "name": "repo-alpha" },
                                "contributions": { "totalCount": 10 }
                            },
                            {
                                "repository": { "nameWithOwner": "user/repo-beta", "name": "repo-beta" },
                                "contributions": { "totalCount": 50 }
                            },
                            {
                                "repository": { "nameWithOwner": "user/repo-gamma", "name": "repo-gamma" },
                                "contributions": { "totalCount": 5 }
                            },
                            {
                                "repository": { "nameWithOwner": "user/repo-delta", "name": "repo-delta" },
                                "contributions": { "totalCount": 25 }
                            },
                            {
                                "repository": { "nameWithOwner": "user/repo-epsilon", "name": "repo-epsilon" },
                                "contributions": { "totalCount": 35 }
                            }
                        ]
                    }
                }
            }
        });

        let snapshot = parse_github_graphql_response(&fixture).expect("parsing should succeed");

        assert_eq!(snapshot.username, "octocat");
        assert_eq!(snapshot.total_contributions, 120);
        assert_eq!(snapshot.commits, 85);
        assert_eq!(snapshot.private_contributions, 20);
        assert_eq!(snapshot.prs, 10);
        assert_eq!(snapshot.issues, 5);

        // Streaks: yesterday (Nov 20) had 5, today (Nov 21) is 0 -> current streak preserved at 2.
        assert_eq!(snapshot.current_streak, 2);
        // Historical longest streak in Week 1 had 7 consecutive days (Nov 1..7).
        assert_eq!(snapshot.longest_streak, 7);

        // Best day: Nov 4 with 15 contributions.
        assert_eq!(snapshot.best_day_count, 15);
        assert_eq!(snapshot.best_day_label, "2026-11-04");

        // Weekly comparison:
        // This week (Week 3): 0+0+0+0+2+5+0 = 7
        assert_eq!(snapshot.this_week_total, 7);
        assert_eq!(snapshot.this_week_days, [0, 0, 0, 0, 2, 5, 0]);
        // Last week (Week 2): 0+1+2+0+3+0+4 = 10
        assert_eq!(snapshot.last_week_total, 10);
        assert_eq!(snapshot.last_week_days, [0, 1, 2, 0, 3, 0, 4]);

        // Top 4 repos sorted descending by commits:
        assert_eq!(snapshot.top_repos.len(), 4);
        assert_eq!(snapshot.top_repos[0].name, "user/repo-beta");
        assert_eq!(snapshot.top_repos[0].commits, 50);
        assert_eq!(snapshot.top_repos[1].name, "user/repo-epsilon");
        assert_eq!(snapshot.top_repos[1].commits, 35);
        assert_eq!(snapshot.top_repos[2].name, "user/repo-delta");
        assert_eq!(snapshot.top_repos[2].commits, 25);
        assert_eq!(snapshot.top_repos[3].name, "user/repo-alpha");
        assert_eq!(snapshot.top_repos[3].commits, 10);
    }

    #[tokio::test]
    async fn test_github_snapshot_never_contains_token_and_handles_unconfigured() {
        let (_tx, rx) = watch::channel(false);
        let cfg = GithubConfig {
            username: Some("unconfigured_user".to_string()),
            token: None,
            token_env: None,
        };

        let monitor = GithubMonitor::spawn(cfg, rx);
        let snapshot = monitor.snapshot();

        assert!(!snapshot.configured);
        assert_eq!(snapshot.error, None);
        assert_eq!(snapshot.username, "unconfigured_user");

        let serialized = serde_json::to_string(&snapshot).expect("serialize snapshot");
        assert!(!serialized.contains("token"));
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("ghp_"));
        assert!(!serialized.contains("password"));
    }

    #[test]
    fn test_parse_github_graphql_handles_today_active_streak() {
        let fixture = json!({
            "data": {
                "user": {
                    "login": "activecoder",
                    "contributionsCollection": {
                        "totalCommitContributions": 10,
                        "restrictedContributionsCount": 0,
                        "totalPullRequestContributions": 0,
                        "totalIssueContributions": 0,
                        "contributionCalendar": {
                            "totalContributions": 10,
                            "weeks": [
                                {
                                    "contributionDays": [
                                        { "date": "2026-11-19", "contributionCount": 2, "contributionLevel": "FIRST_QUARTILE", "weekday": 4 },
                                        { "date": "2026-11-20", "contributionCount": 3, "contributionLevel": "FIRST_QUARTILE", "weekday": 5 },
                                        { "date": "2026-11-21", "contributionCount": 4, "contributionLevel": "SECOND_QUARTILE", "weekday": 6 }
                                    ]
                                }
                            ]
                        },
                        "commitContributionsByRepository": []
                    }
                }
            }
        });

        let snapshot = parse_github_graphql_response(&fixture).expect("parsing should succeed");
        assert_eq!(snapshot.username, "activecoder");
        // All 3 days active, including today -> current_streak = 3
        assert_eq!(snapshot.current_streak, 3);
        assert_eq!(snapshot.longest_streak, 3);
        assert_eq!(snapshot.best_day_count, 4);
        assert_eq!(snapshot.best_day_label, "2026-11-21");
    }

    #[test]
    fn test_parse_github_graphql_returns_error_on_graphql_errors() {
        let fixture = json!({
            "errors": [
                { "message": "Bad credentials" }
            ]
        });

        let err = parse_github_graphql_response(&fixture).unwrap_err();
        assert!(err.contains("Bad credentials"));
    }

    #[test]
    fn test_parse_level_mappings() {
        assert_eq!(parse_level(Some(&json!("NONE"))), 0);
        assert_eq!(parse_level(Some(&json!("FIRST_QUARTILE"))), 1);
        assert_eq!(parse_level(Some(&json!("SECOND_QUARTILE"))), 2);
        assert_eq!(parse_level(Some(&json!("THIRD_QUARTILE"))), 3);
        assert_eq!(parse_level(Some(&json!("FOURTH_QUARTILE"))), 4);
        assert_eq!(parse_level(Some(&json!("UNKNOWN"))), 0);
        assert_eq!(parse_level(Some(&json!(3))), 3);
        assert_eq!(parse_level(None), 0);
    }

    #[tokio::test]
    async fn test_github_monitor_update_config() {
        let (_tx, rx) = watch::channel(false);
        let cfg = GithubConfig {
            username: Some("user1".to_string()),
            token: None,
            token_env: None,
        };

        let monitor = GithubMonitor::spawn(cfg, rx);
        assert_eq!(monitor.snapshot().username, "user1");

        let updated_cfg = GithubConfig {
            username: Some("user2".to_string()),
            token: None,
            token_env: None,
        };
        monitor.update_config(updated_cfg);
        // Direct read from config arc reflects changed config
        assert_eq!(monitor.config.read().unwrap().username.as_deref(), Some("user2"));
    }
}
