use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSnapshot {
    pub daily_limit: i64,
    pub used_today: i64,
    pub remaining_today: i64,
    pub total_limit: i64,
    pub total_usage: i64,
    pub remaining_total: i64,
}

#[derive(Debug, Serialize)]
pub struct ApiEnvelope<T>
where
    T: Serialize,
{
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeneratedEmailData {
    pub email: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmailSummary {
    pub id: String,
    pub email_address: String,
    pub from_address: String,
    pub subject: String,
    pub timestamp: i64,
    pub created_at: String,
    pub has_html: bool,
    pub raw_size: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmailsListData {
    pub emails: Vec<EmailSummary>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmailDetailData {
    pub id: String,
    pub email_address: String,
    pub from_address: String,
    pub subject: String,
    pub content: String,
    pub html_content: String,
    pub has_html: bool,
    pub timestamp: i64,
    pub raw_size: i64,
    pub created_at: String,
    pub raw_headers: String,
    pub raw_mime: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageData {
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClearInboxData {
    pub message: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SiteStatsData {
    pub mail_domain: String,
    pub inboxes: i64,
    pub messages: i64,
    pub messages_today: i64,
    pub generated_today: i64,
    pub retention_days: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GenerateEmailRequest {
    pub prefix: Option<String>,
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmailQuery {
    pub email: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiKeyQuery {
    pub api_key: Option<String>,
}
