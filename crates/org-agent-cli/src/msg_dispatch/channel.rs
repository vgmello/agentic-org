//! `channel` commands: listing, membership and summaries.

use org_command::{Answer, Refusal, Request};
use org_proto::chat::ChatChannelsResult;
use serde_json::{json, Map, Value};

use super::{
    decode_err, insert_opt, opt_bool, opt_non_neg_u64, reject_dry_run, require_str, with_ack,
};
use crate::transport::OfficeTransport;

pub(super) fn try_channel<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    match request.command.verb {
        "list" => Some(read_channel_list(transport, request)),
        "join" => Some(write_channel_membership(transport, request, true)),
        "leave" => Some(write_channel_membership(transport, request, false)),
        "summarize" => Some(read_channel_summarize(transport, request)),
        _ => None,
    }
}

pub(super) fn read_channel_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let include_archived = opt_bool(request, "include_archived").unwrap_or(false);
    let raw = transport.call_method(
        "chat.channels",
        json!({ "include_archived": include_archived }),
    )?;
    let res: ChatChannelsResult =
        serde_json::from_value(raw).map_err(|_| decode_err("chat.channels"))?;
    let items: Vec<Value> = res
        .channels
        .into_iter()
        .map(|c| {
            let mut o = Map::new();
            o.insert("id".into(), Value::String(c.id));
            o.insert(
                "kind".into(),
                serde_json::to_value(c.kind).unwrap_or(Value::String("channel".into())),
            );
            insert_opt(&mut o, "name", c.name.as_deref());
            if let Some(scope) = c.auto_scope {
                o.insert(
                    "auto_scope".into(),
                    serde_json::to_value(scope).unwrap_or(Value::Null),
                );
            }
            o.insert("archived".into(), Value::Bool(c.archived));
            let active_count = c.members.iter().filter(|m| m.left_at.is_none()).count();
            o.insert("member_count".into(), Value::from(active_count));
            o.insert("is_member".into(), Value::Bool(c.is_member));
            Value::Object(o)
        })
        .collect();
    Ok(Answer::completed(Value::Array(items)))
}

pub(super) fn write_channel_membership<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    join: bool,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let channel = require_str(request, "channel")?;
    let method = if join {
        "chat.channel_join"
    } else {
        "chat.channel_leave"
    };
    let raw = transport.call_method(method, json!({ "channel": channel }))?;
    let res: org_proto::chat::ChatChannelMembershipResult =
        serde_json::from_value(raw).map_err(|_| decode_err(method))?;
    let mut obj = Map::new();
    obj.insert("conversation_id".into(), Value::String(res.channel.id));
    insert_opt(&mut obj, "name", res.channel.name.as_deref());
    obj.insert("is_member".into(), Value::Bool(res.joined));
    Ok(with_ack(Answer::completed(Value::Object(obj)), request))
}

pub(super) fn read_channel_summarize<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let channel = require_str(request, "channel")?;
    let question = require_str(request, "question")?;
    let mut params = Map::new();
    params.insert("channel".into(), Value::String(channel.to_owned()));
    params.insert("question".into(), Value::String(question.to_owned()));
    if let Some(limit) = opt_non_neg_u64(request, "max_messages")? {
        params.insert("limit".into(), Value::from(limit.clamp(1, 200)));
    }
    let timeout = std::time::Duration::from_secs(75);
    let raw = transport.call_method_with_timeout(
        "chat.channel_summarize",
        Value::Object(params),
        timeout,
    )?;
    let res: org_proto::chat::ChatChannelSummarizeResult =
        serde_json::from_value(raw).map_err(|_| decode_err("chat.channel_summarize"))?;
    let mut obj = Map::new();
    obj.insert("conversation_id".into(), Value::String(res.conversation_id));
    obj.insert("summary".into(), Value::String(res.summary));
    obj.insert(
        "message_ids".into(),
        Value::Array(res.message_ids.into_iter().map(Value::String).collect()),
    );
    obj.insert("derived_notice".into(), Value::String(res.derived_notice));
    insert_opt(&mut obj, "usage_record_id", res.usage_record_id.as_deref());
    Ok(Answer::completed(Value::Object(obj)))
}
