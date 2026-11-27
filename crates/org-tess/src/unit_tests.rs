//! Unit tests for Ask Tess prompt/viewport sanitization, `TurnHub` replay/bounds/isolation,
//! and `suggestion.apply` revision reconciliation (`TASK-80.4` Part B).

use std::collections::BTreeMap;

use org_proto::tess::{TessViewportContext, UiEntityKind, MAX_VIEWPORT_CONTEXT_BYTES};
use org_proto::ErrorCode;

use crate::turns::ask_partition;
use crate::viewport::{compose_ask_instruction, validate_and_render_viewport};

#[test]
fn viewport_context_is_appended_strictly_last_behind_untrusted_fence() {
    let prompt = "What tasks are blocked right now?";
    let vp = r#"{"route":"/tasks/T-A1","entity_id":"T-A1"}"#;
    let combined = compose_ask_instruction(prompt, Some(vp), false);
    let prompt_idx = combined.find(prompt).unwrap();
    let fence_open = combined.find("<<<UNTRUSTED_VIEWPORT_CONTEXT").unwrap();
    let fence_close = combined
        .find("<<<END_UNTRUSTED_VIEWPORT_CONTEXT>>>")
        .unwrap();
    assert!(prompt_idx < fence_open);
    assert!(fence_open < fence_close);
    assert!(combined.ends_with("<<<END_UNTRUSTED_VIEWPORT_CONTEXT>>>"));

    let reset = compose_ask_instruction(prompt, Some(vp), true);
    assert!(reset.starts_with("[Session reset by caller:"));
    assert!(reset.ends_with("<<<END_UNTRUSTED_VIEWPORT_CONTEXT>>>"));
    assert_eq!(ask_partition("owner-1"), "tess:ask:owner-1");
}

#[test]
fn oversized_or_malformed_viewport_context_is_rejected_and_fence_markers_scrubbed() {
    let valid = TessViewportContext {
        route: Some("/tasks/T-A1".into()),
        route_label: Some("Task <<<breakout>>>".into()),
        entity_kind: Some(UiEntityKind::Task),
        entity_id: Some("T-A1".into()),
        entity_title: Some("Ship Tess".into()),
        decision_id: None,
        filters: BTreeMap::from([("status".into(), "blocked".into())]),
    };
    let rendered = validate_and_render_viewport(&valid).unwrap();
    assert!(rendered.len() <= MAX_VIEWPORT_CONTEXT_BYTES);
    assert!(!rendered.contains("<<<"));
    assert!(!rendered.contains(">>>"));

    // Oversized UTF-8 multibyte viewport context (> 800 B) fails cleanly with InvalidRequest.
    let big = TessViewportContext {
        route: Some("/tasks".into()),
        entity_title: Some("é".repeat(450)), // 900 UTF-8 bytes
        ..TessViewportContext::default()
    };
    let err = validate_and_render_viewport(&big).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);

    // Unsafe external or traversal route fails closed.
    for bad in [
        "https://evil.example/a",
        "//evil.example/a",
        "/tasks/../secret",
    ] {
        let vp = TessViewportContext {
            route: Some(bad.into()),
            ..TessViewportContext::default()
        };
        assert_eq!(
            validate_and_render_viewport(&vp).unwrap_err().code,
            ErrorCode::InvalidRequest,
            "route `{bad}` must be rejected"
        );
    }
}

#[test]
fn apply_revision_parsing_and_reconciliation_enforces_exact_match() {
    assert_eq!(
        crate::apply::parse_caller_revision(Some(3), None).unwrap(),
        Some(3)
    );
    assert_eq!(
        crate::apply::parse_caller_revision(None, Some("rev-3")).unwrap(),
        Some(3)
    );
    assert_eq!(
        crate::apply::parse_caller_revision(Some(3), Some("3")).unwrap(),
        Some(3)
    );
    assert_eq!(
        crate::apply::parse_caller_revision(Some(3), Some("4"))
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        crate::apply::parse_caller_revision(None, Some("not-a-rev"))
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        crate::apply::reconcile_revision(Some(2), Some(3))
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        crate::apply::reconcile_revision(Some(2), Some(2)).unwrap(),
        Some(2)
    );
}

#[test]
fn apply_transport_forbids_privileged_and_recursive_methods() {
    for forbidden in [
        "suggestion.apply",
        "suggestion.create",
        "tess.ask",
        "tess.turn_events",
        "auth.login_request",
        "provider.credential_status",
        "member.session_start",
        "member.external_approve",
        "git.push",
        "office.remove",
        "office.restart",
        "tasks.delete",
    ] {
        assert!(
            !crate::apply::is_permitted_apply_call(forbidden, "tasks.create", "tasks.create"),
            "`{forbidden}` must be blocked by ServerApplyTransport"
        );
    }
    assert!(crate::apply::is_permitted_apply_call(
        "tasks.create",
        "tasks.create",
        "tasks.create"
    ));
    assert!(crate::apply::is_permitted_apply_call(
        "org.snapshot",
        "tasks.create",
        "tasks.create"
    ));
}

#[test]
fn apply_translator_covers_every_eligible_confirm_required_command() {
    let table = org_command_catalogue::table();
    let mut covered = 0usize;
    for row in table.rows() {
        if table.mutation_policy(row) != org_command_catalogue::MutationPolicy::ConfirmRequired {
            continue;
        }
        if !crate::preflight::ALLOWED_SUGGESTION_CALLS.contains(&row.call) {
            continue;
        }
        let input: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(row.example.input).unwrap_or_else(|e| {
                panic!(
                    "example input for `{}` is not a JSON object: {e}",
                    row.name()
                )
            });
        let rev = (row.if_match() != org_command_catalogue::IfMatch::No).then_some(1);
        let mut seq = 0u64;
        let mut next_id = || {
            seq += 1;
            format!("0193a5c0-0000-7000-8000-{seq:012x}")
        };
        let prepared =
            crate::apply::prepare_apply_rpc(&mut next_id, row.resource, row.verb, &input, rev)
                .unwrap_or_else(|| {
                    panic!(
                        "eligible ConfirmRequired command `{}` (call `{}`) missing from org-tess apply translator",
                        row.name(),
                        row.call
                    )
                })
                .unwrap_or_else(|e| {
                    panic!(
                        "example input for `{}` failed apply translation: {e:?}",
                        row.name()
                    )
                });
        assert!(
            crate::apply::is_permitted_apply_call(prepared.method, row.call, prepared.method),
            "`{}` translated to forbidden RPC `{}`",
            row.name(),
            prepared.method
        );
        covered += 1;
    }
    assert!(
        covered >= 45,
        "expected >= 45 eligible ConfirmRequired commands covered, got {covered}"
    );
}
