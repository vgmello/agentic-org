//! Message chat calls: `chat.send` and its committed finish, `msg.reply`, `msg.accept`, and `msg.done`.

use crate::live::Live;
use org_proto::chat::{header_only_received, ChatSend, MsgAccept};
use org_proto::{Call, ErrorCode, Request, Response};

use super::{invalid_params, ok_reply, storage_err, ChatService};

impl ChatService {
    fn finish_chat_send_committed(
        &self,
        member: &str,
        committed: &mut org_office::messaging::features::ChatSendCommitted,
    ) {
        if committed.inserted {
            let mut owner_touched = false;
            for header in committed.delivered_headers() {
                self.mailbox.notify_member(&header.to_member_id);
                if header.to_member_id == self.owner_id {
                    owner_touched = true;
                }
                self.live
                    .publish_live(Live::Received(Box::new(header_only_received(header))));
            }
            if owner_touched {
                self.live.record_mailbox_change();
            }
            self.watcher
                .on_channel_message_posted(&committed.message, committed.conversation_kind);
        }
        crate::federation::sync_native_message_relays(&self.office);
        self.notify_sat_sync();
        if let Some(sessions) = &self.sessions {
            let (woke, presence) = crate::wake_bridge::on_chat_send_committed(
                &self.office,
                sessions,
                member,
                committed,
            );
            committed.result.woke_agents = woke;
            committed.result.recipient_presence = presence;
        }
    }

    pub(super) fn handle_chat_send(&self, member: &str, request: &Request) -> Response {
        let Ok(params) = request.params::<ChatSend>() else {
            return invalid_params(request.id, ChatSend::METHOD);
        };
        if !params.to.trim().starts_with('#') {
            match self.office.check_hr_message_allowed(member, &params.to) {
                Ok(Ok(())) => {}
                Ok(Err(refusal)) => return Response::error(request.id, refusal),
                Err(error) => {
                    crate::logged("chat.send.hr_guard", &error);
                    return storage_err(request.id);
                }
            }
        }
        match self.office.chat_send(member, params) {
            Ok(Ok(mut committed)) => {
                self.finish_chat_send_committed(member, &mut committed);
                ok_reply::<ChatSend>(request.id, &committed.result)
            }
            Ok(Err(refusal)) => Response::error(request.id, refusal),
            Err(error) => {
                crate::logged("chat.send", &error);
                storage_err(request.id)
            }
        }
    }

    pub(super) fn handle_msg_reply(&self, member: &str, request: &Request) -> Response {
        let Ok(params) = request.params::<org_proto::msg::MsgReply>() else {
            return invalid_params(request.id, org_proto::msg::MsgReply::METHOD);
        };
        match self.office.chat_reply(member, params) {
            Ok(Ok(mut committed)) => {
                if member == self.owner_id && committed.inserted {
                    self.live.record_mailbox_change();
                }
                self.finish_chat_send_committed(member, &mut committed);
                let reply = org_proto::msg::SendResult {
                    id: committed.result.id,
                    thread_id: committed.result.thread_id,
                    local_seq: committed.result.local_seq,
                    origin_seq: committed.result.origin_seq,
                };
                ok_reply::<org_proto::msg::MsgReply>(request.id, &reply)
            }
            Ok(Err(refusal)) => Response::error(request.id, refusal),
            Err(error) => {
                crate::logged("msg.reply", &error);
                storage_err(request.id)
            }
        }
    }

    pub(super) fn handle_msg_accept(&self, member: &str, request: &Request) -> Response {
        let Ok(params) = request.params::<MsgAccept>() else {
            return invalid_params(request.id, MsgAccept::METHOD);
        };
        match self.office.msg_accept(member, params) {
            Ok(Ok(result)) => {
                if member == self.owner_id {
                    self.live.record_mailbox_change();
                }
                crate::federation::sync_native_message_relays(&self.office);
                self.notify_sat_sync();
                ok_reply::<MsgAccept>(request.id, &result)
            }
            Ok(Err(refusal)) => Response::error(request.id, refusal),
            Err(error) => {
                crate::logged("msg.accept", &error);
                storage_err(request.id)
            }
        }
    }

    pub(super) fn handle_msg_done(&self, member: &str, request: &Request) -> Response {
        let target_id = request
            .params::<org_proto::msg::MsgDone>()
            .ok()
            .map(|p| p.id);
        // Capture related question IDs before `mailbox.done` marks an `answer_candidate` done.
        let linked_qids: Vec<String> = target_id
            .as_deref()
            .and_then(|id| {
                self.office
                    .chat_inbox(
                        member,
                        org_proto::chat::ChatInboxParams {
                            include_all: true,
                            state: None,
                            because: None,
                            from: None,
                            thread_id: None,
                            cursor: None,
                            limit: Some(100),
                        },
                    )
                    .ok()
                    .and_then(Result::ok)
                    .map(|page| {
                        page.items
                            .into_iter()
                            .find(|i| i.id == id)
                            .map(|i| i.related_message_ids)
                            .unwrap_or_default()
                    })
            })
            .unwrap_or_default();

        let resp = crate::msg_calls::answer(request, Some(&self.mailbox), Some(member), false)
            .unwrap_or_else(|| crate::msg_calls::unsupported(request.id));
        match &resp.outcome {
            org_proto::Outcome::Result(_) => {
                // High #1: only close watches owned by `member` (`asker_id == member`).
                if let Some(id) = target_id.as_deref() {
                    let _ = self.watcher.on_question_done_by(member, id);
                }
                for qid in &linked_qids {
                    let _ = self.watcher.on_question_done_by(member, qid);
                }
                resp
            }
            org_proto::Outcome::Error(err) if err.code == ErrorCode::NotFound => {
                // High #1: an asker has no recipient state for their own channel question; allow
                // `msg.done` on their own watched question ID to close that watch cleanly.
                if let Some(id) = target_id.as_deref() {
                    if self.watcher.on_question_done_by(member, id) {
                        let resolved_at = org_office::format_rfc3339_ms(
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map_or(0, |d| d.as_millis() as u64),
                        );
                        return ok_reply::<org_proto::msg::MsgDone>(
                            request.id,
                            &org_proto::msg::MsgDoneResult {
                                id: id.to_owned(),
                                resolved_at,
                            },
                        );
                    }
                }
                resp
            }
            _ => resp,
        }
    }
}
