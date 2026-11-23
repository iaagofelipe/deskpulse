use crate::monitors::antigravity::AgentSessionSummary;
use chrono::Datelike;
use serde::Serialize;
use std::collections::HashMap;

const ONE_HOUR_MS: u64 = 3600 * 1000;
const FIVE_HOURS_MS: u64 = 5 * 3600 * 1000;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct AiWindowMetric {
    pub label: String,
    pub tokens: u64,
    pub budget_tokens: Option<u64>,
    pub pct: Option<f32>,
    pub reset_label: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct AiTodayStats {
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub messages: u64,
    pub last_hour_tokens: u64,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct AiDayBucket {
    pub date: String,
    pub label: String,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct AiModelUsage {
    pub model: String,
    pub display_name: String,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct AiSnapshot {
    pub provider: String,
    pub provider_label: String,
    pub window_5h: AiWindowMetric,
    pub weekly: AiWindowMetric,
    pub today: AiTodayStats,
    pub last_7_days: Vec<AiDayBucket>,
    pub models_today: Vec<AiModelUsage>,
    pub models_7d: Vec<AiModelUsage>,
    pub sessions: Vec<AgentSessionSummary>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct UsageRecord {
    pub created_at_ms: u64,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub is_message: bool,
}

#[allow(dead_code)]
pub fn humanize_model_name(raw: &str) -> String {
    let lower = raw.to_lowercase();
    let trimmed = lower.trim();
    if trimmed.is_empty() {
        return "Desconhecido".to_string();
    }

    if trimmed.contains("neon") {
        if trimmed.contains("xhigh") || trimmed.contains("x-high") {
            "Neon XHigh".to_string()
        } else if trimmed.contains("high") {
            "Neon High".to_string()
        } else if trimmed.contains("low") {
            "Neon Low".to_string()
        } else {
            "Neon".to_string()
        }
    } else if trimmed.contains("hydrogen") {
        if trimmed.contains("xhigh") || trimmed.contains("x-high") {
            "Hydrogen XHigh".to_string()
        } else if trimmed.contains("high") {
            "Hydrogen High".to_string()
        } else if trimmed.contains("low") {
            "Hydrogen Low".to_string()
        } else {
            "Hydrogen".to_string()
        }
    } else if trimmed.contains("flash") {
        if trimmed.contains("lite") {
            "Flash Lite".to_string()
        } else {
            "Flash".to_string()
        }
    } else if trimmed.contains("opus") {
        "Opus".to_string()
    } else if trimmed.contains("sonnet") {
        "Sonnet".to_string()
    } else if trimmed.contains("haiku") {
        "Haiku".to_string()
    } else if trimmed.contains("pro") {
        if trimmed.contains("high") {
            "Pro High".to_string()
        } else {
            "Pro".to_string()
        }
    } else {
        // Fallback: title-case words separated by -, _, or whitespace
        let words: Vec<String> = raw
            .split(|c: char| c == '-' || c == '_' || c.is_whitespace())
            .filter(|w| !w.is_empty())
            .map(|w| {
                let mut chars = w.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            })
            .collect();
        if words.is_empty() {
            raw.to_string()
        } else {
            words.join(" ")
        }
    }
}

fn weekday_ptbr(wd: chrono::Weekday) -> &'static str {
    match wd {
        chrono::Weekday::Sun => "Dom",
        chrono::Weekday::Mon => "Seg",
        chrono::Weekday::Tue => "Ter",
        chrono::Weekday::Wed => "Qua",
        chrono::Weekday::Thu => "Qui",
        chrono::Weekday::Fri => "Sex",
        chrono::Weekday::Sat => "Sáb",
    }
}

#[allow(dead_code)]
pub fn aggregate_usage(
    provider: &str,
    provider_label: &str,
    records: &[UsageRecord],
    sessions: Vec<AgentSessionSummary>,
    now_ms: u64,
    budget_5h: Option<u64>,
    budget_weekly: Option<u64>,
) -> AiSnapshot {
    let now_local = chrono::DateTime::from_timestamp_millis(now_ms as i64)
        .map(|dt| dt.with_timezone(&chrono::Local));
    let today_date = now_local
        .map(|dt| dt.date_naive())
        .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());

    // Generate 7 day buckets ending today
    let mut last_7_days = Vec::with_capacity(7);
    for i in (0..7).rev() {
        let d = today_date - chrono::Duration::days(i);
        let label = if i == 0 {
            "Hoje".to_string()
        } else {
            weekday_ptbr(d.weekday()).to_string()
        };
        last_7_days.push(AiDayBucket {
            date: d.format("%Y-%m-%d").to_string(),
            label,
            total_tokens: 0,
        });
    }

    let start_7d_date = today_date - chrono::Duration::days(6);

    let mut window_5h_tokens: u64 = 0;
    let mut last_hour_tokens: u64 = 0;

    let mut today_total: u64 = 0;
    let mut today_input: u64 = 0;
    let mut today_output: u64 = 0;
    let mut today_cache: u64 = 0;
    let mut today_messages: u64 = 0;

    let mut models_today_map: HashMap<String, u64> = HashMap::new();
    let mut models_7d_map: HashMap<String, u64> = HashMap::new();

    let window_5h_start = now_ms.saturating_sub(FIVE_HOURS_MS);
    let last_hour_start = now_ms.saturating_sub(ONE_HOUR_MS);

    for rec in records {
        let rec_total = rec
            .input_tokens
            .saturating_add(rec.output_tokens)
            .saturating_add(rec.cache_read_tokens);

        if rec.created_at_ms >= window_5h_start && rec.created_at_ms <= now_ms {
            window_5h_tokens = window_5h_tokens.saturating_add(rec_total);
        }

        if rec.created_at_ms >= last_hour_start && rec.created_at_ms <= now_ms {
            last_hour_tokens = last_hour_tokens.saturating_add(rec_total);
        }

        let rec_date = chrono::DateTime::from_timestamp_millis(rec.created_at_ms as i64)
            .map(|dt| dt.with_timezone(&chrono::Local).date_naive());

        if let Some(rd) = rec_date {
            if rd == today_date {
                today_total = today_total.saturating_add(rec_total);
                today_input = today_input.saturating_add(rec.input_tokens);
                today_output = today_output.saturating_add(rec.output_tokens);
                today_cache = today_cache.saturating_add(rec.cache_read_tokens);
                if rec.is_message {
                    today_messages += 1;
                }
                if !rec.model.is_empty() && rec_total > 0 {
                    *models_today_map.entry(rec.model.clone()).or_insert(0) += rec_total;
                }
            }

            if rd >= start_7d_date && rd <= today_date {
                let day_idx = (rd - start_7d_date).num_days() as usize;
                if day_idx < last_7_days.len() {
                    last_7_days[day_idx].total_tokens = last_7_days[day_idx]
                        .total_tokens
                        .saturating_add(rec_total);
                }
                if !rec.model.is_empty() && rec_total > 0 {
                    *models_7d_map.entry(rec.model.clone()).or_insert(0) += rec_total;
                }
            }
        }
    }

    let weekly_tokens: u64 = last_7_days.iter().map(|b| b.total_tokens).sum();

    let calc_pct = |tokens: u64, budget: Option<u64>| -> Option<f32> {
        budget
            .filter(|&b| b > 0)
            .map(|b| (tokens as f32 / b as f32 * 100.0).min(100.0))
    };

    let window_5h_pct = calc_pct(window_5h_tokens, budget_5h);
    let weekly_pct = calc_pct(weekly_tokens, budget_weekly);

    let format_top_models = |map: HashMap<String, u64>| -> Vec<AiModelUsage> {
        let mut list: Vec<AiModelUsage> = map
            .into_iter()
            .map(|(model, total_tokens)| AiModelUsage {
                display_name: humanize_model_name(&model),
                model,
                total_tokens,
            })
            .collect();
        list.sort_by(|a, b| {
            b.total_tokens
                .cmp(&a.total_tokens)
                .then_with(|| a.model.cmp(&b.model))
        });
        list.truncate(5);
        list
    };

    let models_today = format_top_models(models_today_map);
    let models_7d = format_top_models(models_7d_map);

    AiSnapshot {
        provider: provider.to_string(),
        provider_label: provider_label.to_string(),
        window_5h: AiWindowMetric {
            label: "Janela de 5h".to_string(),
            tokens: window_5h_tokens,
            budget_tokens: budget_5h,
            pct: window_5h_pct,
            reset_label: None,
        },
        weekly: AiWindowMetric {
            label: "Semanal (7 dias)".to_string(),
            tokens: weekly_tokens,
            budget_tokens: budget_weekly,
            pct: weekly_pct,
            reset_label: None,
        },
        today: AiTodayStats {
            total_tokens: today_total,
            input_tokens: today_input,
            output_tokens: today_output,
            cache_read_tokens: today_cache,
            messages: today_messages,
            last_hour_tokens,
        },
        last_7_days,
        models_today,
        models_7d,
        sessions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};

    #[test]
    fn test_humanize_model_name_formats_antigravity_and_claude_models() {
        assert_eq!(humanize_model_name("gemini-4-neon-high"), "Neon High");
        assert_eq!(humanize_model_name("gemini-4-hydrogen-high"), "Hydrogen High");
        assert_eq!(humanize_model_name("gemini-3.8-flash-medium"), "Flash");
        assert_eq!(humanize_model_name("claude-opus-4-6"), "Opus");
        assert_eq!(humanize_model_name("claude-sonnet-4-5"), "Sonnet");
        assert_eq!(humanize_model_name("claude-haiku-4-5"), "Haiku");
        assert_eq!(humanize_model_name("custom-model-v2"), "Custom Model V2");
    }

    #[test]
    fn test_aggregate_usage_computes_today_last_hour_5h_weekly_7days_and_models() {
        // Fixed local datetime: 2026-11-23 at 15:00:00 local time
        let now_dt = Local.with_ymd_and_hms(2026, 11, 23, 15, 0, 0).single().unwrap();
        let now_ms = now_dt.timestamp_millis() as u64;

        let records = vec![
            // 30m ago: in last hour, in 5h, today, in 7d
            UsageRecord {
                created_at_ms: now_ms - 30 * 60 * 1000,
                model: "gemini-4-neon-high".to_string(),
                input_tokens: 1000,
                output_tokens: 200,
                cache_read_tokens: 300,
                is_message: true,
            },
            // 2h ago: outside last hour, in 5h, today, in 7d
            UsageRecord {
                created_at_ms: now_ms - 2 * 3600 * 1000,
                model: "gemini-4-hydrogen-high".to_string(),
                input_tokens: 2000,
                output_tokens: 400,
                cache_read_tokens: 600,
                is_message: true,
            },
            // 6h ago: outside 5h, today, in 7d
            UsageRecord {
                created_at_ms: now_ms - 6 * 3600 * 1000,
                model: "gemini-3.8-flash-medium".to_string(),
                input_tokens: 500,
                output_tokens: 100,
                cache_read_tokens: 0,
                is_message: false,
            },
            // 2d ago: outside today, in 7d
            UsageRecord {
                created_at_ms: now_ms - 2 * 24 * 3600 * 1000,
                model: "claude-sonnet-4-5".to_string(),
                input_tokens: 4000,
                output_tokens: 1000,
                cache_read_tokens: 0,
                is_message: true,
            },
            // 8d ago: outside 7d
            UsageRecord {
                created_at_ms: now_ms - 8 * 24 * 3600 * 1000,
                model: "claude-opus-4-6".to_string(),
                input_tokens: 10000,
                output_tokens: 2000,
                cache_read_tokens: 0,
                is_message: true,
            },
        ];

        let sessions = vec![AgentSessionSummary {
            id: "sess_1".to_string(),
            title: "Task 4".to_string(),
            status: "running".to_string(),
            current_step: "Working".to_string(),
            current_tool: None,
            current_detail: None,
            thinking_title: None,
            last_prompt: None,
            step_index: Some(1),
            context_tokens: Some(5000),
            tasks: Vec::new(),
            recent_actions: Vec::new(),
            task_done: 0,
            task_total: 1,
            updated_at_ms: now_ms,
        }];

        let budget_5h = Some(10_000);
        let budget_weekly = Some(50_000);

        let snap = aggregate_usage(
            "antigravity",
            "Antigravity",
            &records,
            sessions.clone(),
            now_ms,
            budget_5h,
            budget_weekly,
        );

        assert_eq!(snap.provider, "antigravity");
        assert_eq!(snap.provider_label, "Antigravity");
        assert_eq!(snap.sessions, sessions);

        // window_5h: 1500 (30m) + 3000 (2h) = 4500
        assert_eq!(snap.window_5h.tokens, 4500);
        assert_eq!(snap.window_5h.budget_tokens, Some(10_000));
        assert_eq!(snap.window_5h.pct, Some(45.0));

        // weekly: 1500 + 3000 + 600 + 5000 = 10100
        assert_eq!(snap.weekly.tokens, 10100);
        assert_eq!(snap.weekly.budget_tokens, Some(50_000));
        assert_eq!(snap.weekly.pct, Some(20.2));

        // today: 1500 + 3000 + 600 = 5100
        assert_eq!(snap.today.total_tokens, 5100);
        assert_eq!(snap.today.input_tokens, 3500);
        assert_eq!(snap.today.output_tokens, 700);
        assert_eq!(snap.today.cache_read_tokens, 900);
        assert_eq!(snap.today.messages, 2);
        assert_eq!(snap.today.last_hour_tokens, 1500);

        // 7 days ending today
        assert_eq!(snap.last_7_days.len(), 7);
        let today_bucket = snap.last_7_days.last().unwrap();
        assert_eq!(today_bucket.date, "2026-11-23");
        assert_eq!(today_bucket.label, "Hoje");
        assert_eq!(today_bucket.total_tokens, 5100);

        // 2d ago bucket (index 4 out of 0..6)
        let two_days_ago_bucket = &snap.last_7_days[4];
        assert_eq!(two_days_ago_bucket.date, "2026-11-21");
        assert_eq!(two_days_ago_bucket.total_tokens, 5000);

        // models_today
        assert_eq!(snap.models_today.len(), 3);
        assert_eq!(snap.models_today[0].model, "gemini-4-hydrogen-high");
        assert_eq!(snap.models_today[0].display_name, "Hydrogen High");
        assert_eq!(snap.models_today[0].total_tokens, 3000);
        assert_eq!(snap.models_today[1].model, "gemini-4-neon-high");
        assert_eq!(snap.models_today[1].display_name, "Neon High");
        assert_eq!(snap.models_today[1].total_tokens, 1500);
        assert_eq!(snap.models_today[2].model, "gemini-3.8-flash-medium");
        assert_eq!(snap.models_today[2].display_name, "Flash");
        assert_eq!(snap.models_today[2].total_tokens, 600);

        // models_7d
        assert_eq!(snap.models_7d.len(), 4);
        assert_eq!(snap.models_7d[0].model, "claude-sonnet-4-5");
        assert_eq!(snap.models_7d[0].display_name, "Sonnet");
        assert_eq!(snap.models_7d[0].total_tokens, 5000);
        assert_eq!(snap.models_7d[1].model, "gemini-4-hydrogen-high");
        assert_eq!(snap.models_7d[1].total_tokens, 3000);
        assert_eq!(snap.models_7d[2].model, "gemini-4-neon-high");
        assert_eq!(snap.models_7d[2].total_tokens, 1500);
        assert_eq!(snap.models_7d[3].model, "gemini-3.8-flash-medium");
        assert_eq!(snap.models_7d[3].total_tokens, 600);
    }
}
