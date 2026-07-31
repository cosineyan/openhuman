use std::sync::{Arc, OnceLock};

use async_trait::async_trait;

use crate::core::event_bus::{subscribe_global, DomainEvent, EventHandler, SubscriptionHandle};
use crate::openhuman::config::Config;

use super::ops;

static EMAIL_AUTOMATION_HANDLE: OnceLock<SubscriptionHandle> = OnceLock::new();

struct EmailAutomationSubscriber {
    config: Arc<Config>,
}

#[async_trait]
impl EventHandler for EmailAutomationSubscriber {
    fn name(&self) -> &str {
        "email_automation::email_ingest"
    }

    fn domains(&self) -> Option<&[&str]> {
        Some(&["memory"])
    }

    async fn handle(&self, event: &DomainEvent) {
        let DomainEvent::DocumentCanonicalized {
            source_kind,
            source_id,
            body_preview,
            chunk_ids,
            ..
        } = event
        else {
            return;
        };

        if source_kind != "email" {
            return;
        }

        let preview = match body_preview {
            Some(p) if !p.is_empty() => p.clone(),
            _ => return,
        };

        let first_chunk_id = chunk_ids.first().cloned();
        let config = Arc::clone(&self.config);
        let preview_clone = preview.clone();
        let source_id_clone = source_id.clone();

        tokio::spawn(async move {
            // Try to read full body so parse_script gets complete email content.
            // If read_chunk_body returns ≤500 chars it's a truncated preview — fall back to Graph API.
            let full_body = match first_chunk_id.as_deref() {
                Some(id) => {
                    match crate::openhuman::memory_store::content::read::read_chunk_body(&config, id) {
                        Ok(b) if b.len() > 500 => b,
                        _ => ops::fetch_full_email_body_pub(&config, &source_id_clone, &preview_clone).await,
                    }
                }
                None => ops::fetch_full_email_body_pub(&config, &source_id_clone, &preview_clone).await,
            };

            let mut ctx = ops::extract_email_context(&preview_clone);
            ctx.full_body = full_body;
            ctx.source_id = source_id_clone;
            ops::process_email(&config, &ctx);
        });
    }
}

/// Register the email-automation subscriber. Idempotent — safe to call multiple times.
pub fn register_email_automation_subscriber(config: Arc<Config>) {
    if EMAIL_AUTOMATION_HANDLE.get().is_some() {
        return;
    }
    match subscribe_global(Arc::new(EmailAutomationSubscriber { config })) {
        Some(handle) => {
            let _ = EMAIL_AUTOMATION_HANDLE.set(handle);
            log::debug!("[email_automation] subscriber registered");
        }
        None => {
            log::warn!("[email_automation] failed to register subscriber");
        }
    }
}
