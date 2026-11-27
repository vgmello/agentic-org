//! `chat.events_poll`: organization changes, live events, and the bounded long-poll wait.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::live::Live;
use org_proto::chat::{ChatEventsPoll, ChatEventsPollResult};
use org_proto::{Call, Request, Response};

use super::ask::ASK_WAIT_TICK;
use super::{invalid_params, ok_reply, storage_err, ChatService};

impl ChatService {
    fn poll_org_changes(
        &self,
        is_owner: bool,
        after_org_seq: Option<u64>,
        org_head_seq: u64,
    ) -> Result<(Vec<org_proto::org::OrgChangedData>, bool), org_office::OfficeError> {
        match after_org_seq {
            Some(after) if is_owner && after > org_head_seq => Ok((Vec::new(), true)),
            Some(after) if is_owner && after < org_head_seq => {
                Ok((self.office.org_changes_after(after, 100)?, false))
            }
            _ => Ok((Vec::new(), false)),
        }
    }

    fn collect_live_events(
        &self,
        member: &str,
        is_owner: bool,
        via_grant: bool,
        after_live_seq: Option<u64>,
        live_head: u64,
    ) -> (
        Vec<org_proto::msg::MessageReceivedData>,
        Vec<org_proto::msg::MessageStateChangedData>,
        Vec<org_proto::inbox::InboxChangedData>,
        Vec<u64>,
        bool,
    ) {
        let mut messages_received = Vec::new();
        let mut message_states = Vec::new();
        let mut inbox_changes = Vec::new();
        let mut org_live_seqs = Vec::new();
        let Some(after) = after_live_seq else {
            return (
                messages_received,
                message_states,
                inbox_changes,
                org_live_seqs,
                false,
            );
        };
        let batch = self.live.since(after, live_head);
        for ev in batch.events {
            match ev {
                Live::Received(data) if is_owner || data.header.to_member_id == member => {
                    messages_received.push(*data);
                }
                Live::StateChanged(data) if is_owner || data.to_member_id == member => {
                    message_states.push(data);
                }
                Live::InboxChanged(data) if is_owner => inbox_changes.push(data),
                Live::OrgChanged(seq) if via_grant => org_live_seqs.push(seq),
                _ => {}
            }
        }
        (
            messages_received,
            message_states,
            inbox_changes,
            org_live_seqs,
            batch.lost,
        )
    }

    fn collect_events_poll_result(
        &self,
        member: &str,
        is_owner: bool,
        via_grant: bool,
        params: &org_proto::chat::ChatEventsPollParams,
    ) -> Result<ChatEventsPollResult, org_office::OfficeError> {
        let snapshot = self.office.org_snapshot()?;
        let org_head_seq = snapshot.position.seq;
        let (mut org_changes, snapshot_required) =
            self.poll_org_changes(is_owner, params.after_org_seq, org_head_seq)?;
        let head = self.live.head();
        let (messages_received, message_states, inbox_changes, org_live_seqs, live_lost) = self
            .collect_live_events(
                member,
                is_owner,
                via_grant,
                params.after_live_seq,
                head.messages,
            );
        for seq in org_live_seqs {
            org_changes.push(org_proto::org::OrgChangedData {
                position: snapshot.position,
                command_id: format!("live-org:{seq}"),
                changes: Vec::new(),
                actor: None,
            });
        }
        let presence_changes = params
            .after_presence_seq
            .map(|after| self.live.presence_since(after, head.presence).0)
            .unwrap_or_default();
        // Agents' coarse live activity rides the presence cursor (`TASK-103`).
        let activity_changes = params
            .after_presence_seq
            .map(|after| self.live.activity_since(after, head.presence, is_owner))
            .unwrap_or_default();
        Ok(ChatEventsPollResult {
            org_head_seq,
            live_head_seq: head.messages,
            presence_head_seq: head.presence,
            snapshot_required,
            live_lost,
            org_changes,
            messages_received,
            message_states,
            presence_changes,
            activity_changes,
            inbox_changes,
        })
    }

    pub(super) fn handle_events_poll(
        &self,
        member: &str,
        via_grant: bool,
        stopping: &AtomicBool,
        request: &Request,
    ) -> Response {
        let Ok(params) = request.params::<ChatEventsPoll>() else {
            return invalid_params(request.id, ChatEventsPoll::METHOD);
        };
        let is_owner = via_grant && member == self.owner_id;
        let deadline = events_poll_deadline(&params);
        loop {
            let token = self.feed_wake.as_ref().map(|w| w.token());
            let result = match self.collect_events_poll_result(member, is_owner, via_grant, &params)
            {
                Ok(res) => res,
                Err(error) => {
                    crate::logged("chat.events_poll.org", &error);
                    return storage_err(request.id);
                }
            };
            if events_poll_has_updates(&result) || stopping.load(Ordering::SeqCst) {
                return ok_reply::<ChatEventsPoll>(request.id, &result);
            }
            if !self.wait_events_poll_step(deadline, token) {
                return ok_reply::<ChatEventsPoll>(request.id, &result);
            }
        }
    }

    fn wait_events_poll_step(&self, deadline: Option<Instant>, token: Option<u64>) -> bool {
        let (Some(until), Some(wake), Some(seen)) = (deadline, self.feed_wake.as_ref(), token)
        else {
            return false;
        };
        let now = Instant::now();
        if now >= until {
            return false;
        }
        let step = until.saturating_duration_since(now).min(ASK_WAIT_TICK);
        let _ = wake.wait(seen, step);
        true
    }
}

fn events_poll_deadline(params: &org_proto::chat::ChatEventsPollParams) -> Option<Instant> {
    let has_cursor = params.after_org_seq.is_some()
        || params.after_live_seq.is_some()
        || params.after_presence_seq.is_some();
    let wait_ms = params
        .wait_ms
        .unwrap_or(0)
        .min(org_proto::chat::MAX_EVENTS_POLL_WAIT_MS);
    (has_cursor && wait_ms > 0).then(|| Instant::now() + Duration::from_millis(wait_ms))
}

fn events_poll_has_updates(result: &ChatEventsPollResult) -> bool {
    result.snapshot_required
        || result.live_lost
        || !result.org_changes.is_empty()
        || !result.messages_received.is_empty()
        || !result.message_states.is_empty()
        || !result.presence_changes.is_empty()
        || !result.activity_changes.is_empty()
        || !result.inbox_changes.is_empty()
}
