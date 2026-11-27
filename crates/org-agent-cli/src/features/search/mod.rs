//! The search vertical slice (`TASK-91` AC #3): `agent-os search query` calling `search.query`.

use org_command::{Answer, CommandTable, Refusal, Request};
use org_proto::search::{
    SearchEntityType, SearchHitField, SearchQuery, SearchQueryParams, SearchQueryResult,
    SEARCH_MAX_LIMIT,
};
use org_proto::{Call, ErrorCode};
use serde_json::{json, Value};

use crate::runtime::domain::{CommandCue, Domain, ResourceCue};
use crate::runtime::input::{is_set, opt_str_list, opt_string, require_str};
use crate::runtime::transport::OfficeTransport;

const RESOURCES: &[ResourceCue] = &[(
    "search",
    "full-text BM25 search across tasks, processes, projects, directory, readable messages, or your private notes/decisions.",
)];

const CUES: &[CommandCue] = &[(
    "search",
    "query",
    "searching tasks, processes, projects, members, teams, roles, messages, or private notes/decisions (`--types private_note|private_decision` alone); reports truthful `meta.coverage`.",
)];

pub(crate) const DOMAIN: Domain = Domain {
    commands: org_command_catalogue::search::ALL,
    resources: RESOURCES,
    cues: &[CUES],
    dispatch: |transport, request, _table: &CommandTable| match (
        request.command.resource,
        request.command.verb,
    ) {
        ("search", "query") => Some(read_search_query(transport, request)),
        _ => None,
    },
};

fn invalid(message: impl Into<String>, hint: &str) -> Refusal {
    Refusal::new(ErrorCode::InvalidRequest, message, hint)
}

fn parse_types(raw: Option<Vec<String>>) -> Result<Vec<SearchEntityType>, Refusal> {
    let mut parsed = Vec::new();
    for item in raw.unwrap_or_default() {
        let kind = SearchEntityType::parse(&item).ok_or_else(|| {
            invalid(
                format!("invalid entity type `{item}`"),
                "Use task, process, project, member, team, role, message, private_note or private_decision.",
            )
        })?;
        parsed.push(kind);
    }
    let private_count = parsed.iter().filter(|k| k.is_private()).count();
    if private_count > 0 && private_count < parsed.len() {
        return Err(invalid(
            "private_note and private_decision are searched on their own, never beside shared knowledge.",
            "Pass only private types (`--types private_note` / `--types private_decision`), or only shared/message types.",
        ));
    }
    Ok(parsed)
}

/// Maps requested `--fields` roots to server-side `SearchHitField` pruning hints.
/// When the caller requests only always-present identity fields (`entity_type`, `entity_id`),
/// requests `Score` so the office skips excerpt/title construction before `org-command` projects.
fn server_fields(roots: Option<&[String]>) -> Vec<SearchHitField> {
    let Some(roots) = roots else {
        return Vec::new();
    };
    let mapped: Vec<SearchHitField> = roots
        .iter()
        .filter_map(|root| SearchHitField::parse(root))
        .collect();
    if mapped.is_empty() {
        vec![SearchHitField::Score]
    } else {
        mapped
    }
}

fn read_search_query<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let i = &request.input;
    let count_only = is_set(i, "count");
    if count_only && request.cursor.is_some() {
        return Err(invalid(
            "--count answers no page, so it takes no --cursor.",
            "Run `search query --q ... --count` without --cursor.",
        ));
    }
    if let Some(limit) = request.limit {
        if limit == 0 || limit > SEARCH_MAX_LIMIT {
            return Err(invalid(
                format!("--limit for `search query` is 1 to {SEARCH_MAX_LIMIT}."),
                "Pass --limit between 1 and 100.",
            ));
        }
    }
    let params = SearchQueryParams {
        q: require_str(i, "q")?.to_owned(),
        types: parse_types(opt_str_list(i, "types"))?,
        project: opt_string(i, "project"),
        channel: opt_string(i, "channel"),
        status: opt_string(i, "status"),
        modified_file: opt_string(i, "modified_file"),
        assignee: opt_string(i, "assignee"),
        label: opt_string(i, "label"),
        author_id: opt_string(i, "author_id"),
        count_only,
        limit: request.limit,
        cursor: request.cursor.clone(),
        fields: server_fields(request.fields.as_deref()),
    };
    let raw = transport.call_method(SearchQuery::METHOD, json!(params))?;
    let res: SearchQueryResult = serde_json::from_value(raw).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            "office returned an unreadable `search.query` reply",
            "Retry the command or check the office version.",
        )
    })?;
    let hits: Vec<Value> = res.hits.into_iter().map(|hit| json!(hit)).collect();
    let mut answer = Answer::completed(Value::Array(hits));
    answer.total = Some(res.total);
    answer.next_cursor = res.next_cursor;
    answer.coverage = Some(json!(res.coverage));
    Ok(answer)
}

#[cfg(test)]
mod tests;
