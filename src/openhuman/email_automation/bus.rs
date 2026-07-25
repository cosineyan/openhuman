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
            body_preview,
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

        let ctx = ops::extract_email_context(&preview);
        let config = Arc::clone(&self.config);
        // Spawn so the event handler never blocks the dispatch loop
        tokio::spawn(async move {
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
