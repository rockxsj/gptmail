use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::models::{
    ClearInboxData, EmailDetailData, EmailSummary, EmailsListData, GeneratedEmailData, MessageData,
    SiteStatsData, UsageSnapshot,
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use mail_parser::MessageParser;
use rand::distributions::{Alphanumeric, DistString};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

#[derive(Clone)]
pub struct MailService {
    config: AppConfig,
    pool: SqlitePool,
}

impl MailService {
    pub fn new(config: AppConfig, pool: SqlitePool) -> Self {
        Self { config, pool }
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn is_allowed_recipient(&self, email: &str) -> bool {
        self.normalize_email(email).is_ok()
    }

    pub async fn authorize_and_track(
        &self,
        provided_key: Option<&str>,
    ) -> AppResult<UsageSnapshot> {
        let api_key = provided_key
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AppError::Unauthorized("Authentication required".to_string()))?;

        if !self
            .config
            .api_keys
            .iter()
            .any(|candidate| candidate == api_key)
        {
            return Err(AppError::Unauthorized(
                "Authentication required".to_string(),
            ));
        }

        let mut tx = self.pool.begin().await.map_err(anyhow::Error::from)?;
        let usage_day = Utc::now().date_naive().format("%Y-%m-%d").to_string();

        sqlx::query(
            "INSERT OR IGNORE INTO daily_usage (api_key, usage_day, request_count) VALUES (?, ?, 0)",
        )
        .bind(api_key)
        .bind(&usage_day)
        .execute(&mut *tx)
        .await
        .map_err(anyhow::Error::from)?;

        sqlx::query("INSERT OR IGNORE INTO total_usage (api_key, request_count) VALUES (?, 0)")
            .bind(api_key)
            .execute(&mut *tx)
            .await
            .map_err(anyhow::Error::from)?;

        let daily_used: i64 = sqlx::query_scalar(
            "SELECT request_count FROM daily_usage WHERE api_key = ? AND usage_day = ?",
        )
        .bind(api_key)
        .bind(&usage_day)
        .fetch_one(&mut *tx)
        .await
        .map_err(anyhow::Error::from)?;

        let total_used: i64 =
            sqlx::query_scalar("SELECT request_count FROM total_usage WHERE api_key = ?")
                .bind(api_key)
                .fetch_one(&mut *tx)
                .await
                .map_err(anyhow::Error::from)?;

        let usage_before = self.build_usage_snapshot(daily_used, total_used);

        if (self.config.daily_limit > 0 && daily_used >= self.config.daily_limit)
            || (self.config.total_limit > 0 && total_used >= self.config.total_limit)
        {
            return Err(AppError::RateLimited {
                message: "Daily quota exceeded".to_string(),
                usage: Some(usage_before),
            });
        }

        sqlx::query(
            "UPDATE daily_usage SET request_count = request_count + 1 WHERE api_key = ? AND usage_day = ?",
        )
        .bind(api_key)
        .bind(&usage_day)
        .execute(&mut *tx)
        .await
        .map_err(anyhow::Error::from)?;

        sqlx::query("UPDATE total_usage SET request_count = request_count + 1 WHERE api_key = ?")
            .bind(api_key)
            .execute(&mut *tx)
            .await
            .map_err(anyhow::Error::from)?;

        tx.commit().await.map_err(anyhow::Error::from)?;

        Ok(self.build_usage_snapshot(daily_used + 1, total_used + 1))
    }

    pub async fn generate_email(
        &self,
        prefix: Option<&str>,
        domain: Option<&str>,
    ) -> AppResult<GeneratedEmailData> {
        let domain = self.resolve_domain(domain)?;
        let local = match prefix.map(str::trim).filter(|value| !value.is_empty()) {
            Some(prefix) => self.validate_prefix(prefix)?.to_ascii_lowercase(),
            None => self.random_prefix(),
        };
        let email = format!("{local}@{domain}");
        self.ensure_inbox(&email).await?;
        Ok(GeneratedEmailData { email })
    }

    pub async fn list_messages(&self, email: &str) -> AppResult<EmailsListData> {
        let normalized = self.normalize_email(email)?;
        self.touch_inbox(&normalized).await?;

        let rows = sqlx::query(
            r#"
            SELECT id, email_address, from_address, subject, timestamp, created_at, has_html, raw_size
            FROM messages
            WHERE email_address = ?
            ORDER BY timestamp DESC, id DESC
            "#,
        )
        .bind(&normalized)
        .fetch_all(&self.pool)
        .await
        .map_err(anyhow::Error::from)?;

        let emails = rows
            .into_iter()
            .map(|row| EmailSummary {
                id: row.get::<String, _>("id"),
                email_address: row.get::<String, _>("email_address"),
                from_address: row.get::<String, _>("from_address"),
                subject: row.get::<String, _>("subject"),
                timestamp: row.get::<i64, _>("timestamp"),
                created_at: row.get::<String, _>("created_at"),
                has_html: row.get::<i64, _>("has_html") != 0,
                raw_size: row.get::<i64, _>("raw_size"),
            })
            .collect::<Vec<_>>();

        Ok(EmailsListData {
            count: emails.len(),
            emails,
        })
    }

    pub async fn get_message(&self, id: &str) -> AppResult<EmailDetailData> {
        let row = sqlx::query(
            r#"
            SELECT id, email_address, from_address, subject, text_content, html_content, has_html,
                   raw_headers, raw_mime, raw_size, created_at, timestamp
            FROM messages
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(anyhow::Error::from)?
        .ok_or_else(|| AppError::NotFound("Email not found".to_string()))?;

        Ok(EmailDetailData {
            id: row.get::<String, _>("id"),
            email_address: row.get::<String, _>("email_address"),
            from_address: row.get::<String, _>("from_address"),
            subject: row.get::<String, _>("subject"),
            content: row.get::<String, _>("text_content"),
            html_content: row.get::<String, _>("html_content"),
            has_html: row.get::<i64, _>("has_html") != 0,
            raw_headers: row.get::<String, _>("raw_headers"),
            raw_mime: row.get::<String, _>("raw_mime"),
            raw_size: row.get::<i64, _>("raw_size"),
            created_at: row.get::<String, _>("created_at"),
            timestamp: row.get::<i64, _>("timestamp"),
        })
    }

    pub async fn delete_message(&self, id: &str) -> AppResult<MessageData> {
        let result = sqlx::query("DELETE FROM messages WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(anyhow::Error::from)?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Email not found".to_string()));
        }

        Ok(MessageData {
            message: "Email deleted".to_string(),
        })
    }

    pub async fn clear_inbox(&self, email: &str) -> AppResult<ClearInboxData> {
        let normalized = self.normalize_email(email)?;
        let result = sqlx::query("DELETE FROM messages WHERE email_address = ?")
            .bind(&normalized)
            .execute(&self.pool)
            .await
            .map_err(anyhow::Error::from)?;

        let count = result.rows_affected();
        Ok(ClearInboxData {
            message: format!("Deleted {count} emails"),
            count,
        })
    }

    pub async fn stats(&self) -> AppResult<SiteStatsData> {
        let inboxes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inboxes")
            .fetch_one(&self.pool)
            .await
            .map_err(anyhow::Error::from)?;
        let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
            .fetch_one(&self.pool)
            .await
            .map_err(anyhow::Error::from)?;
        let start_of_day = start_of_today_utc();
        let messages_today: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE timestamp >= ?")
                .bind(start_of_day)
                .fetch_one(&self.pool)
                .await
                .map_err(anyhow::Error::from)?;
        let generated_today: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM inboxes WHERE created_timestamp >= ?")
                .bind(start_of_day)
                .fetch_one(&self.pool)
                .await
                .map_err(anyhow::Error::from)?;

        Ok(SiteStatsData {
            mail_domain: self.config.mail_domain.clone(),
            inboxes,
            messages,
            messages_today,
            generated_today,
            retention_days: self.config.retention_days,
        })
    }

    pub async fn ingest_message(
        &self,
        mail_from: Option<&str>,
        recipients: &[String],
        raw: &[u8],
    ) -> anyhow::Result<Vec<String>> {
        if recipients.is_empty() {
            return Ok(Vec::new());
        }

        let raw_mime = String::from_utf8_lossy(raw).to_string();
        let raw_headers = extract_header_block(&raw_mime);
        let message_id_header = extract_header_value(&raw_headers, "message-id");
        let parsed = MessageParser::default().parse(raw);
        let from_address = parsed
            .as_ref()
            .and_then(|message| message.from())
            .and_then(|address| address.first())
            .and_then(|addr| addr.address())
            .map(ToOwned::to_owned)
            .or_else(|| mail_from.map(ToOwned::to_owned))
            .unwrap_or_else(|| "unknown@localhost".to_string());
        let subject = parsed
            .as_ref()
            .and_then(|message| message.subject())
            .map(|value| value.to_string())
            .unwrap_or_else(|| "(no subject)".to_string());
        let text_content = parsed
            .as_ref()
            .and_then(|message| message.body_text(0))
            .map(|value| value.to_string())
            .unwrap_or_default();
        let html_content = parsed
            .as_ref()
            .and_then(|message| message.body_html(0))
            .map(|value| value.to_string())
            .unwrap_or_default();
        let has_html = !html_content.trim().is_empty();
        let now = Utc::now();
        let timestamp = now.timestamp();
        let created_at = now.to_rfc3339();
        let raw_size = raw.len() as i64;

        let mut inserted = Vec::new();

        for recipient in recipients {
            let normalized = match self.normalize_email(recipient) {
                Ok(value) => value,
                Err(_) => continue,
            };

            let inbox_id = self.ensure_inbox(&normalized).await?;
            let id = Uuid::new_v4().to_string();
            let dedupe_key = message_id_header
                .as_ref()
                .map(|message_id| format!("{}:{}", normalized, message_id.to_ascii_lowercase()))
                .unwrap_or_else(|| id.clone());

            let result = sqlx::query(
                r#"
                INSERT OR IGNORE INTO messages (
                    id, dedupe_key, inbox_id, email_address, from_address, subject,
                    text_content, html_content, has_html, raw_headers, raw_mime, raw_size,
                    created_at, timestamp, message_id_header
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&id)
            .bind(&dedupe_key)
            .bind(&inbox_id)
            .bind(&normalized)
            .bind(&from_address)
            .bind(&subject)
            .bind(&text_content)
            .bind(&html_content)
            .bind(if has_html { 1_i64 } else { 0_i64 })
            .bind(&raw_headers)
            .bind(&raw_mime)
            .bind(raw_size)
            .bind(&created_at)
            .bind(timestamp)
            .bind(message_id_header.clone())
            .execute(&self.pool)
            .await?;

            if result.rows_affected() > 0 {
                inserted.push(id);
            }
        }

        Ok(inserted)
    }

    pub async fn cleanup_expired_messages(&self) -> anyhow::Result<u64> {
        let cutoff = (Utc::now() - Duration::days(self.config.retention_days)).timestamp();

        let deleted_messages = sqlx::query("DELETE FROM messages WHERE timestamp < ?")
            .bind(cutoff)
            .execute(&self.pool)
            .await?
            .rows_affected();

        sqlx::query(
            r#"
            DELETE FROM inboxes
            WHERE last_seen_timestamp < ?
              AND NOT EXISTS (
                  SELECT 1 FROM messages WHERE messages.inbox_id = inboxes.id
              )
            "#,
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await?;

        Ok(deleted_messages)
    }

    fn resolve_domain(&self, requested: Option<&str>) -> AppResult<String> {
        match requested.map(str::trim).filter(|value| !value.is_empty()) {
            Some(domain) if domain.eq_ignore_ascii_case(&self.config.mail_domain) => {
                Ok(self.config.mail_domain.clone())
            }
            Some(_) => Err(AppError::BadRequest(format!(
                "Only {} is accepted by this server",
                self.config.mail_domain
            ))),
            None => Ok(self.config.mail_domain.clone()),
        }
    }

    fn validate_prefix<'a>(&self, prefix: &'a str) -> AppResult<&'a str> {
        if prefix.len() > 64 {
            return Err(AppError::BadRequest(
                "Prefix must be 64 characters or fewer".to_string(),
            ));
        }

        if prefix
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
        {
            Ok(prefix)
        } else {
            Err(AppError::BadRequest(
                "Prefix may only contain letters, digits, dot, underscore, and hyphen".to_string(),
            ))
        }
    }

    fn normalize_email(&self, email: &str) -> AppResult<String> {
        let trimmed = email.trim();
        let (local, domain) = trimmed
            .rsplit_once('@')
            .ok_or_else(|| AppError::BadRequest("Invalid email address".to_string()))?;

        if local.is_empty() || domain.is_empty() {
            return Err(AppError::BadRequest("Invalid email address".to_string()));
        }

        if !domain.eq_ignore_ascii_case(&self.config.mail_domain) {
            return Err(AppError::BadRequest(format!(
                "Email domain must be {}",
                self.config.mail_domain
            )));
        }

        if local.chars().any(|ch| ch.is_whitespace()) {
            return Err(AppError::BadRequest("Invalid email address".to_string()));
        }

        Ok(format!(
            "{}@{}",
            local.to_ascii_lowercase(),
            self.config.mail_domain
        ))
    }

    fn random_prefix(&self) -> String {
        Alphanumeric
            .sample_string(&mut rand::thread_rng(), 10)
            .to_ascii_lowercase()
    }

    async fn ensure_inbox(&self, email: &str) -> AppResult<String> {
        let normalized = self.normalize_email(email)?;
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT INTO inboxes (id, email_address, created_timestamp, last_seen_timestamp)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(email_address)
            DO UPDATE SET last_seen_timestamp = excluded.last_seen_timestamp
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&normalized)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(anyhow::Error::from)?;

        let inbox_id =
            sqlx::query_scalar::<_, String>("SELECT id FROM inboxes WHERE email_address = ?")
                .bind(&normalized)
                .fetch_one(&self.pool)
                .await
                .map_err(anyhow::Error::from)?;

        Ok(inbox_id)
    }

    async fn touch_inbox(&self, email: &str) -> AppResult<()> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            UPDATE inboxes
            SET last_seen_timestamp = ?
            WHERE email_address = ?
            "#,
        )
        .bind(now)
        .bind(email)
        .execute(&self.pool)
        .await
        .map_err(anyhow::Error::from)?;

        Ok(())
    }

    fn build_usage_snapshot(&self, used_today: i64, total_usage: i64) -> UsageSnapshot {
        let remaining_today = if self.config.daily_limit == 0 {
            -1
        } else {
            (self.config.daily_limit - used_today).max(0)
        };
        let remaining_total = if self.config.total_limit == 0 {
            -1
        } else {
            (self.config.total_limit - total_usage).max(0)
        };

        UsageSnapshot {
            daily_limit: self.config.daily_limit,
            used_today,
            remaining_today,
            total_limit: self.config.total_limit,
            total_usage,
            remaining_total,
        }
    }
}

fn extract_header_block(raw_mime: &str) -> String {
    raw_mime
        .split_once("\r\n\r\n")
        .map(|(headers, _)| headers.to_string())
        .or_else(|| {
            raw_mime
                .split_once("\n\n")
                .map(|(headers, _)| headers.to_string())
        })
        .unwrap_or_else(|| raw_mime.to_string())
}

fn extract_header_value(headers: &str, wanted: &str) -> Option<String> {
    let mut current_name: Option<String> = None;
    let mut current_value = String::new();

    for line in headers.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if current_name.is_some() {
                current_value.push(' ');
                current_value.push_str(line.trim());
            }
            continue;
        }

        if let Some(name) = current_name.take() {
            if name.eq_ignore_ascii_case(wanted) {
                return Some(current_value.trim().to_string());
            }
            current_value.clear();
        }

        if let Some((name, value)) = line.split_once(':') {
            current_name = Some(name.trim().to_string());
            current_value = value.trim().to_string();
        }
    }

    if let Some(name) = current_name
        && name.eq_ignore_ascii_case(wanted)
    {
        return Some(current_value.trim().to_string());
    }

    None
}

fn start_of_today_utc() -> i64 {
    let today: NaiveDate = Utc::now().date_naive();
    DateTime::<Utc>::from_naive_utc_and_offset(
        today.and_hms_opt(0, 0, 0).expect("valid midnight"),
        Utc,
    )
    .timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn test_config() -> AppConfig {
        AppConfig {
            app_base_url: "http://127.0.0.1:3000".to_string(),
            http_bind: "127.0.0.1:3000".to_string(),
            smtp_bind: "127.0.0.1:2525".to_string(),
            mail_domain: "example.com".to_string(),
            api_keys: vec!["test-key".to_string()],
            retention_days: 1,
            sqlite_path: ":memory:".to_string(),
            daily_limit: 0,
            total_limit: 0,
        }
    }

    #[tokio::test]
    async fn stores_and_lists_inbound_message() {
        let config = test_config();
        let pool = db::connect(&config).await.unwrap();
        let service = MailService::new(config, pool);

        let generated = service.generate_email(Some("demo"), None).await.unwrap();
        assert_eq!(generated.email, "demo@example.com");
        assert!(service.is_allowed_recipient(&generated.email));

        let raw = b"From: Sender <sender@example.net>\r\nMessage-ID: <abc123@example.net>\r\nSubject: Test code\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n654321\r\n";
        let ids = service
            .ingest_message(
                Some("sender@example.net"),
                std::slice::from_ref(&generated.email),
                raw,
            )
            .await
            .unwrap();
        assert_eq!(ids.len(), 1);

        let list = service.list_messages(&generated.email).await.unwrap();
        assert_eq!(list.count, 1);
        assert_eq!(list.emails[0].subject, "Test code");

        let detail = service.get_message(&ids[0]).await.unwrap();
        assert_eq!(detail.email_address, generated.email);
        assert!(detail.content.contains("654321"));
    }
}
