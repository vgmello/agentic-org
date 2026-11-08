//! The messaging operations behind `msg send`, `ask`, `reply` and `inbox`: each one whole,
//! so the CLI only parses, calls and prints.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::Value;

use crate::envelope::{Envelope, MessagePriority};
use crate::meta::InboxItem;
use crate::spool::{Spool, SpoolError};
use crate::time::check_wait_timeout;

/// `msg inbox --summary` lists at most this many items unless `--limit` says otherwise.
pub const INBOX_SUMMARY_DEFAULT_LIMIT: usize = 50;
/// The largest `--limit` `msg inbox` takes.
pub const INBOX_MAX_LIMIT: usize = 1000;

/// What a sender writes: everything but the addressing.
#[derive(Debug, Clone, Default)]
pub struct Outgoing {
    pub subject: Option<String>,
    pub body: String,
    pub meta: BTreeMap<String, Value>,
    pub priority: Option<MessagePriority>,
}

/// `msg inbox` filters.
#[derive(Debug, Clone, Default)]
pub struct InboxQuery {
    /// Include snoozed, done and read messages.
    pub include_all: bool,
    /// Headers only (no body, no meta); at most [`INBOX_SUMMARY_DEFAULT_LIMIT`] by default.
    pub summary_only: bool,
    /// Only messages from this sender (case-insensitive).
    pub from: Option<String>,
    /// Only this message and direct replies to it (Phase A threads).
    pub thread: Option<String>,
    /// At most this many (1..=[`INBOX_MAX_LIMIT`]).
    pub limit: Option<usize>,
}

fn invalid(e: String) -> SpoolError {
    SpoolError::InvalidInput(e)
}

impl Spool {
    fn queue(
        &self,
        mut env: Envelope,
        priority: Option<MessagePriority>,
    ) -> Result<Envelope, SpoolError> {
        if let Some(p) = priority {
            env.set_priority(p);
        }
        self.enqueue(&env)?;
        self.nudge_relay();
        Ok(env)
    }

    /// `msg send`: queue a message to a registered member.
    pub fn send(&self, from: &str, to: &str, msg: Outgoing) -> Result<Envelope, SpoolError> {
        if !self.is_registered(to) {
            return Err(SpoolError::UnknownRecipient(to.to_string()));
        }
        let env = Envelope::new_msg(from, to, msg.subject, msg.body, msg.meta).map_err(invalid)?;
        self.queue(env, msg.priority)
    }

    /// `msg ask`: queue a request and wait up to `timeout` for its reply (claiming only that
    /// reply). Everything is checked before any side effect (registering the asker, binding
    /// its wake socket, queueing).
    pub fn ask(
        &self,
        from: &str,
        to: &str,
        msg: Outgoing,
        timeout: Duration,
    ) -> Result<Envelope, SpoolError> {
        check_wait_timeout(timeout).map_err(invalid)?;
        if !self.is_registered(to) {
            return Err(SpoolError::UnknownRecipient(to.to_string()));
        }
        let mut req = Envelope::new_request(from, to, msg.subject, msg.body, msg.meta, timeout)
            .map_err(invalid)?;
        if let Some(p) = msg.priority {
            req.set_priority(p);
        }
        req.to_json_bytes().map_err(invalid)?;

        self.register(from)?;
        let listener = self.bind_wake_listener(from)?;
        let req = self.queue(req, None)?;
        self.wait_for_reply_with_listener(from, &req.id, &listener, timeout)
    }

    /// `msg reply`: answer message `orig_id` in `me`'s inbox, then mark it read (only once the
    /// reply is safely queued; a deliberate change from v1-old, per ADR-0003 §2.3).
    pub fn reply(&self, me: &str, orig_id: &str, msg: Outgoing) -> Result<Envelope, SpoolError> {
        let orig = self.find_for_reply(me, orig_id)?;
        if !self.is_registered(&orig.from) {
            return Err(SpoolError::UnknownRecipient(orig.from.clone()));
        }
        let reply = orig
            .new_reply(me, msg.subject, msg.body, msg.meta)
            .map_err(invalid)?;
        let reply = self.queue(reply, msg.priority)?;
        let _ = self.claim(me, orig_id);
        Ok(reply)
    }

    /// `msg inbox`: [`Spool::list_inbox_items`] with the CLI's filters and defaults.
    pub fn inbox(&self, agent: &str, query: &InboxQuery) -> Result<Vec<InboxItem>, SpoolError> {
        if query.limit.is_some_and(|l| l == 0 || l > INBOX_MAX_LIMIT) {
            return Err(invalid(format!(
                "--limit must be between 1 and {INBOX_MAX_LIMIT}"
            )));
        }
        let mut items =
            self.list_inbox_items(agent, query.include_all, query.summary_only, None)?;
        if let Some(from) = &query.from {
            items.retain(|i| i.from.eq_ignore_ascii_case(from));
        }
        if let Some(thread) = &query.thread {
            items.retain(|i| &i.id == thread || i.reply_to.as_deref() == Some(thread.as_str()));
        }
        let limit = query.limit.or(if query.summary_only {
            Some(INBOX_SUMMARY_DEFAULT_LIMIT)
        } else {
            None
        });
        if let Some(limit) = limit {
            items.truncate(limit);
        }
        Ok(items)
    }
}
