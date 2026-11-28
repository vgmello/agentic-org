//! Unit tests of the documentation routes' query parsing, path checks and refusal mapping.

use super::*;
use org_proto::{ApiError, ErrorCode};

/// A canonical lowercase UUIDv7 `command_id`.
const COMMAND_ID: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607109";

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
fn list_query_values_stay_within_their_fields_bounds() {
    for limit in ["1", "200"] {
        let query = format!("limit={limit}");
        assert!(
            parse_list_query("p".into(), Some(&query)).is_ok(),
            "{limit}"
        );
    }
    for limit in ["0", "201", "-1", "4294967296", "1.5", ""] {
        let query = format!("limit={limit}");
        assert!(
            parse_list_query("p".into(), Some(&query)).is_err(),
            "{limit}"
        );
    }
    for (key, max) in [
        ("section", DOC_SECTION_MAX_BYTES),
        ("kind", DOC_KIND_MAX_BYTES),
        ("cursor", DOC_PATH_MAX_BYTES),
    ] {
        let at = format!("{key}={}", "a".repeat(max));
        assert!(
            parse_list_query("p".into(), Some(&at)).is_ok(),
            "{key} at {max}"
        );
        let over = format!("{key}={}", "a".repeat(max + 1));
        assert!(
            parse_list_query("p".into(), Some(&over)).is_err(),
            "{key} over {max}"
        );
    }
}

#[test]
fn write_bodies_need_a_canonical_command_id_and_no_unknown_field() {
    let pair = CallerAuth::LocalAuthless;
    let create = |body: &str| {
        to_call(
            DocRoute::Create(pair, "org".into(), "checkout".into()),
            body.as_bytes(),
        )
    };
    let update = |body: &str| {
        to_call(
            DocRoute::Update(pair, "org".into(), "checkout".into(), "doc".into()),
            body.as_bytes(),
        )
    };
    let moved = |body: &str| {
        to_call(
            DocRoute::Move(pair, "org".into(), "checkout".into(), "doc".into()),
            body.as_bytes(),
        )
    };
    assert!(create(&format!(
        r#"{{"command_id":"{COMMAND_ID}","kind":"guide","title":"T"}}"#
    ))
    .is_ok());
    assert!(update(&format!(
        r#"{{"command_id":"{COMMAND_ID}","expected_revision":1}}"#
    ))
    .is_ok());
    assert!(moved(&format!(
        r#"{{"command_id":"{COMMAND_ID}","expected_revision":1,"new_path":"a.md"}}"#
    ))
    .is_ok());
    for bad in [
        "c",
        "",
        "0190A5D2-7C3E-7A01-8B2C-3D4E5F607109",
        "0190a5d2-7c3e-4a01-8b2c-3d4e5f607109",
        "0190a5d27c3e7a018b2c3d4e5f607109",
    ] {
        assert!(
            create(&format!(
                r#"{{"command_id":"{bad}","kind":"guide","title":"T"}}"#
            ))
            .is_err(),
            "create {bad:?}"
        );
        assert!(
            update(&format!(
                r#"{{"command_id":"{bad}","expected_revision":1}}"#
            ))
            .is_err(),
            "update {bad:?}"
        );
        assert!(
            moved(&format!(
                r#"{{"command_id":"{bad}","expected_revision":1,"new_path":"a.md"}}"#
            ))
            .is_err(),
            "move {bad:?}"
        );
    }
    assert!(create(&format!(
        r#"{{"command_id":"{COMMAND_ID}","kind":"guide","title":"T","owner":"me"}}"#
    ))
    .is_err());
    assert!(update(&format!(
        r#"{{"command_id":"{COMMAND_ID}","expected_revision":1,"archived":true}}"#
    ))
    .is_err());
    assert!(moved(&format!(
        r#"{{"command_id":"{COMMAND_ID}","expected_revision":1,"new_path":"a.md","force":true}}"#
    ))
    .is_err());
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
    let body = format!(
        r#"{{"command_id":"{COMMAND_ID}","project":"elsewhere","document_id":"other","expected_revision":3,"new_section":"archive"}}"#
    );
    let moved = to_call(
        DocRoute::Move(pair, "org".into(), "checkout".into(), "doc".into()),
        body.as_bytes(),
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
        format!(
            r#"{{"command_id":"{COMMAND_ID}","project":"elsewhere","kind":"guide","title":"T"}}"#
        )
        .as_bytes(),
    )
    .ok()
    .unwrap();
    let DocCall::Create(_, _, params) = create else {
        panic!("a create");
    };
    assert_eq!(params.project, "checkout");
    let deleted = to_call(
        DocRoute::Delete(pair, "org".into(), "checkout".into(), "doc".into()),
        format!(
            r#"{{"command_id":"{COMMAND_ID}","project":"elsewhere","document_id":"other","expected_revision":4}}"#
        )
        .as_bytes(),
    )
    .ok()
    .unwrap();
    let DocCall::Delete(_, _, params) = deleted else {
        panic!("a delete");
    };
    assert_eq!(
        (
            params.project.as_str(),
            params.document_id.as_str(),
            params.expected_revision
        ),
        ("checkout", "doc", 4)
    );
    let media = to_call(
        DocRoute::PutMedia(pair, "org".into(), "checkout".into()),
        format!(
            r#"{{"command_id":"{COMMAND_ID}","project":"elsewhere","file_name":"a.png","media_type":"image/png","type":"image","title":"A","content_base64":"AA=="}}"#
        )
        .as_bytes(),
    )
    .ok()
    .unwrap();
    let DocCall::PutMedia(_, _, params) = media else {
        panic!("a put_media");
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
