//! The channel catch-up ladder (`TASK-99`): read-marker watermarks, delta anchors, thread
//! statistics and one-line snippets, computed engine-independently over the conversation's
//! canonical `(created_at, local_seq, id)` order so Memory and SQLite answer alike.

use std::collections::HashMap;

use org_proto::chat::{ChatMessagesParams, SNIPPET_MAX_CHARS};
use org_proto::{ApiError, ErrorCode};
use org_store::Id;

use crate::messaging::{
    ports::MessagingRead,
    records::{ConversationReadMarkerRecord, ConversationRecord, MessageRecord},
};

/// How many of `msgs` (in canonical order) the caller's read marker covers: `msgs[boundary..]`
/// are after it. A marker naming a message no longer here falls back to its `read_at` (and a
/// time-only marker means everything created by then); no marker covers nothing.
pub(crate) fn watermark_boundary(
    msgs: &[MessageRecord],
    marker: Option<&ConversationReadMarkerRecord>,
) -> usize {
    let Some(marker) = marker else {
        return 0;
    };
    if let Some(anchor) = marker.last_read_message_id.as_deref() {
        if let Some(index) = msgs.iter().position(|m| m.id == anchor) {
            return index + 1;
        }
    }
    match marker.read_at.as_deref() {
        Some(read_at) => msgs.partition_point(|m| m.created_at.as_str() <= read_at),
        None => 0,
    }
}

/// Whether `caller` sent `msg` (the caller's own posts are never new to it).
pub(crate) fn sent_by(msg: &MessageRecord, caller: &Id) -> bool {
    msg.sender_employee_id.as_ref() == Some(caller)
}

/// The messages new to `caller`: after the watermark, not sent by it.
pub(crate) fn new_messages<'a>(
    msgs: &'a [MessageRecord],
    boundary: usize,
    caller: &'a Id,
) -> impl Iterator<Item = &'a MessageRecord> + 'a {
    msgs.get(boundary..)
        .unwrap_or_default()
        .iter()
        .filter(move |m| !sent_by(m, caller))
}

/// Whether `msg` is a reply inside another message's thread.
pub(crate) fn is_reply(msg: &MessageRecord) -> bool {
    msg.thread_root_id
        .as_deref()
        .is_some_and(|root| root != msg.id)
}

/// Whether `msg` belongs on the channel timeline: a thread root, or a reply also posted there.
pub(crate) fn is_top_level(msg: &MessageRecord) -> bool {
    !is_reply(msg) || msg.also_in_channel
}

/// Reply count and latest reply time per thread root, from one pass over the conversation.
#[derive(Debug, Default)]
pub(crate) struct ThreadStats(HashMap<String, (u64, String)>);

impl ThreadStats {
    pub(crate) fn of(msgs: &[MessageRecord]) -> Self {
        let mut stats: HashMap<String, (u64, String)> = HashMap::new();
        for msg in msgs.iter().filter(|m| is_reply(m)) {
            let Some(root) = msg.thread_root_id.as_deref() else {
                continue;
            };
            let entry = stats.entry(root.to_owned()).or_default();
            entry.0 += 1;
            if msg.created_at > entry.1 {
                entry.1.clone_from(&msg.created_at);
            }
        }
        Self(stats)
    }

    /// `(reply_count, latest_reply_at)` of the thread `message_id` roots.
    pub(crate) fn of_root(&self, message_id: &str) -> (u64, Option<String>) {
        self.0
            .get(message_id)
            .map_or((0, None), |(count, at)| (*count, Some(at.clone())))
    }
}

/// `body` on one line (every run of whitespace one space), at most [`SNIPPET_MAX_CHARS`]
/// characters, cut on a character boundary and ending in `…` when cut.
pub fn snippet(body: &str) -> String {
    // At most one character past the cap is kept: enough to know the body was longer.
    let mut chars: Vec<char> = Vec::with_capacity(SNIPPET_MAX_CHARS + 1);
    'words: for word in body.split_whitespace() {
        if !chars.is_empty() {
            chars.push(' ');
        }
        for ch in word.chars() {
            if chars.len() > SNIPPET_MAX_CHARS {
                break 'words;
            }
            chars.push(ch);
        }
    }
    if chars.len() > SNIPPET_MAX_CHARS {
        chars.truncate(SNIPPET_MAX_CHARS - 1);
        while chars.last() == Some(&' ') {
            chars.pop();
        }
        chars.push('…');
    }
    chars.into_iter().collect()
}

/// A delta read's bounds in `msgs`, as indexes `(base, page)`. Matches are counted from
/// `base`, which only the read's own anchor sets (the `since_read` marker and `after_id`), so
/// `total` holds steady from page to page; the page starts at `page`, strictly after
/// `page_after_id` but never before `base`, so an older cursor cannot reach behind the anchor.
pub(crate) fn delta_bounds(
    read: &impl MessagingRead,
    msgs: &[MessageRecord],
    conv_id: &Id,
    caller: &Id,
    params: &ChatMessagesParams,
) -> Result<(usize, usize), ApiError> {
    let mut base = 0;
    if params.since_read {
        let marker = read.read_marker(conv_id, caller).map_err(|_| {
            ApiError::new(ErrorCode::StorageError, "failed to read the read marker")
        })?;
        base = watermark_boundary(msgs, marker.as_ref());
    }
    if let Some(anchor) = params.after_id.as_deref() {
        base = base.max(just_after(msgs, anchor, "after_id")?);
    }
    let page = match params.page_after_id.as_deref() {
        Some(cursor) => base.max(just_after(msgs, cursor, "page_after_id")?),
        None => base,
    };
    Ok((base, page))
}

/// The index just past message `anchor` in `msgs`. A message this conversation does not hold,
/// unknown or another conversation's, is refused alike, so the answer reveals nothing else.
fn just_after(msgs: &[MessageRecord], anchor: &str, field: &str) -> Result<usize, ApiError> {
    msgs.iter()
        .position(|m| m.id == anchor)
        .map(|index| index + 1)
        .ok_or_else(|| {
            ApiError::new(
                ErrorCode::NotFound,
                format!("{field} names no message in this conversation"),
            )
        })
}

/// The caller's new channel messages for a `since_read` digest (`TASK-99` rung 3).
#[derive(Debug, Clone)]
pub struct ChannelCatchup {
    pub conversation: ConversationRecord,
    /// The oldest new messages, at most the requested limit, in canonical order.
    pub messages: Vec<MessageRecord>,
    /// Every new message after the read marker (`>= messages.len()`).
    pub new_total: u64,
    /// The caller's read marker as it stands (never moved by a digest).
    pub last_read_message_id: Option<String>,
}

impl ChannelCatchup {
    /// New messages left after the digest's last message.
    pub fn remaining_unread(&self) -> u64 {
        self.new_total.saturating_sub(self.messages.len() as u64)
    }

    /// The last message the digest covers, or the unchanged marker when nothing is new.
    pub fn through_message_id(&self) -> Option<String> {
        self.messages
            .last()
            .map(|m| m.id.clone())
            .or_else(|| self.last_read_message_id.clone())
    }
}

#[cfg(test)]
#[path = "catchup_tests.rs"]
mod tests;
