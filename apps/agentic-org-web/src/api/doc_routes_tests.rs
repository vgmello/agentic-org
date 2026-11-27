//! Unit tests of the documentation routes' query parsing, path checks and refusal mapping.

use super::*;
use org_proto::{ApiError, ErrorCode};

#[test]
fn list_query_takes_known_filters_once_and_decodes_them() {
    let params = parse_list_query(
        "checkout".into(),
        Some("section=guides%2Fops&kind=runbook&status=current&limit=5&cursor=guides%2Fa.md"),
    )
    .ok()
    .unwrap();
    assert_eq!(params.project, "checkout");
    assert_eq!(params.section.as_deref(), Some("guides/ops"));
    assert_eq!(params.kind.as_deref(), Some("runbook"));
    assert_eq!(params.status, Some(DocStatusWire::Current));
    assert_eq!(params.limit, Some(5));
    assert_eq!(params.cursor.as_deref(), Some("guides/a.md"));
    assert!(parse_list_query("p".into(), None)
        .ok()
        .unwrap()
        .section
        .is_none());
    for bad in [
        "section=a&section=b",
        "unknown=1",
        "status=archived",
        "limit=many",
        "noequals",
        "section=%zz",
        "section=%e9",
        "section=%2",
    ] {
        assert!(parse_list_query("p".into(), Some(bad)).is_err(), "{bad}");
    }
    let long = format!("cursor={}", "a".repeat(DOC_REF_MAX_BYTES + 1));
    assert!(parse_list_query("p".into(), Some(&long)).is_err());
}

#[test]
fn path_segments_are_checked_before_any_office_call() {
    for ok in ["checkout", "P-12", "0190a5d2-7c3e-7a01-8b2c-3d4e5f607101"] {
        assert!(valid_project(ok).is_ok(), "{ok}");
    }
    for bad in ["", "a b", "a/b", "..", "é", &"x".repeat(129)] {
        assert!(valid_project(bad).is_err(), "{bad:?}");
    }
    assert!(valid_doc_id("0190a5d2-7c3e-7a01-8b2c-3d4e5f607101").is_ok());
    for bad in [
        "guides/setup.md",
        "index",
        "0190A5D2-7C3E-7A01-8B2C-3D4E5F607101",
    ] {
        assert!(valid_doc_id(bad).is_err(), "{bad}");
    }
}

#[test]
fn office_refusals_keep_their_class_and_unknown_codes_are_unavailable() {
    for (code, status) in [
        (ErrorCode::InvalidRequest, StatusCode::BAD_REQUEST),
        (ErrorCode::NotAuthorized, StatusCode::FORBIDDEN),
        (ErrorCode::NotFound, StatusCode::NOT_FOUND),
        (ErrorCode::TooLarge, StatusCode::PAYLOAD_TOO_LARGE),
        (ErrorCode::Conflict, StatusCode::CONFLICT),
        (ErrorCode::AlreadyExists, StatusCode::CONFLICT),
        (ErrorCode::OperationReused, StatusCode::CONFLICT),
        (ErrorCode::StorageError, StatusCode::SERVICE_UNAVAILABLE),
    ] {
        let reply = map_office_failure(OfficeFailure::Refused(ApiError::new(
            code,
            "project not found",
        )));
        assert_eq!(reply.status(), status, "{code:?}");
    }
    let not_found = map_office_failure(OfficeFailure::Refused(ApiError::new(
        ErrorCode::NotFound,
        "project not found",
    )));
    let body: Value = serde_json::from_slice(not_found.body_bytes()).unwrap();
    assert_eq!(body["error"]["code"], "not_found");
    assert_eq!(
        map_office_failure(OfficeFailure::Transport).status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[test]
fn bodies_take_the_project_and_document_from_the_path() {
    let pair = CallerAuth::LocalAuthless;
    let body = br#"{"command_id":"c","project":"elsewhere","document_id":"other","expected_revision":3,"new_section":"archive"}"#;
    let moved = to_call(
        DocRoute::Move(pair, "org".into(), "checkout".into(), "doc".into()),
        body,
    )
    .ok()
    .unwrap();
    let DocCall::Move(_, org, params) = moved else {
        panic!("a move");
    };
    assert_eq!(
        (
            org.as_str(),
            params.project.as_str(),
            params.document_id.as_str()
        ),
        ("org", "checkout", "doc")
    );
    assert_eq!(params.new_section.as_deref(), Some("archive"));
    let create = to_call(
        DocRoute::Create(pair, "org".into(), "checkout".into()),
        br#"{"command_id":"c","project":"elsewhere","kind":"guide","title":"T"}"#,
    )
    .ok()
    .unwrap();
    let DocCall::Create(_, _, params) = create else {
        panic!("a create");
    };
    assert_eq!(params.project, "checkout");
    assert!(to_call(
        DocRoute::Create(pair, "org".into(), "p".into()),
        b"not json"
    )
    .is_err());
    assert!(to_call(DocRoute::Index(pair, "org".into(), "p".into()), b"{}").is_err());
    assert!(prepare(DocRoute::Index(pair, "org".into(), "p".into()), b"").is_ok());
}
