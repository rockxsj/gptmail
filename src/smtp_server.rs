use crate::service::MailService;
use anyhow::Result;
use smtpd::{Error, Response, Session, SmtpConfig, SmtpHandler, SmtpHandlerFactory, async_trait};
use tracing::{error, info};

pub async fn run(service: MailService) -> Result<()> {
    let config = SmtpConfig {
        bind_addr: service.config().smtp_bind.clone(),
        appname: "gptmail".to_string(),
        hostname: service.config().mail_domain.clone(),
        require_auth: false,
        disable_reverse_dns: true,
        ..Default::default()
    };

    info!(
        bind = %config.bind_addr,
        domain = %service.config().mail_domain,
        "starting smtp server"
    );
    smtpd::start_server(config, InboundHandlerFactory { service })
        .await
        .map_err(anyhow::Error::from)
}

#[derive(Clone)]
struct InboundHandlerFactory {
    service: MailService,
}

impl SmtpHandlerFactory for InboundHandlerFactory {
    type Handler = InboundHandler;

    fn new_handler(&self, _session: &Session) -> Self::Handler {
        InboundHandler {
            service: self.service.clone(),
        }
    }
}

struct InboundHandler {
    service: MailService,
}

#[async_trait]
impl SmtpHandler for InboundHandler {
    async fn handle_rcpt(&mut self, _session: &Session, to: &str) -> smtpd::Result {
        if self.service.is_allowed_recipient(to) {
            Ok(Response::default())
        } else {
            Err(Error::Response(Response::new(
                550,
                format!(
                    "Recipient domain must be {}",
                    self.service.config().mail_domain
                ),
                Some("5.1.1".into()),
            )))
        }
    }

    async fn handle_email(&mut self, session: &Session, data: Vec<u8>) -> smtpd::Result {
        match self
            .service
            .ingest_message(Some(&session.from), &session.to, &data)
            .await
        {
            Ok(ids) => {
                let queue_id = ids.first().cloned().unwrap_or_else(|| "queued".to_string());
                Ok(Response::ok(format!("Queued as <{queue_id}>")))
            }
            Err(error) => {
                error!(?error, recipients = ?session.to, "failed to persist inbound message");
                Err(Error::Response(Response::new(
                    451,
                    "Temporary local problem while storing message",
                    Some("4.3.0".into()),
                )))
            }
        }
    }
}
