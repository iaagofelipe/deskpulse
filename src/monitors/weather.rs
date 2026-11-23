use crate::config::WeatherConfig;
use chrono::Datelike;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{watch, Notify};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WeatherHour {
    pub time_label: String,
    pub temp: f32,
    pub weather_code: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WeatherDay {
    pub day_label: String,
    pub high: f32,
    pub low: f32,
    pub precip_pct: u8,
    pub weather_code: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WeatherSnapshot {
    pub configured: bool,
    pub city: String,
    pub unit: String,
    pub temp: Option<f32>,
    pub feels_like: Option<f32>,
    pub humidity_pct: Option<u8>,
    pub wind_speed: Option<f32>,
    pub weather_code: Option<u16>,
    pub condition: String,
    pub today_high: Option<f32>,
    pub today_low: Option<f32>,
    pub hourly: Vec<WeatherHour>,
    pub daily: Vec<WeatherDay>,
    pub updated_at_ms: Option<u64>,
    pub error: Option<String>,
}

impl Default for WeatherSnapshot {
    fn default() -> Self {
        Self {
            configured: false,
            city: String::new(),
            unit: "c".to_string(),
            temp: None,
            feels_like: None,
            humidity_pct: None,
            wind_speed: None,
            weather_code: None,
            condition: String::new(),
            today_high: None,
            today_low: None,
            hourly: Vec::new(),
            daily: Vec::new(),
            updated_at_ms: None,
            error: None,
        }
    }
}

#[allow(dead_code)]
pub struct WeatherMonitor {
    snapshot: Arc<RwLock<WeatherSnapshot>>,
    config: Arc<RwLock<WeatherConfig>>,
    notify: Arc<Notify>,
}

impl WeatherMonitor {
    #[allow(dead_code)]
    pub fn spawn(initial: WeatherConfig, shutdown: watch::Receiver<bool>) -> Self {
        let is_configured = !initial.city.trim().is_empty()
            || initial.latitude.is_some()
            || initial.longitude.is_some();
        let snap = WeatherSnapshot {
            configured: is_configured,
            city: initial.city.clone(),
            unit: initial.unit.clone(),
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
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default();
            let mut geo_cache: HashMap<String, (f64, f64)> = HashMap::new();
            let poll_interval = Duration::from_secs(15 * 60);

            loop {
                if *shutdown_bg.borrow() {
                    break;
                }

                let current_cfg = config_bg.read().unwrap().clone();
                let has_coords = current_cfg.latitude.is_some() && current_cfg.longitude.is_some();
                let has_city = !current_cfg.city.trim().is_empty();

                if !has_coords && !has_city {
                    if let Ok(mut snap) = snapshot_bg.write() {
                        *snap = WeatherSnapshot {
                            configured: false,
                            city: current_cfg.city.clone(),
                            unit: current_cfg.unit.clone(),
                            ..Default::default()
                        };
                    }
                } else {
                    let coords = if let (Some(lat), Some(lon)) =
                        (current_cfg.latitude, current_cfg.longitude)
                    {
                        Some((lat, lon))
                    } else {
                        let cache_key = current_cfg.city.trim().to_lowercase();
                        if let Some(&cached) = geo_cache.get(&cache_key) {
                            Some(cached)
                        } else {
                            match client
                                .get("https://geocoding-api.open-meteo.com/v1/search")
                                .query(&[
                                    ("name", current_cfg.city.trim()),
                                    ("count", "1"),
                                    ("language", "pt"),
                                ])
                                .send()
                                .await
                            {
                                Ok(resp) => {
                                    if let Ok(json) = resp.json::<serde_json::Value>().await {
                                        if let Some(res) = json
                                            .get("results")
                                            .and_then(|r| r.as_array())
                                            .and_then(|a| a.first())
                                        {
                                            let lat = res.get("latitude").and_then(|v| v.as_f64());
                                            let lon = res.get("longitude").and_then(|v| v.as_f64());
                                            if let (Some(lat), Some(lon)) = (lat, lon) {
                                                geo_cache.insert(cache_key, (lat, lon));
                                                Some((lat, lon))
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                }
                                Err(_) => None,
                            }
                        }
                    };

                    match coords {
                        Some((lat, lon)) => {
                            let is_fahrenheit = current_cfg.unit.eq_ignore_ascii_case("f")
                                || current_cfg.unit.eq_ignore_ascii_case("fahrenheit");
                            let (temp_unit, wind_unit) = if is_fahrenheit {
                                ("fahrenheit", "mph")
                            } else {
                                ("celsius", "kmh")
                            };

                            let lat_str = lat.to_string();
                            let lon_str = lon.to_string();
                            let query = [
                                ("latitude", lat_str.as_str()),
                                ("longitude", lon_str.as_str()),
                                (
                                    "current",
                                    "temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,wind_speed_10m",
                                ),
                                ("hourly", "temperature_2m,weather_code"),
                                (
                                    "daily",
                                    "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max",
                                ),
                                ("timezone", "auto"),
                                ("temperature_unit", temp_unit),
                                ("wind_speed_unit", wind_unit),
                            ];

                            match client
                                .get("https://api.open-meteo.com/v1/forecast")
                                .query(&query)
                                .send()
                                .await
                            {
                                Ok(resp) => match resp.json::<serde_json::Value>().await {
                                    Ok(json) => {
                                        let now_iso = chrono::Local::now()
                                            .format("%Y-%m-%dT%H:%M:%S")
                                            .to_string();
                                        match parse_open_meteo_response(
                                            &current_cfg.city,
                                            &current_cfg.unit,
                                            &json,
                                            &now_iso,
                                        ) {
                                            Ok(mut snap) => {
                                                snap.updated_at_ms = Some(
                                                    std::time::SystemTime::now()
                                                        .duration_since(std::time::UNIX_EPOCH)
                                                        .unwrap_or_default()
                                                        .as_millis()
                                                        as u64,
                                                );
                                                if let Ok(mut cur) = snapshot_bg.write() {
                                                    *cur = snap;
                                                }
                                            }
                                            Err(err) => {
                                                if let Ok(mut cur) = snapshot_bg.write() {
                                                    cur.error = Some(err);
                                                }
                                            }
                                        }
                                    }
                                    Err(err) => {
                                        if let Ok(mut cur) = snapshot_bg.write() {
                                            cur.error =
                                                Some(format!("Falha ao decodificar previsão: {err}"));
                                        }
                                    }
                                },
                                Err(err) => {
                                    if let Ok(mut cur) = snapshot_bg.write() {
                                        cur.error = Some(format!(
                                            "Falha ao conectar com Open-Meteo: {err}"
                                        ));
                                    }
                                }
                            }
                        }
                        None => {
                            if let Ok(mut cur) = snapshot_bg.write() {
                                cur.configured = true;
                                cur.city = current_cfg.city.clone();
                                cur.unit = current_cfg.unit.clone();
                                cur.error = Some("Cidade não encontrada".to_string());
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

    #[allow(dead_code)]
    pub fn snapshot(&self) -> WeatherSnapshot {
        self.snapshot.read().unwrap().clone()
    }

    #[allow(dead_code)]
    pub fn update_config(&self, cfg: WeatherConfig) {
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

#[allow(dead_code)]
pub fn wmo_condition_pt(code: u16) -> &'static str {
    match code {
        0 => "Céu limpo",
        1 => "Predominantemente limpo",
        2 => "Parcialmente nublado",
        3 => "Nublado",
        45 => "Nevoeiro",
        48 => "Nevoeiro com geada",
        51 => "Garoa leve",
        53 => "Garoa moderada",
        55 => "Garoa densa",
        56 => "Garoa congelante leve",
        57 => "Garoa congelante densa",
        61 => "Chuva leve",
        63 => "Chuva moderada",
        65 => "Chuva forte",
        66 => "Chuva congelante leve",
        67 => "Chuva congelante forte",
        71 => "Neve leve",
        73 => "Neve moderada",
        75 => "Neve forte",
        77 => "Grãos de neve",
        80 => "Pancadas de chuva leves",
        81 => "Pancadas de chuva moderadas",
        82 => "Pancadas de chuva violentas",
        85 => "Pancadas de neve leves",
        86 => "Pancadas de neve fortes",
        95 => "Tempestade",
        96 => "Tempestade com granizo leve",
        99 => "Tempestade com granizo forte",
        _ => "Desconhecido",
    }
}

fn format_time_label(iso_time: &str) -> String {
    if let Some((_, time_part)) = iso_time.split_once('T') {
        let parts: Vec<&str> = time_part.split(':').collect();
        if !parts.is_empty() {
            return format!("{}h", parts[0]);
        }
    }
    iso_time.to_string()
}

fn weekday_to_pt(weekday: chrono::Weekday) -> &'static str {
    match weekday {
        chrono::Weekday::Mon => "Seg",
        chrono::Weekday::Tue => "Ter",
        chrono::Weekday::Wed => "Qua",
        chrono::Weekday::Thu => "Qui",
        chrono::Weekday::Fri => "Sex",
        chrono::Weekday::Sat => "Sáb",
        chrono::Weekday::Sun => "Dom",
    }
}

#[allow(dead_code)]
pub fn parse_open_meteo_response(
    city: &str,
    unit: &str,
    json: &serde_json::Value,
    now_local_iso: &str,
) -> Result<WeatherSnapshot, String> {
    if !json.is_object() {
        return Err("Resposta Open-Meteo não é um objeto JSON".to_string());
    }

    if let Some(err) = json
        .get("reason")
        .or_else(|| json.get("error"))
        .and_then(|v| v.as_str())
    {
        return Err(format!("Erro da API Open-Meteo: {err}"));
    }

    let current = json.get("current").or_else(|| json.get("current_weather"));
    let temp = current
        .and_then(|c| c.get("temperature_2m").or_else(|| c.get("temperature")))
        .and_then(|v| v.as_f64())
        .map(|f| f as f32);
    let feels_like = current
        .and_then(|c| c.get("apparent_temperature"))
        .and_then(|v| v.as_f64())
        .map(|f| f as f32);
    let humidity_pct = current
        .and_then(|c| c.get("relative_humidity_2m"))
        .and_then(|v| v.as_f64())
        .map(|f| f.clamp(0.0, 100.0) as u8);
    let wind_speed = current
        .and_then(|c| {
            c.get("wind_speed_10m")
                .or_else(|| c.get("wind_speed"))
                .or_else(|| c.get("windspeed"))
        })
        .and_then(|v| v.as_f64())
        .map(|f| f as f32);
    let weather_code = current
        .and_then(|c| c.get("weather_code").or_else(|| c.get("weathercode")))
        .and_then(|v| v.as_u64())
        .map(|u| u as u16);
    let condition = weather_code.map(wmo_condition_pt).unwrap_or("").to_string();

    let mut daily = Vec::new();
    let mut today_high = None;
    let mut today_low = None;

    if let Some(daily_obj) = json.get("daily") {
        let times = daily_obj.get("time").and_then(|v| v.as_array());
        let maxs = daily_obj
            .get("temperature_2m_max")
            .and_then(|v| v.as_array());
        let mins = daily_obj
            .get("temperature_2m_min")
            .and_then(|v| v.as_array());
        let codes = daily_obj
            .get("weather_code")
            .or_else(|| daily_obj.get("weathercode"))
            .and_then(|v| v.as_array());
        let precips = daily_obj
            .get("precipitation_probability_max")
            .and_then(|v| v.as_array());

        if let Some(times_arr) = times {
            let limit = times_arr.len().min(6);
            for i in 0..limit {
                let time_str = times_arr[i].as_str().unwrap_or_default();
                let high = maxs
                    .and_then(|a| a.get(i))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as f32;
                let low = mins
                    .and_then(|a| a.get(i))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as f32;
                let precip_pct = precips
                    .and_then(|a| a.get(i))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
                    .clamp(0.0, 100.0) as u8;
                let day_weather_code = codes
                    .and_then(|a| a.get(i))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u16;

                if i == 0 {
                    today_high = Some(high);
                    today_low = Some(low);
                }

                let day_label = if i == 0 {
                    "Hoje".to_string()
                } else if let Ok(date) = chrono::NaiveDate::parse_from_str(time_str, "%Y-%m-%d") {
                    weekday_to_pt(date.weekday()).to_string()
                } else {
                    time_str.to_string()
                };

                daily.push(WeatherDay {
                    day_label,
                    high,
                    low,
                    precip_pct,
                    weather_code: day_weather_code,
                });
            }
        }
    }

    let mut hourly = Vec::new();
    if let Some(hourly_obj) = json.get("hourly") {
        let times = hourly_obj.get("time").and_then(|v| v.as_array());
        let temps = hourly_obj
            .get("temperature_2m")
            .or_else(|| hourly_obj.get("temperature"))
            .and_then(|v| v.as_array());
        let codes = hourly_obj
            .get("weather_code")
            .or_else(|| hourly_obj.get("weathercode"))
            .and_then(|v| v.as_array());

        if let (Some(times_arr), Some(temps_arr), Some(codes_arr)) = (times, temps, codes) {
            let start_prefix = if now_local_iso.len() >= 13 {
                &now_local_iso[..13]
            } else {
                now_local_iso
            };

            let start_idx = times_arr
                .iter()
                .position(|t| {
                    t.as_str()
                        .map(|s| s.starts_with(start_prefix))
                        .unwrap_or(false)
                })
                .or_else(|| {
                    times_arr
                        .iter()
                        .position(|t| t.as_str().map(|s| s >= start_prefix).unwrap_or(false))
                })
                .unwrap_or(0);

            let end_idx = (start_idx + 24).min(times_arr.len());
            for i in start_idx..end_idx {
                let time_str = times_arr[i].as_str().unwrap_or_default();
                let hour_temp = temps_arr
                    .get(i)
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as f32;
                let hour_code = codes_arr
                    .get(i)
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u16;

                hourly.push(WeatherHour {
                    time_label: format_time_label(time_str),
                    temp: hour_temp,
                    weather_code: hour_code,
                });
            }
        }
    }

    Ok(WeatherSnapshot {
        configured: true,
        city: city.to_string(),
        unit: unit.to_string(),
        temp,
        feels_like,
        humidity_pct,
        wind_speed,
        weather_code,
        condition,
        today_high,
        today_low,
        hourly,
        daily,
        updated_at_ms: None,
        error: None,
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;

    fn generate_open_meteo_fixture() -> serde_json::Value {
        let mut hourly_times = Vec::new();
        let mut hourly_temps = Vec::new();
        let mut hourly_codes = Vec::new();

        // 48 hours starting from 2026-11-23T00:00 to 2026-11-24T23:00
        for day in 23..=24 {
            for hour in 0..24 {
                hourly_times.push(format!("2026-11-{}T{:02}:00", day, hour));
                hourly_temps.push(18.0 + (hour as f64) * 0.5);
                hourly_codes.push(if hour >= 12 { 2 } else { 0 });
            }
        }

        serde_json::json!({
            "latitude": -23.55,
            "longitude": -46.63,
            "generationtime_ms": 0.5,
            "utc_offset_seconds": -10800,
            "timezone": "America/Sao_Paulo",
            "timezone_abbreviation": "-03",
            "elevation": 760.0,
            "current": {
                "time": "2026-11-23T10:00",
                "temperature_2m": 24.5,
                "relative_humidity_2m": 68,
                "apparent_temperature": 25.2,
                "weather_code": 2,
                "wind_speed_10m": 12.3
            },
            "hourly": {
                "time": hourly_times,
                "temperature_2m": hourly_temps,
                "weather_code": hourly_codes
            },
            "daily": {
                "time": [
                    "2026-11-23",
                    "2026-11-24",
                    "2026-11-25",
                    "2026-11-26",
                    "2026-11-27",
                    "2026-11-28",
                    "2026-11-29"
                ],
                "weather_code": [2, 3, 61, 0, 1, 95, 2],
                "temperature_2m_max": [27.0, 26.2, 23.0, 28.5, 30.1, 25.0, 24.0],
                "temperature_2m_min": [17.5, 18.0, 16.2, 17.0, 19.2, 18.5, 16.0],
                "precipitation_probability_max": [15, 30, 85, 5, 10, 90, 20]
            }
        })
    }

    #[test]
    fn test_parse_open_meteo_response_extracts_current_next_24h_and_6_days() {
        let fixture = generate_open_meteo_fixture();
        let now_local = "2026-11-23T10:15";

        let snap = parse_open_meteo_response("São Paulo", "c", &fixture, now_local)
            .expect("parse should succeed");

        assert!(snap.configured);
        assert_eq!(snap.city, "São Paulo");
        assert_eq!(snap.unit, "c");
        assert_eq!(snap.temp, Some(24.5));
        assert_eq!(snap.feels_like, Some(25.2));
        assert_eq!(snap.humidity_pct, Some(68));
        assert_eq!(snap.wind_speed, Some(12.3));
        assert_eq!(snap.weather_code, Some(2));
        assert_eq!(snap.condition, "Parcialmente nublado");
        assert_eq!(snap.today_high, Some(27.0));
        assert_eq!(snap.today_low, Some(17.5));
        assert_eq!(snap.error, None);

        // Hourly: 24 entries starting at current hour (10:00 -> "10h")
        assert_eq!(snap.hourly.len(), 24);
        assert_eq!(snap.hourly[0].time_label, "10h");
        assert_eq!(snap.hourly[0].temp, 23.0); // 18.0 + 10 * 0.5
        assert_eq!(snap.hourly[0].weather_code, 0);

        // Daily: 6 entries, first is "Hoje"
        assert_eq!(snap.daily.len(), 6);
        assert_eq!(snap.daily[0].day_label, "Hoje");
        assert_eq!(snap.daily[0].high, 27.0);
        assert_eq!(snap.daily[0].low, 17.5);
        assert_eq!(snap.daily[0].precip_pct, 15);
        assert_eq!(snap.daily[0].weather_code, 2);

        // 2026-11-24 is Tuesday -> "Ter"
        assert_eq!(snap.daily[1].day_label, "Ter");
        assert_eq!(snap.daily[1].high, 26.2);
        assert_eq!(snap.daily[1].low, 18.0);
        assert_eq!(snap.daily[1].precip_pct, 30);
        assert_eq!(snap.daily[1].weather_code, 3);

        // 2026-11-25 is Wednesday -> "Qua"
        assert_eq!(snap.daily[2].day_label, "Qua");
        // 2026-11-26 is Thursday -> "Qui"
        assert_eq!(snap.daily[3].day_label, "Qui");
        // 2026-11-27 is Friday -> "Sex"
        assert_eq!(snap.daily[4].day_label, "Sex");
        // 2026-11-28 is Saturday -> "Sáb"
        assert_eq!(snap.daily[5].day_label, "Sáb");
    }

    #[tokio::test]
    async fn test_unconfigured_weather_returns_clean_empty_snapshot() {
        let cfg = WeatherConfig {
            city: String::new(),
            latitude: None,
            longitude: None,
            unit: "c".to_string(),
        };
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let monitor = WeatherMonitor::spawn(cfg, shutdown_rx);

        let snap = monitor.snapshot();
        assert!(!snap.configured);
        assert_eq!(snap.error, None);
        assert_eq!(snap.temp, None);
        assert_eq!(snap.feels_like, None);
        assert_eq!(snap.humidity_pct, None);
        assert_eq!(snap.wind_speed, None);
        assert_eq!(snap.weather_code, None);
        assert_eq!(snap.condition, "");
        assert_eq!(snap.today_high, None);
        assert_eq!(snap.today_low, None);
        assert!(snap.hourly.is_empty());
        assert!(snap.daily.is_empty());

        let _ = shutdown_tx.send(true);
    }

    #[test]
    fn test_wmo_condition_pt_mappings() {
        assert_eq!(wmo_condition_pt(0), "Céu limpo");
        assert_eq!(wmo_condition_pt(1), "Predominantemente limpo");
        assert_eq!(wmo_condition_pt(2), "Parcialmente nublado");
        assert_eq!(wmo_condition_pt(3), "Nublado");
        assert_eq!(wmo_condition_pt(45), "Nevoeiro");
        assert_eq!(wmo_condition_pt(51), "Garoa leve");
        assert_eq!(wmo_condition_pt(61), "Chuva leve");
        assert_eq!(wmo_condition_pt(65), "Chuva forte");
        assert_eq!(wmo_condition_pt(71), "Neve leve");
        assert_eq!(wmo_condition_pt(80), "Pancadas de chuva leves");
        assert_eq!(wmo_condition_pt(95), "Tempestade");
        assert_eq!(wmo_condition_pt(99), "Tempestade com granizo forte");
        assert_eq!(wmo_condition_pt(999), "Desconhecido");
    }

    #[tokio::test]
    async fn test_weather_monitor_update_config() {
        let initial = WeatherConfig {
            city: "Curitiba".to_string(),
            latitude: Some(-25.42),
            longitude: Some(-49.27),
            unit: "c".to_string(),
        };
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let monitor = WeatherMonitor::spawn(initial.clone(), shutdown_rx);

        let snap = monitor.snapshot();
        assert!(snap.configured);
        assert_eq!(snap.city, "Curitiba");

        let mut updated = initial.clone();
        updated.city = "Florianópolis".to_string();
        monitor.update_config(updated);

        assert_eq!(monitor.config.read().unwrap().city, "Florianópolis");
        let _ = shutdown_tx.send(true);
    }

    #[test]
    fn test_parse_open_meteo_response_invalid_json() {
        let invalid = serde_json::json!("not an object");
        let res = parse_open_meteo_response("Cidade", "c", &invalid, "2026-11-23T10:00");
        assert!(res.is_err());
    }
}
