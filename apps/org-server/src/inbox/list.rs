use super::{read, Hq, Keyset, Query, Source};
use org_proto::inbox::{InboxKind, InboxListParams, InboxListResult, InboxState};
use org_proto::{ApiError, ErrorCode};

fn invalid() -> ApiError {
    ApiError::new(ErrorCode::InvalidRequest, "invalid inbox list parameters")
}

impl Hq {
    pub(crate) fn list(&self, params: InboxListParams) -> Result<InboxListResult, ApiError> {
        let limit = params.limit.unwrap_or(50);
        if !(1..=100).contains(&limit)
            || !matches!(params.state, InboxState::Open | InboxState::Handled)
            || params
                .kinds
                .as_ref()
                .is_some_and(|kinds| kinds.iter().any(|kind| matches!(kind, InboxKind::Unknown)))
            || params
                .project
                .as_deref()
                .is_some_and(|id| !org_proto::org::is_valid_id(id))
            || params
                .team
                .as_deref()
                .is_some_and(|id| !org_proto::org::is_valid_id(id))
        {
            return Err(invalid());
        }
        let after = params.cursor.as_deref().map(decode).transpose()?;
        let sources: Vec<&dyn Source> = self.sources.iter().map(|source| source.as_ref()).collect();
        let query = Query {
            state: params.state,
            kinds: params.kinds.as_deref(),
            project: params.project.as_deref(),
            team: params.team.as_deref(),
            after: after.as_ref(),
            limit: limit as usize + 1,
        };
        let (as_of, mut result) = read(&self.clock, &sources, &query);
        let more = result.items.len() > limit as usize;
        result.items.truncate(limit as usize);
        let next_cursor = if more {
            result.items.last().map(|item| encode(&Keyset::of(item)))
        } else {
            None
        };
        Ok(InboxListResult {
            items: result.items,
            next_cursor,
            counts: result.totals.counts,
            as_of,
            degraded: result.totals.degraded,
        })
    }
}

fn encode(key: &Keyset) -> String {
    format!(
        "v1:{}:{}",
        key.created_at_ms
            .map_or_else(|| "-".into(), |time| time.to_string()),
        key.reference
    )
}

fn decode(cursor: &str) -> Result<Keyset, ApiError> {
    let (version, rest) = cursor.split_once(':').ok_or_else(invalid)?;
    let (time, reference) = rest.split_once(':').ok_or_else(invalid)?;
    if version != "v1" || reference.is_empty() {
        return Err(invalid());
    }
    let created_at_ms = if time == "-" {
        None
    } else {
        Some(time.parse().map_err(|_| invalid())?)
    };
    let key = Keyset {
        created_at_ms,
        reference: reference.into(),
    };
    if encode(&key) != cursor {
        return Err(invalid());
    }
    Ok(key)
}
