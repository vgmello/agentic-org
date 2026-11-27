//! Browser HTTP routes for B6 conversations, channels, direct messages, reactions,
//! read markers, mute preferences, handoff acceptance, triage, and owner inbox (`TASK-45`).

use hyper::{http::request::Parts, Method, StatusCode};
use org_proto::ask::{
    ChatAnswer, ChatAnswerParams, ChatAsk, ChatAskParams, ChatWaitAsk, ChatWaitAskParams,
    MsgWithdraw, MsgWithdrawParams,
};
use org_proto::chat::{
    ChatChannelCreate, ChatChannelCreateParams, ChatChannelJoin, ChatChannelLeave,
    ChatChannelMembershipParams, ChatChannels, ChatChannelsParams, ChatConversations,
    ChatConversationsParams, ChatInbox, ChatInboxParams, ChatMarkRead, ChatMarkReadParams,
    ChatMessages, ChatMessagesParams, ChatMute, ChatMuteParams, ChatReact, ChatReactParams,
    ChatSend, ChatSendParams, DmOpen, DmOpenParams, MsgAccept, MsgAcceptParams,
};
use org_proto::inbox::{InboxList, InboxListParams};
use org_proto::msg::{
    MsgDefer, MsgDeferParams, MsgDone, MsgDoneParams, MsgReply, MsgReplyParams, MsgUnsnooze,
    MsgUnsnoozeParams,
};
use org_proto::web::{ReadResponse, WebError, WebErrorCode};
use serde_json::Value;

use super::hq_context::{resolve_hq_office, revalidate_and_resolve_hq};
use super::{
    failure, invalid, not_found, result, unavailable, Call, CallerAuth, Route, SessionApi,
};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::service::{ApiReply, PreparedCall};

pub enum ChatRoute {
    Channels(CallerAuth, String),
    ChannelCreate(CallerAuth, String),
    ChannelJoin(CallerAuth, String),
    ChannelLeave(CallerAuth, String),
    Conversations(CallerAuth, String),
    DmOpen(CallerAuth, String),
    Send(CallerAuth, String),
    Messages(CallerAuth, String),
    Inbox(CallerAuth, String),
    MarkRead(CallerAuth, String),
    Mute(CallerAuth, String),
    React(CallerAuth, String),
    Accept(CallerAuth, String),
    Reply(CallerAuth, String),
    Done(CallerAuth, String),
    Defer(CallerAuth, String),
    Unsnooze(CallerAuth, String),
    Ask(CallerAuth, String),
    Answer(CallerAuth, String),
    Withdraw(CallerAuth, String),
    WaitAsk(CallerAuth, String),
    OwnerInbox(CallerAuth, String),
}

pub enum ChatCall {
    Channels(CallerAuth, String),
    ChannelCreate(CallerAuth, String, ChatChannelCreateParams),
    ChannelJoin(CallerAuth, String, ChatChannelMembershipParams),
    ChannelLeave(CallerAuth, String, ChatChannelMembershipParams),
    Conversations(CallerAuth, String),
    DmOpen(CallerAuth, String, DmOpenParams),
    Send(CallerAuth, String, Box<ChatSendParams>),
    Messages(CallerAuth, String, ChatMessagesParams),
    Inbox(CallerAuth, String, ChatInboxParams),
    MarkRead(CallerAuth, String, ChatMarkReadParams),
    Mute(CallerAuth, String, ChatMuteParams),
    React(CallerAuth, String, ChatReactParams),
    Accept(CallerAuth, String, MsgAcceptParams),
    Reply(CallerAuth, String, Box<MsgReplyParams>),
    Done(CallerAuth, String, MsgDoneParams),
    Defer(CallerAuth, String, MsgDeferParams),
    Unsnooze(CallerAuth, String, MsgUnsnoozeParams),
    Ask(CallerAuth, String, Box<ChatAskParams>),
    Answer(CallerAuth, String, Box<ChatAnswerParams>),
    Withdraw(CallerAuth, String, MsgWithdrawParams),
    WaitAsk(CallerAuth, String, ChatWaitAskParams),
    OwnerInbox(CallerAuth, String),
}

fn valid_org_id(id: &str) -> Result<(), ApiReply> {
    org_office_launch::is_storage_id(id)
        .then_some(())
        .ok_or_else(invalid)
}

fn auth_org(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    build: fn(CallerAuth, String) -> ChatRoute,
) -> Result<Route, ApiReply> {
    let pair = api.caller_auth(request)?;
    valid_org_id(org)?;
    Ok(Route::Chat(build(pair, org.to_owned())))
}

/// Attempts to route B6 chat & owner-inbox paths under `/api/v1/organizations/{org}/...`.
pub(super) fn try_route(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
    _get: bool,
    post: bool,
    query: bool,
) -> Option<Result<Route, ApiReply>> {
    let is_get = matches!(request.method, Method::GET | Method::HEAD) && !query;
    match tail {
        ["inbox"] if is_get => Some(auth_org(api, request, org, ChatRoute::OwnerInbox)),
        ["chat", rest @ ..] => route_chat(api, request, org, rest, is_get, post),
        _ => None,
    }
}

fn route_chat(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    rest: &[&str],
    is_get: bool,
    post: bool,
) -> Option<Result<Route, ApiReply>> {
    match rest {
        ["channels"] if is_get => Some(auth_org(api, request, org, ChatRoute::Channels)),
        ["channels"] if post => Some(auth_org(api, request, org, ChatRoute::ChannelCreate)),
        ["channels", "join"] if post => Some(auth_org(api, request, org, ChatRoute::ChannelJoin)),
        ["channels", "leave"] if post => Some(auth_org(api, request, org, ChatRoute::ChannelLeave)),
        ["conversations"] if is_get => Some(auth_org(api, request, org, ChatRoute::Conversations)),
        ["dm"] if post => Some(auth_org(api, request, org, ChatRoute::DmOpen)),
        ["messages"] if post => Some(auth_org(api, request, org, ChatRoute::Send)),
        ["messages", "list"] if post => Some(auth_org(api, request, org, ChatRoute::Messages)),
        ["inbox"] if post => Some(auth_org(api, request, org, ChatRoute::Inbox)),
        ["read"] if post => Some(auth_org(api, request, org, ChatRoute::MarkRead)),
        ["mute"] if post => Some(auth_org(api, request, org, ChatRoute::Mute)),
        ["react"] if post => Some(auth_org(api, request, org, ChatRoute::React)),
        ["accept"] if post => Some(auth_org(api, request, org, ChatRoute::Accept)),
        ["reply"] if post => Some(auth_org(api, request, org, ChatRoute::Reply)),
        ["done"] if post => Some(auth_org(api, request, org, ChatRoute::Done)),
        ["defer"] if post => Some(auth_org(api, request, org, ChatRoute::Defer)),
        ["unsnooze"] if post => Some(auth_org(api, request, org, ChatRoute::Unsnooze)),
        ["ask"] if post => Some(auth_org(api, request, org, ChatRoute::Ask)),
        ["ask", "wait"] if post => Some(auth_org(api, request, org, ChatRoute::WaitAsk)),
        ["answer"] if post => Some(auth_org(api, request, org, ChatRoute::Answer)),
        ["withdraw"] if post => Some(auth_org(api, request, org, ChatRoute::Withdraw)),
        _ => None,
    }
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ApiReply> {
    serde_json::from_slice(bytes).map_err(|_| invalid())
}

fn empty(bytes: &[u8]) -> Result<(), ApiReply> {
    if bytes.is_empty() {
        Ok(())
    } else {
        Err(invalid())
    }
}

pub(super) fn prepare(route: ChatRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    let call = prepare_chat_call(route, bytes)?;
    Ok(PreparedCall::read(Call::Chat(call), bytes.len()))
}

fn prepare_chat_call(route: ChatRoute, bytes: &[u8]) -> Result<ChatCall, ApiReply> {
    let call = match route {
        ChatRoute::Channels(c, o) => {
            empty(bytes)?;
            ChatCall::Channels(c, o)
        }
        ChatRoute::ChannelCreate(c, o) => ChatCall::ChannelCreate(c, o, decode(bytes)?),
        ChatRoute::ChannelJoin(c, o) => ChatCall::ChannelJoin(c, o, decode(bytes)?),
        ChatRoute::ChannelLeave(c, o) => ChatCall::ChannelLeave(c, o, decode(bytes)?),
        ChatRoute::Conversations(c, o) => {
            empty(bytes)?;
            ChatCall::Conversations(c, o)
        }
        ChatRoute::DmOpen(c, o) => ChatCall::DmOpen(c, o, decode(bytes)?),
        ChatRoute::Send(c, o) => ChatCall::Send(c, o, Box::new(decode(bytes)?)),
        ChatRoute::Messages(c, o) => ChatCall::Messages(c, o, decode(bytes)?),
        ChatRoute::Inbox(c, o) => ChatCall::Inbox(c, o, decode(bytes)?),
        ChatRoute::MarkRead(c, o) => ChatCall::MarkRead(c, o, decode(bytes)?),
        ChatRoute::Mute(c, o) => ChatCall::Mute(c, o, decode(bytes)?),
        ChatRoute::React(c, o) => ChatCall::React(c, o, decode(bytes)?),
        ChatRoute::Accept(c, o) => ChatCall::Accept(c, o, decode(bytes)?),
        ChatRoute::Reply(c, o) => ChatCall::Reply(c, o, Box::new(decode(bytes)?)),
        ChatRoute::Done(c, o) => ChatCall::Done(c, o, decode(bytes)?),
        ChatRoute::Defer(c, o) => {
            let mut p: MsgDeferParams = decode(bytes)?;
            if let org_proto::msg::SnoozeTarget::Until { at } = &mut p.until {
                *at = org_proto::canonicalize_rfc3339_utc(at).ok_or_else(invalid)?;
            }
            ChatCall::Defer(c, o, p)
        }
        ChatRoute::Unsnooze(c, o) => ChatCall::Unsnooze(c, o, decode(bytes)?),
        ChatRoute::Ask(c, o) => {
            let mut p: ChatAskParams = decode(bytes)?;
            p.hold_ms = Some(0);
            ChatCall::Ask(c, o, Box::new(p))
        }
        ChatRoute::Answer(c, o) => ChatCall::Answer(c, o, Box::new(decode(bytes)?)),
        ChatRoute::Withdraw(c, o) => ChatCall::Withdraw(c, o, decode(bytes)?),
        ChatRoute::WaitAsk(c, o) => {
            let mut p: ChatWaitAskParams = decode(bytes)?;
            p.hold_ms = Some(0);
            ChatCall::WaitAsk(c, o, p)
        }
        ChatRoute::OwnerInbox(c, o) => {
            empty(bytes)?;
            ChatCall::OwnerInbox(c, o)
        }
    };
    Ok(call)
}

async fn run_office<C: org_proto::Call>(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
    params: &C::Params,
    is_read: bool,
) -> ApiReply {
    if let Err(error) = api.validate_async(pair).await {
        return failure(error);
    }
    let mut office_id = match resolve_hq_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(Failure::Refused(_)) => return unavailable(),
        Err(error) => return failure(error),
    };
    let auth = api.auth(&pair);
    let mut reply = api
        .offices
        .call_typed::<C>(&auth, &office_id, params, is_read)
        .await;
    if matches!(
        &reply,
        Err(OfficeFailure::Refused(e)) if e.code == org_proto::ErrorCode::NotAuthorized
    ) {
        office_id = match revalidate_and_resolve_hq(api, pair, org_id).await {
            Ok(id) => id,
            Err(Failure::Refused(_)) => return unavailable(),
            Err(error) => return failure(error),
        };
        reply = api
            .offices
            .call_typed::<C>(&auth, &office_id, params, is_read)
            .await;
    }
    match reply {
        Ok(res) => result(res),
        Err(err) => map_office_failure(err),
    }
}

fn map_office_failure(error: OfficeFailure) -> ApiReply {
    match error {
        OfficeFailure::BeforeDispatch(e) => failure(e),
        OfficeFailure::Transport => unavailable(),
        OfficeFailure::Refused(err) => {
            let web = WebError::from(err);
            let status = match web.code {
                WebErrorCode::InvalidRequest
                | WebErrorCode::InvalidName
                | WebErrorCode::Cycle
                | WebErrorCode::UnknownRecipient => StatusCode::BAD_REQUEST,
                WebErrorCode::NotAuthorized | WebErrorCode::OwnerProtected => StatusCode::FORBIDDEN,
                WebErrorCode::NotFound => return not_found(),
                WebErrorCode::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
                WebErrorCode::Conflict
                | WebErrorCode::AlreadyExists
                | WebErrorCode::NotEmpty
                | WebErrorCode::OperationReused => StatusCode::CONFLICT,
                WebErrorCode::Expired => StatusCode::GONE,
                _ => return unavailable(),
            };
            let payload = ReadResponse::<Value>::Error(web);
            ApiReply::json(
                status,
                serde_json::to_value(payload).expect("gateway error serializes"),
            )
        }
    }
}

pub(super) async fn execute(api: &SessionApi, call: ChatCall) -> ApiReply {
    match call {
        ChatCall::Channels(c, o) => {
            run_office::<ChatChannels>(api, c, &o, &ChatChannelsParams::default(), true).await
        }
        ChatCall::ChannelCreate(c, o, p) => {
            run_office::<ChatChannelCreate>(api, c, &o, &p, false).await
        }
        ChatCall::ChannelJoin(c, o, p) => {
            run_office::<ChatChannelJoin>(api, c, &o, &p, false).await
        }
        ChatCall::ChannelLeave(c, o, p) => {
            run_office::<ChatChannelLeave>(api, c, &o, &p, false).await
        }
        ChatCall::Conversations(c, o) => {
            run_office::<ChatConversations>(api, c, &o, &ChatConversationsParams::default(), true)
                .await
        }
        ChatCall::DmOpen(c, o, p) => run_office::<DmOpen>(api, c, &o, &p, false).await,
        ChatCall::Send(c, o, p) => run_office::<ChatSend>(api, c, &o, &p, false).await,
        ChatCall::Messages(c, o, p) => run_office::<ChatMessages>(api, c, &o, &p, true).await,
        ChatCall::Inbox(c, o, p) => run_office::<ChatInbox>(api, c, &o, &p, true).await,
        ChatCall::MarkRead(c, o, p) => run_office::<ChatMarkRead>(api, c, &o, &p, false).await,
        ChatCall::Mute(c, o, p) => run_office::<ChatMute>(api, c, &o, &p, false).await,
        ChatCall::React(c, o, p) => run_office::<ChatReact>(api, c, &o, &p, false).await,
        ChatCall::Accept(c, o, p) => run_office::<MsgAccept>(api, c, &o, &p, false).await,
        ChatCall::Reply(c, o, p) => run_office::<MsgReply>(api, c, &o, &p, false).await,
        ChatCall::Done(c, o, p) => run_office::<MsgDone>(api, c, &o, &p, false).await,
        ChatCall::Defer(c, o, p) => run_office::<MsgDefer>(api, c, &o, &p, false).await,
        ChatCall::Unsnooze(c, o, p) => run_office::<MsgUnsnooze>(api, c, &o, &p, false).await,
        ChatCall::Ask(c, o, p) => run_office::<ChatAsk>(api, c, &o, &p, false).await,
        ChatCall::Answer(c, o, p) => run_office::<ChatAnswer>(api, c, &o, &p, false).await,
        ChatCall::Withdraw(c, o, p) => run_office::<MsgWithdraw>(api, c, &o, &p, false).await,
        ChatCall::WaitAsk(c, o, p) => run_office::<ChatWaitAsk>(api, c, &o, &p, true).await,
        ChatCall::OwnerInbox(c, o) => {
            let params = InboxListParams {
                state: org_proto::inbox::InboxState::Open,
                kinds: None,
                project: None,
                team: None,
                cursor: None,
                limit: None,
                has_file: None,
                has_image: None,
            };
            run_office::<InboxList>(api, c, &o, &params, true).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::CallerAuth;

    fn dummy_auth() -> CallerAuth {
        CallerAuth::LocalAuthless
    }

    #[test]
    fn defer_canonicalizes_rfc3339_and_unsnooze_and_snoozed_inbox_prepare() {
        let defer_bytes = br#"{"id":"0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa","until":{"type":"until","at":"2026-11-23T11:00:00+02:00"}}"#;
        let Ok(ChatCall::Defer(_, _, p)) =
            prepare_chat_call(ChatRoute::Defer(dummy_auth(), "org-1".into()), defer_bytes)
        else {
            panic!("expected ChatCall::Defer");
        };
        assert_eq!(
            p.until,
            org_proto::msg::SnoozeTarget::Until {
                at: "2026-11-23T09:00:00.000Z".into()
            }
        );

        let bad_defer = br#"{"id":"0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa","until":{"type":"until","at":"not-a-timestamp"}}"#;
        assert!(
            prepare_chat_call(ChatRoute::Defer(dummy_auth(), "org-1".into()), bad_defer).is_err()
        );

        let unsnooze_bytes = br#"{"id":"0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa"}"#;
        let Ok(ChatCall::Unsnooze(_, _, up)) = prepare_chat_call(
            ChatRoute::Unsnooze(dummy_auth(), "org-1".into()),
            unsnooze_bytes,
        ) else {
            panic!("expected ChatCall::Unsnooze");
        };
        assert_eq!(up.id, "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa");

        let inbox_bytes = br#"{"state":"snoozed","limit":10}"#;
        let Ok(ChatCall::Inbox(_, _, ip)) =
            prepare_chat_call(ChatRoute::Inbox(dummy_auth(), "org-1".into()), inbox_bytes)
        else {
            panic!("expected ChatCall::Inbox");
        };
        assert_eq!(
            ip.state,
            Some(org_proto::msg::MessageLifecycleState::Snoozed)
        );
    }
}
