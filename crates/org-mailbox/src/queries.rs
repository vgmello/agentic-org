//! Query, reply, and external wake methods on [`Mailbox`] (`msg.reply`, `msg.list`, `msg.get`).

use org_proto::msg::{
    HeaderPage, MsgGetParams, MsgGetResult, MsgListParams, MsgReplyParams, MsgSendParams,
    SendResult, DEFAULT_PAGE_LIMIT, KIND_DELIVERY_FAILED, KIND_REPLY, MAX_ID_BYTES, MAX_PAGE_LIMIT,
    OFFICE_SENDER,
};

use crate::error::MailboxError;
use crate::mailbox::{address, optional_size, size, Mailbox};
use crate::model::{HeaderQuery, HeaderScope};

impl Mailbox {
    /// Wakes any held `msg.wait` listeners for `member_id` (used when channel/chat writes
    /// deliver an inbox entry to that member).
    pub fn notify_member(&self, member_id: &str) {
        self.waiters.notify(member_id);
    }

    /// `msg.reply`: reply to a message addressed to the bound member, marking the original
    /// read and delivering the reply to the original sender in the same thread.
    pub fn reply(
        &self,
        bound_member_id: &str,
        params: MsgReplyParams,
    ) -> Result<SendResult, MailboxError> {
        self.bound_member(bound_member_id)?;
        size(&params.to_id, MAX_ID_BYTES)?;
        let parent = self
            .store
            .get_message(&params.to_id)?
            .ok_or(MailboxError::NotFound)?;
        let header = &parent.message.header;
        if header.to_member_id != bound_member_id {
            return Err(MailboxError::NotFound);
        }
        if header.kind == KIND_DELIVERY_FAILED || header.from_member_id == OFFICE_SENDER {
            return Err(MailboxError::InvalidRequest);
        }
        let sent = self.send(
            bound_member_id,
            MsgSendParams {
                id: params.id,
                to: header.from_member_id.clone(),
                kind: KIND_REPLY.into(),
                priority: params.priority,
                subject: params.subject,
                body: params.body,
                meta: params.meta,
                reply_to: Some(params.to_id.clone()),
                attachment_ids: params.attachment_ids,
            },
        )?;
        let now = self.now_utc()?;
        let _ = self
            .store
            .claim_read(bound_member_id, Some(&params.to_id), &now);
        Ok(sent)
    }

    /// `msg.list`: list message headers visible to the caller (agent sees only messages it
    /// sent or received; grant-bound owner/app sees all or filtered by `participant`).
    pub fn list(
        &self,
        bound_member_id: &str,
        is_app_bound: bool,
        params: MsgListParams,
    ) -> Result<HeaderPage, MailboxError> {
        self.bound_member(bound_member_id)?;
        optional_size(params.thread_id.as_deref(), MAX_ID_BYTES)?;
        let limit = params.limit.unwrap_or(DEFAULT_PAGE_LIMIT);
        if limit > MAX_PAGE_LIMIT {
            return Err(MailboxError::InvalidRequest);
        }
        let participant = match params.participant.as_deref() {
            Some(reference) => {
                let Some(trimmed) = address(reference)? else {
                    return Err(MailboxError::NotFound);
                };
                let (member, _) = self
                    .directory
                    .resolve_member(trimmed)?
                    .ok_or(MailboxError::NotFound)?;
                Some(member.id)
            }
            None => None,
        };
        let scope = if is_app_bound {
            match participant {
                Some(id) => HeaderScope::Participant(id),
                None => HeaderScope::All,
            }
        } else {
            if let Some(id) = &participant {
                if id != bound_member_id {
                    return Err(MailboxError::InvalidRequest);
                }
            }
            HeaderScope::Participant(bound_member_id.to_owned())
        };
        Ok(self.store.list_headers(HeaderQuery {
            scope,
            from_member_id: None,
            thread_id: params.thread_id,
            unread_only: params.unread_only,
            include_snoozed: params.include_snoozed,
            cursor: params.cursor,
            limit,
        })?)
    }

    /// `msg.get`: read a single message without changing its lifecycle state.
    pub fn get(
        &self,
        bound_member_id: &str,
        is_app_bound: bool,
        params: MsgGetParams,
    ) -> Result<MsgGetResult, MailboxError> {
        self.bound_member(bound_member_id)?;
        size(&params.id, MAX_ID_BYTES)?;
        let stored = self
            .store
            .get_message(&params.id)?
            .ok_or(MailboxError::NotFound)?;
        let header = &stored.message.header;
        if !is_app_bound
            && header.from_member_id != bound_member_id
            && header.to_member_id != bound_member_id
        {
            return Err(MailboxError::NotFound);
        }
        Ok(MsgGetResult {
            message: stored.message,
        })
    }
}
