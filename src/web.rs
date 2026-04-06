use crate::error::{AppError, AppResult};
use crate::models::{ApiEnvelope, ApiKeyQuery, EmailQuery, GenerateEmailRequest, UsageSnapshot};
use crate::service::MailService;
use askama::Template;
use axum::extract::{Json, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get};
use axum::{Router, debug_handler};
use serde::Serialize;
use tower_http::trace::TraceLayer;
use tracing::error;

pub fn router(service: MailService) -> Router {
    Router::new()
        .route("/", get(index_page))
        .route("/favicon.ico", get(favicon))
        .route(
            "/api/generate-email",
            get(generate_email_get).post(generate_email_post),
        )
        .route("/api/emails", get(list_emails))
        .route("/api/email/{id}", get(get_email).delete(delete_email))
        .route("/api/emails/clear", delete(clear_inbox))
        .route("/api/stats", get(stats))
        .route("/{email}/{id}", get(message_page))
        .route("/{email}", get(inbox_page))
        .layer(TraceLayer::new_for_http())
        .with_state(service)
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    mail_domain: &'a str,
}

#[derive(Template)]
#[template(path = "inbox.html")]
struct InboxTemplate<'a> {
    email: &'a str,
}

#[derive(Template)]
#[template(path = "message.html")]
struct MessageTemplate<'a> {
    email: &'a str,
    message_id: &'a str,
}

async fn favicon() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn index_page(State(service): State<MailService>) -> Response {
    page_result(render_template(IndexTemplate {
        mail_domain: &service.config().mail_domain,
    }))
}

async fn inbox_page(State(service): State<MailService>, Path(email): Path<String>) -> Response {
    match validate_ui_email(&service, &email) {
        Ok(normalized) => page_result(render_template(InboxTemplate { email: &normalized })),
        Err(error) => page_result(Err(error)),
    }
}

async fn message_page(
    State(service): State<MailService>,
    Path((email, id)): Path<(String, String)>,
) -> Response {
    match validate_ui_email(&service, &email) {
        Ok(normalized) => page_result(render_template(MessageTemplate {
            email: &normalized,
            message_id: &id,
        })),
        Err(error) => page_result(Err(error)),
    }
}

#[debug_handler]
async fn generate_email_get(
    State(service): State<MailService>,
    headers: HeaderMap,
    Query(query): Query<ApiKeyQuery>,
) -> Response {
    api_result(async move {
        let usage = service
            .authorize_and_track(resolve_api_key(&headers, query.api_key.as_deref()))
            .await?;
        let data = service.generate_email(None, None).await?;
        Ok(success_json(data, usage))
    })
    .await
}

#[debug_handler]
async fn generate_email_post(
    State(service): State<MailService>,
    headers: HeaderMap,
    Query(query): Query<ApiKeyQuery>,
    Json(payload): Json<GenerateEmailRequest>,
) -> Response {
    api_result(async move {
        let usage = service
            .authorize_and_track(resolve_api_key(&headers, query.api_key.as_deref()))
            .await?;
        let data = service
            .generate_email(payload.prefix.as_deref(), payload.domain.as_deref())
            .await?;
        Ok(success_json(data, usage))
    })
    .await
}

#[debug_handler]
async fn list_emails(
    State(service): State<MailService>,
    headers: HeaderMap,
    Query(query): Query<EmailQuery>,
) -> Response {
    api_result(async move {
        let usage = service
            .authorize_and_track(resolve_api_key(&headers, query.api_key.as_deref()))
            .await?;
        let data = service.list_messages(&query.email).await?;
        Ok(success_json(data, usage))
    })
    .await
}

#[debug_handler]
async fn get_email(
    State(service): State<MailService>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<ApiKeyQuery>,
) -> Response {
    api_result(async move {
        let usage = service
            .authorize_and_track(resolve_api_key(&headers, query.api_key.as_deref()))
            .await?;
        let data = service.get_message(&id).await?;
        Ok(success_json(data, usage))
    })
    .await
}

#[debug_handler]
async fn delete_email(
    State(service): State<MailService>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<ApiKeyQuery>,
) -> Response {
    api_result(async move {
        let usage = service
            .authorize_and_track(resolve_api_key(&headers, query.api_key.as_deref()))
            .await?;
        let data = service.delete_message(&id).await?;
        Ok(success_json(data, usage))
    })
    .await
}

#[debug_handler]
async fn clear_inbox(
    State(service): State<MailService>,
    headers: HeaderMap,
    Query(query): Query<EmailQuery>,
) -> Response {
    api_result(async move {
        let usage = service
            .authorize_and_track(resolve_api_key(&headers, query.api_key.as_deref()))
            .await?;
        let data = service.clear_inbox(&query.email).await?;
        Ok(success_json(data, usage))
    })
    .await
}

#[debug_handler]
async fn stats(
    State(service): State<MailService>,
    headers: HeaderMap,
    Query(query): Query<ApiKeyQuery>,
) -> Response {
    api_result(async move {
        let usage = service
            .authorize_and_track(resolve_api_key(&headers, query.api_key.as_deref()))
            .await?;
        let data = service.stats().await?;
        Ok(success_json(data, usage))
    })
    .await
}

async fn api_result<F>(future: F) -> Response
where
    F: std::future::Future<Output = AppResult<Response>>,
{
    match future.await {
        Ok(response) => response,
        Err(error) => error_json(error),
    }
}

fn success_json<T>(data: T, usage: UsageSnapshot) -> Response
where
    T: Serialize,
{
    (
        StatusCode::OK,
        axum::Json(ApiEnvelope {
            success: true,
            data: Some(data),
            error: None,
            usage: Some(usage),
        }),
    )
        .into_response()
}

fn error_json(error: AppError) -> Response {
    if let AppError::Internal(ref inner) = error {
        error!(?inner, "request failed");
    }

    let status = error.status_code();
    (
        status,
        axum::Json(ApiEnvelope::<serde_json::Value> {
            success: false,
            data: None,
            error: Some(error.to_string()),
            usage: error.usage(),
        }),
    )
        .into_response()
}

fn page_result(result: AppResult<Html<String>>) -> Response {
    match result {
        Ok(html) => html.into_response(),
        Err(error) => {
            if let AppError::Internal(ref inner) = error {
                error!(?inner, "page render failed");
            }
            (error.status_code(), error.to_string()).into_response()
        }
    }
}

fn resolve_api_key<'a>(headers: &'a HeaderMap, query_api_key: Option<&'a str>) -> Option<&'a str> {
    query_api_key.or_else(|| {
        headers
            .get("x-api-key")
            .and_then(|value| value.to_str().ok())
    })
}

fn validate_ui_email(service: &MailService, email: &str) -> AppResult<String> {
    if service.is_allowed_recipient(email) {
        Ok(email.to_ascii_lowercase())
    } else {
        Err(AppError::NotFound("Mailbox not found".to_string()))
    }
}

fn render_template<T: Template>(template: T) -> AppResult<Html<String>> {
    let rendered = template
        .render()
        .map_err(|error| AppError::Internal(error.into()))?;
    Ok(Html(rendered))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::db;
    use axum::body::Body;
    use axum::http::Request;
    use tower::util::ServiceExt;

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
    async fn api_flow_is_compatible() {
        let config = test_config();
        let pool = db::connect(&config).await.unwrap();
        let service = MailService::new(config, pool);
        let app = router(service.clone());

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/generate-email?api_key=test-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let email = payload["data"]["email"].as_str().unwrap().to_string();

        service
            .ingest_message(
                Some("sender@example.net"),
                std::slice::from_ref(&email),
                b"From: Sender <sender@example.net>\r\nSubject: Login code\r\n\r\n123456\r\n",
            )
            .await
            .unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/emails?email={email}&api_key=test-key"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["data"]["count"].as_u64().unwrap(), 1);
    }
}
