use anyhow::{Context, Result, anyhow};
use dotenvy::dotenv;
use std::env;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub app_base_url: String,
    pub http_bind: String,
    pub smtp_bind: String,
    pub mail_domain: String,
    pub api_keys: Vec<String>,
    pub retention_days: i64,
    pub sqlite_path: String,
    pub daily_limit: i64,
    pub total_limit: i64,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let _ = dotenv();

        let app_base_url =
            env::var("APP_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".to_string());
        let http_bind = env::var("HTTP_BIND").unwrap_or_else(|_| "127.0.0.1:3000".to_string());
        let smtp_bind = env::var("SMTP_BIND").unwrap_or_else(|_| "127.0.0.1:2525".to_string());
        let mail_domain = required_env("MAIL_DOMAIN")?.to_ascii_lowercase();
        let api_keys = parse_csv_env("API_KEYS")?;
        let retention_days = env::var("RETENTION_DAYS")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(1)
            .clamp(1, 7);
        let sqlite_path =
            env::var("SQLITE_PATH").unwrap_or_else(|_| "./data/gptmail.sqlite3".to_string());
        let daily_limit = env::var("DAILY_LIMIT")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0)
            .max(0);
        let total_limit = env::var("TOTAL_LIMIT")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0)
            .max(0);

        Ok(Self {
            app_base_url,
            http_bind,
            smtp_bind,
            mail_domain,
            api_keys,
            retention_days,
            sqlite_path,
            daily_limit,
            total_limit,
        })
    }
}

fn required_env(name: &str) -> Result<String> {
    env::var(name).with_context(|| format!("missing required environment variable {name}"))
}

fn parse_csv_env(name: &str) -> Result<Vec<String>> {
    let raw = required_env(name)?;
    let values = raw
        .split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    if values.is_empty() {
        return Err(anyhow!("{name} must contain at least one API key"));
    }

    Ok(values)
}
