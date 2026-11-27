//! The universal reviews slice (`review`: 9 verbs, `TASK-92.1`).
//!
//! Owns `review request|list|inspect|diff|route|finding|verdict|resubmit|close`, strictly
//! self-contained with zero cross-feature imports (`vsa_structure`).

mod parse;
mod read;
#[cfg(test)]
mod tests;
mod write;

use org_command::{Answer, CommandTable, Refusal, Request};

use crate::runtime::domain::Domain;
use crate::runtime::transport::OfficeTransport;

/// The reviews slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: org_command_catalogue::reviews::ALL,
    resources: &[(
        "review",
        "requesting, listing, inspecting, diffing, routing, finding/replying on, submitting verdicts for, resubmitting, or closing universal reviews.",
    )],
    cues: &[write::CUES, read::CUES],
    dispatch,
};

fn dispatch(
    transport: &mut dyn OfficeTransport,
    request: &Request,
    _table: &CommandTable,
) -> Option<Result<Answer, Refusal>> {
    if request.command.resource != "review" {
        return None;
    }
    Some(match request.command.verb {
        "request" => write::write_review_request(transport, request),
        "list" => read::read_review_list(transport, request),
        "inspect" => read::read_review_inspect(transport, request),
        "diff" => read::read_review_diff(transport, request),
        "route" => write::write_review_route(transport, request),
        "finding" => write::write_review_finding(transport, request),
        "verdict" => write::write_review_verdict(transport, request),
        "resubmit" => write::write_review_resubmit(transport, request),
        "close" => write::write_review_close(transport, request),
        _ => return None,
    })
}
