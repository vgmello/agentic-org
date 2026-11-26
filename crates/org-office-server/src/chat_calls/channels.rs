//! Channel calls: leaving, summaries, the watching model, and opening a channel watch on a channel ask.

use org_proto::chat::{
    ChatChannelLeave, ChatChannelSummarize, ChatWatchingModelGet, ChatWatchingModelSet,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use super::{invalid_params, ok_reply, storage_err, ChatService};

impl ChatService {
    pub(super) fn handle_channel_leave(&self, member: &str, request: &Request) -> Response {
        let Ok(params) = request.params::<ChatChannelLeave>() else {
            return invalid_params(request.id, ChatChannelLeave::METHOD);
        };
        match self.office.set_channel_membership(member, params, false) {
            Ok(Ok(res)) => {
                self.watcher.on_member_left_channel(member, &res.channel.id);
                ok_reply::<ChatChannelLeave>(request.id, &res)
            }
            Ok(Err(refusal)) => Response::error(request.id, refusal),
            Err(error) => {
                crate::logged(ChatChannelLeave::METHOD, &error);
                storage_err(request.id)
            }
        }
    }

    pub(super) fn handle_channel_summarize(&self, member: &str, request: &Request) -> Response {
        let Ok(params) = request.params::<ChatChannelSummarize>() else {
            return invalid_params(request.id, ChatChannelSummarize::METHOD);
        };
        match self.watcher.summarize_channel(member, &params) {
            Ok(res) => ok_reply::<ChatChannelSummarize>(request.id, &res),
            Err(refusal) => Response::error(request.id, refusal),
        }
    }

    pub(super) fn handle_watching_model_get(&self, request: &Request) -> Response {
        match self.office.watching_model_view() {
            Ok(res) => ok_reply::<ChatWatchingModelGet>(request.id, &res),
            Err(error) => {
                crate::logged(ChatWatchingModelGet::METHOD, &error);
                storage_err(request.id)
            }
        }
    }

    pub(super) fn handle_watching_model_set(
        &self,
        member: &str,
        via_grant: bool,
        request: &Request,
    ) -> Response {
        if !via_grant {
            return Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::NotAuthorized,
                    "only the authenticated organization owner may change the watching model",
                ),
            );
        }
        let Ok(params) = request.params::<ChatWatchingModelSet>() else {
            return invalid_params(request.id, ChatWatchingModelSet::METHOD);
        };
        match self.office.set_watching_model_setting(
            member,
            &params.command_id,
            params.model_profile_id.as_deref(),
        ) {
            Ok(Ok(res)) => ok_reply::<ChatWatchingModelSet>(request.id, &res),
            Ok(Err(refusal)) => Response::error(request.id, refusal),
            Err(error) => {
                crate::logged(ChatWatchingModelSet::METHOD, &error);
                storage_err(request.id)
            }
        }
    }

    pub(super) fn open_channel_watch_if_applicable(
        &self,
        member: &str,
        committed: &org_office::messaging::asks::AskCommitted,
    ) -> (Option<bool>, Option<String>) {
        if !committed.inserted {
            return (None, None);
        }
        let conv_kind = self
            .office
            .conversation_by_id(&committed.result.conversation_id)
            .ok()
            .flatten()
            .map_or(org_proto::chat::ConversationKind::Dm, |c| c.kind);
        if conv_kind != org_proto::chat::ConversationKind::Channel {
            return (None, None);
        }
        let q_text = committed
            .card
            .questions
            .iter()
            .map(|q| q.prompt.as_str())
            .collect::<Vec<_>>()
            .join(" / ");
        let (watched, reason) = self.watcher.on_channel_ask_opened(
            member,
            &committed.result.ask_id,
            &committed.result.conversation_id,
            conv_kind,
            &q_text,
        );
        (Some(watched), reason)
    }
}
