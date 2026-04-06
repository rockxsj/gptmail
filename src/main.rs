use anyhow::Result;
use axum::serve;
use gptmail::{AppConfig, MailService, db, smtp_server, web};
use tokio::net::TcpListener;
use tokio::time::{Duration, interval};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let config = AppConfig::from_env()?;
    let pool = db::connect(&config).await?;
    let service = MailService::new(config.clone(), pool);

    let app = web::router(service.clone());
    let http_listener = TcpListener::bind(&config.http_bind).await?;
    let http_addr = http_listener.local_addr()?;

    let http_task = tokio::spawn(async move {
        info!(bind = %http_addr, "http server listening");
        serve(http_listener, app).await.map_err(anyhow::Error::from)
    });

    let smtp_service = service.clone();
    let smtp_task = tokio::spawn(async move { smtp_server::run(smtp_service).await });

    let cleanup_service = service.clone();
    let cleanup_task = tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(900));
        loop {
            ticker.tick().await;
            match cleanup_service.cleanup_expired_messages().await {
                Ok(deleted) if deleted > 0 => info!(deleted, "cleaned expired messages"),
                Ok(_) => {}
                Err(error) => error!(?error, "cleanup task failed"),
            }
        }
    });

    tokio::select! {
        result = http_task => result??,
        result = smtp_task => result??,
        _ = tokio::signal::ctrl_c() => {
            info!("shutdown signal received");
        }
    }

    cleanup_task.abort();
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}
