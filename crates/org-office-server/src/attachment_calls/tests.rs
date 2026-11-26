//! The nine `attachment.*` RPCs end to end over a real office on both engines (`TASK-98.1`
//! AC #4): chunked uploads within the frame cap, binding, listing, inspecting, editing and
//! chunked reads, with the private space refused to every agent.

use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_office::{EngineChoice, TestOffice};
use org_proto::attachments::{
    AttachmentAttachParams, AttachmentInspectParams, AttachmentListParams,
    AttachmentReadChunkParams, AttachmentScope, AttachmentTargetKind, AttachmentUpdateMetaParams,
    AttachmentUploadBeginParams, AttachmentUploadChunkParams, AttachmentUploadRef,
    ATTACHMENT_CHUNK_MAX_BYTES,
};
use org_proto::hiring::{HireProposalWire, HrHire, HrHireParams};
use org_proto::{frame, Outcome, MAX_FRAME_BYTES};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

fn uuid() -> String {
    let n = NEXT.fetch_add(1, Ordering::Relaxed) + 4_000;
    let ms = org_office::organization_family::testkit::CREATION_AT.0 + n;
    let mut b = [0u8; 16];
    b[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    b[6] = 0x70 | ((n >> 8) as u8 & 0x0f);
    b[7] = (n & 0xff) as u8;
    b[8] = 0x80;
    b[15] = (n & 0xff) as u8;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Sends one typed call through the dispatcher, checking the reply fits a frame.
fn call<C: Call>(
    office: &Arc<Office>,
    member: &str,
    params: &C::Params,
) -> Result<C::Result, ApiError> {
    let request = Request::new::<C>(9, params).unwrap();
    let line = serde_json::to_vec(&request).unwrap();
    assert!(
        line.len() <= MAX_FRAME_BYTES,
        "{} request is {} bytes",
        C::METHOD,
        line.len()
    );
    let response =
        dispatch_attachment_rpc(office, Some(member), &request).expect("an attachment call");
    let mut wire = Vec::new();
    frame::write_message(&mut wire, &response).expect("every reply fits one frame");
    match response.outcome {
        Outcome::Result(value) => Ok(serde_json::from_value(value).unwrap()),
        Outcome::Error(error) => Err(error),
    }
}

fn hire_agent(office: &Arc<Office>, owner: &str) -> String {
    let root = office.org_snapshot().unwrap().teams[0].id.clone();
    let employee_id = uuid();
    let request = Request::new::<HrHire>(
        1,
        &HrHireParams {
            command_id: uuid(),
            request_id: uuid(),
            employee_id: employee_id.clone(),
            home_team_id: root,
            role_id: None,
            proposal: HireProposalWire {
                name: "Courier Bot".into(),
                handle: "courier-bot".into(),
                title_in_team: Some("Engineer".into()),
                ..Default::default()
            },
            reason: "attachment rpc tests".into(),
        },
    )
    .unwrap();
    let response = office.dispatch_hiring_rpc(Some(owner), &request).unwrap();
    assert!(
        matches!(response.outcome, Outcome::Result(_)),
        "{response:?}"
    );
    employee_id
}

/// 1.1 MiB of CSV: three chunks.
fn payload() -> Vec<u8> {
    let mut out = b"n,square\n".to_vec();
    let mut n = 0u64;
    while out.len() < 1_150_000 {
        out.extend(format!("{n},{}\n", n * n).into_bytes());
        n += 1;
    }
    out
}

fn upload(
    office: &Arc<Office>,
    member: &str,
    filename: &str,
    bytes: &[u8],
) -> Result<String, ApiError> {
    let begun = call::<AttachmentUploadBegin>(
        office,
        member,
        &AttachmentUploadBeginParams {
            filename: filename.into(),
            mime_type: "text/csv".into(),
            byte_count: bytes.len() as u64,
        },
    )?;
    assert_eq!(begun.chunk_size_bytes, ATTACHMENT_CHUNK_MAX_BYTES);
    let id = begun.upload.upload_id;
    let mut offset = 0;
    for chunk in bytes.chunks(begun.chunk_size_bytes as usize) {
        call::<AttachmentUploadChunk>(
            office,
            member,
            &AttachmentUploadChunkParams {
                upload_id: id.clone(),
                offset,
                data_base64: STANDARD.encode(chunk),
            },
        )?;
        offset += chunk.len() as u64;
    }
    let committed = call::<AttachmentUploadCommit>(
        office,
        member,
        &AttachmentUploadRef {
            upload_id: id.clone(),
        },
    )?;
    assert!(committed.upload.committed);
    Ok(id)
}

fn attach(
    office: &Arc<Office>,
    member: &str,
    upload_id: &str,
    kind: AttachmentTargetKind,
) -> Result<String, ApiError> {
    call::<AttachmentAttach>(
        office,
        member,
        &AttachmentAttachParams {
            command_id: uuid(),
            upload_id: Some(upload_id.into()),
            source_attachment_id: None,
            target_kind: kind,
            target_id: None,
            title: None,
            description: None,
            artifact_status: None,
        },
    )
    .map(|r| r.attachment.id)
}

fn read(office: &Arc<Office>, member: &str, id: &str) -> Result<Vec<u8>, ApiError> {
    let mut out = Vec::new();
    let mut chunks = 0;
    loop {
        let chunk = call::<AttachmentReadChunk>(
            office,
            member,
            &AttachmentReadChunkParams {
                attachment_id: id.into(),
                offset: out.len() as u64,
                max_bytes: None,
            },
        )?;
        chunks += 1;
        let bytes = STANDARD.decode(chunk.data_base64).unwrap();
        assert!(bytes.len() <= ATTACHMENT_CHUNK_MAX_BYTES as usize);
        out.extend(bytes);
        if chunk.eof {
            assert!(chunks >= 3, "a large file is read in several chunks");
            return Ok(out);
        }
    }
}

#[test]
fn all_nine_attachment_rpcs_work_end_to_end_on_both_engines() {
    for engine in [EngineChoice::Memory, EngineChoice::Sqlite] {
        let office = Arc::new(Office::for_test(TestOffice { engine }));
        let owner = office.owner_member_id().to_owned();
        let agent = hire_agent(&office, &owner);
        let bytes = payload();

        // 1-3, 5: upload in three chunks, commit, attach organization-wide.
        let upload_id = upload(&office, &owner, "squares.csv", &bytes).unwrap();
        let shared = attach(
            &office,
            &owner,
            &upload_id,
            AttachmentTargetKind::Organization,
        )
        .unwrap();
        // 9: read back by an agent in 512 KiB chunks.
        assert_eq!(read(&office, &agent, &shared).unwrap(), bytes);
        // 6: inspect.
        let inspected = call::<AttachmentInspect>(
            &office,
            &agent,
            &AttachmentInspectParams {
                attachment_id: shared.clone(),
            },
        )
        .unwrap();
        assert_eq!(inspected.attachment.byte_count, bytes.len() as u64);
        // 7: list.
        let listed = call::<AttachmentList>(
            &office,
            &agent,
            &AttachmentListParams {
                target_kind: Some(AttachmentTargetKind::Organization),
                target_id: None,
                scope: None,
                limit: Some(10),
                cursor: None,
            },
        )
        .unwrap();
        assert_eq!(listed.attachments.len(), 1);
        // 8: update the metadata.
        let renamed = call::<AttachmentUpdateMeta>(
            &office,
            &owner,
            &AttachmentUpdateMetaParams {
                command_id: uuid(),
                attachment_id: shared.clone(),
                title: Some("Squares".into()),
                description: Some("n and n squared".into()),
                artifact_status: None,
            },
        )
        .unwrap();
        assert_eq!(renamed.attachment.title, "Squares");
        // 4: abort a second upload.
        let begun = call::<AttachmentUploadBegin>(
            &office,
            &agent,
            &AttachmentUploadBeginParams {
                filename: "scratch.csv".into(),
                mime_type: "text/csv".into(),
                byte_count: 10,
            },
        )
        .unwrap();
        let aborted = call::<AttachmentUploadAbort>(
            &office,
            &agent,
            &AttachmentUploadRef {
                upload_id: begun.upload.upload_id.clone(),
            },
        )
        .unwrap();
        assert_eq!(aborted.upload_id, begun.upload.upload_id);
        let again = call::<AttachmentUploadAbort>(
            &office,
            &agent,
            &AttachmentUploadRef {
                upload_id: begun.upload.upload_id,
            },
        )
        .unwrap_err();
        assert_eq!(again.code, ErrorCode::NotFound);

        // The owner's private space: theirs to use, never an agent's.
        let small = upload(&office, &owner, "private.csv", b"a,b\n1,2\n").unwrap();
        let private = attach(&office, &owner, &small, AttachmentTargetKind::UserPrivate).unwrap();
        let list_private = |member: &str| {
            call::<AttachmentList>(
                &office,
                member,
                &AttachmentListParams {
                    target_kind: None,
                    target_id: None,
                    scope: Some(AttachmentScope::UserPrivate),
                    limit: None,
                    cursor: None,
                },
            )
        };
        assert_eq!(list_private(&owner).unwrap().attachments.len(), 1);
        // An agent has no private space: it is told so, and pointed at a 1:1 DM instead.
        let none = list_private(&agent).unwrap_err();
        assert_eq!(none.code, ErrorCode::NotAuthorized);
        assert!(none.message.contains("1:1 direct message"), "{none:?}");
        let peek = call::<AttachmentInspect>(
            &office,
            &agent,
            &AttachmentInspectParams {
                attachment_id: private.clone(),
            },
        )
        .unwrap_err();
        assert_eq!(peek.code, ErrorCode::NotFound);
        let staged = upload(&office, &agent, "mine.csv", b"x,y\n").unwrap();
        let refused =
            attach(&office, &agent, &staged, AttachmentTargetKind::UserPrivate).unwrap_err();
        assert_eq!(refused.code, ErrorCode::NotAuthorized);
        let refused = call::<AttachmentReadChunk>(
            &office,
            &agent,
            &AttachmentReadChunkParams {
                attachment_id: private,
                offset: 0,
                max_bytes: None,
            },
        )
        .unwrap_err();
        assert_eq!(refused.code, ErrorCode::NotFound);
    }
}

#[test]
fn unbound_bad_and_foreign_requests_are_refused_by_the_dispatcher() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let request = Request::new::<AttachmentInspect>(
        3,
        &AttachmentInspectParams {
            attachment_id: uuid(),
        },
    )
    .unwrap();
    let unbound = dispatch_attachment_rpc(&office, None, &request).unwrap();
    assert!(matches!(unbound.outcome, Outcome::Error(e) if e.code == ErrorCode::NotBound));
    let bad = Request {
        id: 4,
        method: AttachmentUploadChunk::METHOD.into(),
        principal: None,
        params: Value::String("not params".into()),
    };
    let owner = office.owner_member_id().to_owned();
    let answered = dispatch_attachment_rpc(&office, Some(&owner), &bad).unwrap();
    assert!(matches!(answered.outcome, Outcome::Error(e) if e.code == ErrorCode::InvalidRequest));
    let other = Request::new::<org_proto::directory::Who>(5, &Default::default()).unwrap();
    assert!(dispatch_attachment_rpc(&office, Some(&owner), &other).is_none());
    for method in org_proto::attachments::ATTACHMENT_METHODS {
        let request = Request {
            id: 6,
            method: method.into(),
            principal: None,
            params: serde_json::json!({}),
        };
        assert!(is_attachment_method(&request), "{method}");
    }
}
