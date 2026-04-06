use crate::config::AppConfig;
use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Executor, SqlitePool};
use std::path::Path;
use std::str::FromStr;

pub async fn connect(config: &AppConfig) -> Result<SqlitePool> {
    if config.sqlite_path != ":memory:"
        && let Some(parent) = Path::new(&config.sqlite_path).parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    let options = if config.sqlite_path == ":memory:" {
        SqliteConnectOptions::from_str("sqlite::memory:")?
    } else {
        SqliteConnectOptions::new()
            .filename(&config.sqlite_path)
            .create_if_missing(true)
    }
    .foreign_keys(true)
    .journal_mode(SqliteJournalMode::Wal)
    .synchronous(SqliteSynchronous::Normal);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    initialize_schema(&pool).await?;
    Ok(pool)
}

async fn initialize_schema(pool: &SqlitePool) -> Result<()> {
    pool.execute(
        r#"
        CREATE TABLE IF NOT EXISTS inboxes (
            id TEXT PRIMARY KEY,
            email_address TEXT NOT NULL UNIQUE,
            created_timestamp INTEGER NOT NULL,
            last_seen_timestamp INTEGER NOT NULL
        );
        "#,
    )
    .await?;

    pool.execute(
        r#"
        CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            dedupe_key TEXT NOT NULL UNIQUE,
            inbox_id TEXT NOT NULL,
            email_address TEXT NOT NULL,
            from_address TEXT NOT NULL,
            subject TEXT NOT NULL,
            text_content TEXT NOT NULL,
            html_content TEXT NOT NULL,
            has_html INTEGER NOT NULL,
            raw_headers TEXT NOT NULL,
            raw_mime TEXT NOT NULL,
            raw_size INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            message_id_header TEXT,
            FOREIGN KEY (inbox_id) REFERENCES inboxes(id) ON DELETE CASCADE
        );
        "#,
    )
    .await?;

    pool.execute(
        r#"
        CREATE TABLE IF NOT EXISTS daily_usage (
            api_key TEXT NOT NULL,
            usage_day TEXT NOT NULL,
            request_count INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (api_key, usage_day)
        );
        "#,
    )
    .await?;

    pool.execute(
        r#"
        CREATE TABLE IF NOT EXISTS total_usage (
            api_key TEXT PRIMARY KEY,
            request_count INTEGER NOT NULL DEFAULT 0
        );
        "#,
    )
    .await?;

    pool.execute("CREATE INDEX IF NOT EXISTS idx_messages_email_timestamp ON messages(email_address, timestamp DESC);")
        .await?;
    pool.execute("CREATE INDEX IF NOT EXISTS idx_messages_timestamp ON messages(timestamp DESC);")
        .await?;
    pool.execute(
        "CREATE INDEX IF NOT EXISTS idx_inboxes_last_seen ON inboxes(last_seen_timestamp);",
    )
    .await?;

    Ok(())
}
