use org_proto::chat::ConversationKind;
use org_proto::ErrorCode;

use super::*;
use crate::authz::PrincipalKind;
use crate::messaging::features::decode_refusal;
use crate::messaging::records::SCHEMA_VERSION;

const ORG: &str = "0190f000-0000-7000-8000-000000000001";
const DM: &str = "0190f000-0000-7000-8000-0000000000d1";
const AGENT: &str = "0190f000-0000-7000-8000-0000000000a1";
const PERSON: &str = "0190f000-0000-7000-8000-0000000000b1";
const STRANGER: &str = "0190f000-0000-7000-8000-0000000000c1";
const MESSAGE: &str = "0190f000-0000-7000-8000-0000000000e1";
const AT: &str = "2026-11-26T12:00:00.000Z";

fn id(text: &str) -> Id {
    Id::parse(text).unwrap()
}

fn caller(employee: &str) -> PrincipalContext {
    PrincipalContext {
        employee_id: employee.into(),
        kind: PrincipalKind::Agent,
        is_owner: false,
        team_ids: BTreeSet::new(),
        active: true,
    }
}

fn staged(upload: &str, by: &str, mime: &str) -> StagedFile {
    StagedFile {
        upload_id: upload.into(),
        uploaded_by: by.into(),
        content_sha256: "ab".repeat(32),
        media_type: mime.into(),
        byte_count: 42,
        filename: format!("{upload}.bin"),
        width: None,
        height: None,
        page_count: None,
    }
}

fn files(by: &str, uploads: &[(&str, &str)]) -> MessageFiles {
    MessageFiles {
        caller: Some(caller(by)),
        requested: uploads.iter().map(|(u, _)| (*u).to_owned()).collect(),
        staged: uploads
            .iter()
            .map(|(u, mime)| staged(u, by, mime))
            .collect(),
    }
}

fn conversation(kind: ConversationKind) -> ConversationRecord {
    ConversationRecord {
        schema_version: SCHEMA_VERSION,
        revision: 1,
        organization_id: id(ORG),
        id: id(DM),
        kind,
        name: None,
        auto_scope: None,
        team_id: None,
        project_id: None,
        archived: false,
        created_at: AT.into(),
        memberships: Vec::new(),
        pins: Vec::new(),
    }
}

fn bind(
    files: &MessageFiles,
    conv: &ConversationRecord,
    members: &[Id],
) -> Result<Vec<MessageAttachmentRecord>, ErrorCode> {
    files
        .bind(&Binding {
            organization_id: &id(ORG),
            conversation: conv,
            members,
            scope: MESSAGE_SCOPE,
            key: MESSAGE,
            created_at: AT,
        })
        .map_err(|refusal| decode_refusal(&refusal).code)
}

#[test]
fn bound_ids_are_uuidv7_in_the_order_named_and_stable_for_a_retry() {
    let named = files(
        AGENT,
        &[
            ("u-3", "image/png"),
            ("u-1", "application/pdf"),
            ("u-2", "text/csv"),
        ],
    );
    let ids = named.bound_ids(&id(ORG), MESSAGE_SCOPE, MESSAGE, AT);
    assert!(ids.iter().all(|i| org_proto::is_uuid_v7(i)), "{ids:?}");
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(sorted, ids, "IDs sort in the order the files were named");
    assert_eq!(named.bound_ids(&id(ORG), MESSAGE_SCOPE, MESSAGE, AT), ids);
    // Another record, scope or time binds the same uploads under other IDs.
    for other in [
        named.bound_ids(&id(ORG), MESSAGE_SCOPE, "another-message", AT),
        named.bound_ids(&id(ORG), ANSWER_SCOPE, MESSAGE, AT),
        named.bound_ids(&id(ORG), MESSAGE_SCOPE, MESSAGE, "2026-11-26T12:00:01.000Z"),
    ] {
        assert!(other.iter().all(|o| !ids.contains(o)));
    }
}

#[test]
fn binding_checks_the_uploader_and_the_conversation_through_the_policy() {
    let dm = conversation(ConversationKind::Dm);
    let members = [id(AGENT), id(PERSON)];
    let named = files(AGENT, &[("u-1", "image/png"), ("u-2", "application/pdf")]);
    let bound = bind(&named, &dm, &members).unwrap();
    assert_eq!(bound.len(), 2);
    assert_eq!(
        bound[0].content_ref,
        format!("blobs/sha256/ab/ab/{}", "ab".repeat(32))
    );
    assert_eq!(bound[0].title.as_deref(), Some("u-1.bin"));
    assert_eq!(bound[0].uploaded_by_employee_id, Some(id(AGENT)));
    assert_eq!(bound[0].created_at.as_deref(), Some(AT));
    assert!(named.matches(&bound, &id(ORG), MESSAGE_SCOPE, MESSAGE, AT));
    assert!(!named.matches(&bound[..1], &id(ORG), MESSAGE_SCOPE, MESSAGE, AT));
    assert!(!MessageFiles::none().matches(&bound, &id(ORG), MESSAGE_SCOPE, MESSAGE, AT));
    assert!(MessageFiles::none().matches(&[], &id(ORG), MESSAGE_SCOPE, MESSAGE, AT));

    // Someone else's upload, even when its record says otherwise, is never theirs to bind.
    let mut hijack = files(PERSON, &[("u-1", "image/png")]);
    hijack.staged[0].uploaded_by = AGENT.into();
    assert_eq!(bind(&hijack, &dm, &members), Err(ErrorCode::NotFound));
    // An upload that was not there for the caller leaves nothing to bind.
    let mut missing = files(AGENT, &[("u-1", "image/png"), ("u-2", "text/csv")]);
    missing.staged.pop();
    assert_eq!(bind(&missing, &dm, &members), Err(ErrorCode::NotFound));
    let mut anonymous = files(AGENT, &[("u-1", "image/png")]);
    anonymous.caller = None;
    assert_eq!(bind(&anonymous, &dm, &members), Err(ErrorCode::NotFound));
    // A stranger to a DM is not told it exists; a channel's non-member may not post files.
    assert_eq!(
        bind(&files(STRANGER, &[("u-9", "image/png")]), &dm, &members),
        Err(ErrorCode::NotFound)
    );
    let channel = conversation(ConversationKind::Channel);
    assert_eq!(
        bind(
            &files(STRANGER, &[("u-9", "image/png")]),
            &channel,
            &members
        ),
        Err(ErrorCode::NotAuthorized)
    );
    assert_eq!(bind(&MessageFiles::none(), &dm, &members), Ok(Vec::new()));
}

#[test]
fn metas_fall_back_to_the_record_and_filters_tell_images_from_documents() {
    let dm = conversation(ConversationKind::Dm);
    let bound = bind(
        &files(AGENT, &[("u-1", "image/png"), ("u-2", "application/pdf")]),
        &dm,
        &[id(AGENT)],
    )
    .unwrap();
    let legacy = MessageAttachmentRecord {
        id: "legacy".into(),
        media_type: "text/plain".into(),
        ..Default::default()
    };
    let shown = metas(std::slice::from_ref(&legacy), AT, Some(&id(PERSON)));
    assert_eq!(shown[0].filename, "legacy");
    assert_eq!(shown[0].title, "legacy");
    assert_eq!(shown[0].created_at, AT);
    assert_eq!(shown[0].uploaded_by_employee_id, PERSON);
    assert!(metas(&bound, "other", None)
        .iter()
        .all(|m| m.created_at == AT));

    let filter = |image, file| AttachmentFilter::new(image, file);
    assert!(admits(filter(None, None), &[]));
    assert!(admits(filter(Some(true), Some(true)), &bound));
    assert!(!admits(
        filter(Some(true), None),
        std::slice::from_ref(&legacy)
    ));
    assert!(admits(
        filter(Some(false), Some(true)),
        std::slice::from_ref(&legacy)
    ));
    assert!(!admits(filter(None, Some(true)), &bound[..1]));
    assert!(admits(filter(Some(false), Some(false)), &[]));
}

#[test]
fn a_message_needs_text_or_files_and_files_need_a_uuidv7_id() {
    let code = |id: &str, body: &str, files: &[&str]| {
        let files: Vec<String> = files.iter().map(|f| (*f).to_owned()).collect();
        check_message_files(id, body, &files).map_err(|r| decode_refusal(&r).code)
    };
    let upload = "0190f000-0000-7000-8000-0000000000f1";
    assert_eq!(code(MESSAGE, "hi", &[]), Ok(()));
    assert_eq!(code(MESSAGE, "", &[upload]), Ok(()));
    assert_eq!(code(MESSAGE, " \n", &[]), Err(ErrorCode::InvalidRequest));
    assert_eq!(code("01JDQXR7Y6H0Q7E5B8W4N2M3KA", "hi", &[]), Ok(()));
    assert_eq!(
        code("01JDQXR7Y6H0Q7E5B8W4N2M3KA", "hi", &[upload]),
        Err(ErrorCode::InvalidRequest)
    );
    assert_eq!(
        code(MESSAGE, "", &[upload, upload]),
        Err(ErrorCode::InvalidRequest)
    );
    let eleven = [upload; 11];
    assert_eq!(code(MESSAGE, "", &eleven), Err(ErrorCode::TooLarge));
}
