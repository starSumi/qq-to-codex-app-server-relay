use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, Error as AnyhowError};
use relay_protocol::{ChatScope, CoreToAdapter, InboundMessage, PROTOCOL_VERSION};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::{
    app_server::AppServerClient,
    config::Config,
    storage::{InboxDisposition, Store},
};

pub struct Engine {
    config: Arc<Config>,
    store: Arc<Store>,
    app_server: AppServerClient,
    target_locks: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    rate_limits: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl Engine {
    pub fn new(config: Arc<Config>, store: Arc<Store>, app_server: AppServerClient) -> Self {
        Self {
            config,
            store,
            app_server,
            target_locks: Mutex::new(HashMap::new()),
            rate_limits: Mutex::new(HashMap::new()),
        }
    }

    pub async fn process(&self, message: InboundMessage) -> anyhow::Result<Option<CoreToAdapter>> {
        if !self.config.auth_mode.authorizes(
            message.sender_is_bot,
            &message.sender_id,
            &self.config.allow_from,
        ) {
            tracing::warn!(sender = %pseudonym(&message.sender_id), "ignored unauthorized inbound message");
            return Ok(None);
        }
        if self.validate(&message).is_err() {
            tracing::warn!(sender = %pseudonym(&message.sender_id), "ignored invalid inbound message");
            return Ok(None);
        }
        if !self.take_rate_limit(&message.sender_id) {
            return Ok(Some(CoreToAdapter::Error {
                protocol_version: PROTOCOL_VERSION.into(),
                outbox_id: None,
                in_reply_to: Some(message.message_id),
                target: Some(message.target),
                code: "rate_limited".into(),
                message: "Too many requests. Please try again later.".into(),
                retryable: true,
            }));
        }

        let target_key = target_key(&message);
        let target_lock = {
            let mut locks = self.target_locks.lock().expect("target lock map poisoned");
            Arc::clone(
                locks
                    .entry(target_key.clone())
                    .or_insert_with(|| Arc::new(AsyncMutex::new(()))),
            )
        };
        let guard = target_lock.lock().await;
        let result = self.process_for_target(message, &target_key).await;
        drop(guard);
        self.remove_target_lock_if_idle(&target_key, &target_lock);
        result
    }

    async fn process_for_target(
        &self,
        message: InboundMessage,
        target_key: &str,
    ) -> anyhow::Result<Option<CoreToAdapter>> {
        let content_hash = format!("{:x}", Sha256::digest(message.content.as_bytes()));
        match self
            .store
            .begin_inbound(&message, target_key, &content_hash)?
        {
            InboxDisposition::New => {}
            InboxDisposition::Duplicate => {
                tracing::info!(message = %pseudonym(&message.message_id), "ignored duplicate inbound message");
                return Ok(None);
            }
            InboxDisposition::Conflict => {
                tracing::error!(message = %pseudonym(&message.message_id), "rejected conflicting reuse of a message identifier");
                return Ok(None);
            }
        }

        if message.content.trim() == "/reset" {
            self.store.reset_thread(target_key)?;
            return self
                .store
                .complete_reply(
                    &message,
                    "The Codex thread for this QQ conversation was reset.",
                )
                .map(Some);
        }

        let result = self.run_codex_turn(target_key, &message.content).await;
        match result {
            Ok(text) => {
                let text = truncate_utf8(
                    if text.is_empty() {
                        "(empty response)"
                    } else {
                        &text
                    },
                    self.config.max_output_bytes,
                );
                self.store.complete_reply(&message, &text).map(Some)
            }
            Err(error) => {
                let reference = Uuid::new_v4().simple().to_string();
                let reference = &reference[..12];
                let failure_kind = failure_kind(&error);
                tracing::error!(
                    reference,
                    target = %pseudonym(target_key),
                    failure_kind,
                    "Codex turn failed"
                );
                let public_message = public_failure_message(failure_kind, reference);
                self.store
                    .complete_error(&message, &public_message, "codex_turn_failed")
                    .map(Some)
            }
        }
    }

    fn remove_target_lock_if_idle(&self, target_key: &str, target_lock: &Arc<AsyncMutex<()>>) {
        let mut locks = self.target_locks.lock().expect("target lock map poisoned");
        let is_current = locks
            .get(target_key)
            .is_some_and(|current| Arc::ptr_eq(current, target_lock));
        if is_current && Arc::strong_count(target_lock) == 2 {
            locks.remove(target_key);
        }
    }

    async fn run_codex_turn(&self, target_key: &str, content: &str) -> anyhow::Result<String> {
        let mut thread_id = self.store.thread_id(target_key)?;
        if let Some(existing) = thread_id.as_deref() {
            match self.app_server.resume_thread(existing).await {
                Ok(resumed) => thread_id = Some(resumed),
                Err(_error) => {
                    tracing::warn!(target = %pseudonym(target_key), "thread resume failed; starting a replacement");
                    thread_id = None;
                }
            }
        }
        let thread_id = if let Some(thread_id) = thread_id {
            thread_id
        } else {
            self.app_server.start_thread().await?
        };
        // A successful resume is activity too. Refreshing the binding prevents retention
        // from deleting an actively used, long-lived thread.
        self.store.set_thread_id(target_key, &thread_id)?;
        self.app_server
            .run_turn(&thread_id, content)
            .await
            .context("app-server turn failed")
    }

    fn validate(&self, message: &InboundMessage) -> anyhow::Result<()> {
        for (name, value) in [
            ("message_id", message.message_id.as_str()),
            ("sender_id", message.sender_id.as_str()),
            ("target_id", message.target.target_id.as_str()),
        ] {
            if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                anyhow::bail!("invalid {name}");
            }
        }
        if message.platform_timestamp.len() > 64 {
            anyhow::bail!("invalid platform_timestamp");
        }
        if !target_matches_sender(
            &message.target.scope,
            &message.target.target_id,
            &message.sender_id,
        ) {
            anyhow::bail!("c2c target does not match sender");
        }
        let content = message.content.trim();
        if content.is_empty() {
            anyhow::bail!("message content is empty");
        }
        if content.len() > self.config.max_input_bytes {
            anyhow::bail!("message content exceeds RELAY_MAX_INPUT_BYTES");
        }
        Ok(())
    }

    fn take_rate_limit(&self, sender_id: &str) -> bool {
        let now = Instant::now();
        let cutoff = now.checked_sub(Duration::from_secs(60)).unwrap_or(now);
        let mut limits = self.rate_limits.lock().expect("rate limit map poisoned");
        let entries = limits.entry(sender_id.to_owned()).or_default();
        while entries.front().is_some_and(|entry| *entry < cutoff) {
            entries.pop_front();
        }
        if entries.len() >= self.config.rate_limit_per_minute {
            return false;
        }
        entries.push_back(now);
        true
    }
}

fn target_matches_sender(scope: &ChatScope, target_id: &str, sender_id: &str) -> bool {
    !matches!(scope, ChatScope::C2c) || target_id == sender_id
}

fn target_key(message: &InboundMessage) -> String {
    let scope = match message.target.scope {
        ChatScope::C2c => "c2c",
        ChatScope::Group => "group",
    };
    format!("{scope}:{}", message.target.target_id)
}

fn failure_kind(error: &AnyhowError) -> &'static str {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<crate::app_server::AppServerError>())
        .map_or(
            "internal_error",
            crate::app_server::AppServerError::failure_kind,
        )
}

fn public_failure_message(kind: &str, reference: &str) -> String {
    let detail = match kind {
        "upstream_bad_gateway" | "upstream_unavailable" => {
            "The model provider is temporarily unavailable. Please try again later."
        }
        "upstream_rate_limited" => {
            "The model provider is rate limiting requests. Please try again later."
        }
        "upstream_auth_failed" => {
            "The model provider rejected authentication. Check the Codex provider configuration."
        }
        "turn_timeout" | "request_timeout" => {
            "The Codex request timed out. Please try again later."
        }
        _ => "The Codex relay could not complete this request.",
    };
    format!("{detail} Reference: {reference}")
}

fn pseudonym(value: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(value.as_bytes()));
    digest[..12].to_owned()
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let suffix = "\n\n[response truncated by relay policy]";
    if max_bytes <= suffix.len() {
        return suffix[..max_bytes].to_owned();
    }
    let content_limit = max_bytes.saturating_sub(suffix.len());
    let mut boundary = content_limit.min(value.len());
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}{suffix}", &value[..boundary])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pseudonyms_are_stable_but_do_not_expose_identifiers() {
        assert_eq!(pseudonym("openid"), pseudonym("openid"));
        assert_ne!(pseudonym("openid"), "openid");
        assert_eq!(pseudonym("openid").len(), 12);
    }

    #[test]
    fn output_truncation_preserves_utf8_boundaries() {
        let output = truncate_utf8("你好世界", 44);
        assert!(output.is_char_boundary(output.len()));
        assert!(output.len() <= 44);

        let tiny = truncate_utf8("oversized", 8);
        assert_eq!(tiny.len(), 8);
    }

    #[test]
    fn c2c_targets_cannot_redirect_replies_to_another_sender() {
        assert!(target_matches_sender(&ChatScope::C2c, "alice", "alice"));
        assert!(!target_matches_sender(&ChatScope::C2c, "bob", "alice"));
        assert!(target_matches_sender(&ChatScope::Group, "group-1", "alice"));
    }

    #[test]
    fn public_failure_messages_classify_retryable_provider_errors() {
        assert!(
            public_failure_message("upstream_bad_gateway", "abc123")
                .starts_with("The model provider is temporarily unavailable.")
        );
        assert!(
            public_failure_message("upstream_rate_limited", "abc123").contains("rate limiting")
        );
        assert!(
            public_failure_message("internal_error", "abc123")
                .starts_with("The Codex relay could not complete this request.")
        );
    }
}
