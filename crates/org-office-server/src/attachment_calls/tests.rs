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

#[test]
fn only_an_attach_that_bound_a_file_wakes_the_outbox_shipper() {
    let office = Arc::new(Office::for_test(TestOffice {
        engine: EngineChoice::Memory,
    }));
    let owner = office.owner_member_id().to_owned();
    let upload_id = upload(&office, &owner, "wake.csv", b"a,b\n1,2\n").unwrap();
    let attach_request = |upload_id: &str| {
        Request::new::<AttachmentAttach>(
            11,
            &AttachmentAttachParams {
                command_id: uuid(),
                upload_id: Some(upload_id.into()),
                source_attachment_id: None,
                target_kind: AttachmentTargetKind::Organization,
                target_id: None,
                title: None,
                description: None,
                artifact_status: None,
            },
        )
        .unwrap()
    };
    let bound = attach_request(&upload_id);
    let response = dispatch_attachment_rpc(&office, Some(&owner), &bound).unwrap();
    assert!(bound_a_file(&bound, &response), "{response:?}");
    // A refused attach (that upload is bound and gone now) and any other call wake nothing.
    let refused = attach_request(&upload_id);
    let response = dispatch_attachment_rpc(&office, Some(&owner), &refused).unwrap();
    assert!(matches!(response.outcome, Outcome::Error(_)));
    assert!(!bound_a_file(&refused, &response));
    let list = Request::new::<org_proto::attachments::AttachmentList>(
        12,
        &AttachmentListParams {
            target_kind: Some(AttachmentTargetKind::Organization),
            target_id: None,
            scope: Some(AttachmentScope::Shared),
            limit: None,
            cursor: None,
        },
    )
    .unwrap();
    let response = dispatch_attachment_rpc(&office, Some(&owner), &list).unwrap();
    assert!(matches!(response.outcome, Outcome::Result(_)));
    assert!(!bound_a_file(&list, &response));
}

fn tiny_png_bytes() -> Vec<u8> {
    // 1x1 valid PNG (opaque blue pixel)
    const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGNgYPgPAAEDAQAIicLsAAAAAElFTkSuQmCC";
    STANDARD.decode(PNG_B64).unwrap()
}

fn create_task_for_test(office: &Arc<Office>, owner: &str) -> (String, String) {
    let team_id = office.org_snapshot().unwrap().teams[0].id.clone();
    let created = office
        .task_create(
            owner,
            org_proto::tasks::TasksCreateParams {
                command_id: uuid(),
                task_id: uuid(),
                team_id: Some(team_id),
                project_id: None,
                assigned_team_id: None,
                assignee_id: None,
                task_type: None,
                work_type: None,
                title: "PDF delivery task".into(),
                body_markdown: String::new(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                details: None,
                dod: None,
                attachment_ids: vec![],
            },
        )
        .unwrap()
        .unwrap();
    (created.task.id, created.task.key)
}

#[test]
fn attachment_render_hosts_markdown_and_html_to_task_and_org_and_returns_valid_pdf() {
    use org_proto::attachments::{
        ArtifactStatus, AttachmentRender, AttachmentRenderParams, RenderFormat, RenderSourceKind,
    };

    for engine in [EngineChoice::Memory, EngineChoice::Sqlite] {
        let office = Arc::new(Office::for_test(TestOffice { engine }));
        let owner = office.owner_member_id().to_owned();
        let agent = hire_agent(&office, &owner);
        let (task_id, task_key) = create_task_for_test(&office, &owner);

        // 1. Agent renders Markdown -> attaches to task via task_ref, then inspects & fetches it.
        let md_params = AttachmentRenderParams {
            command_id: Some(uuid()),
            format: RenderFormat::Pdf,
            source_kind: RenderSourceKind::Markdown,
            source: "# Delivery Summary\n\n| Item | Status |\n| --- | --- |\n| Render | OK |\n"
                .into(),
            filename: Some("summary.md".into()),
            target_kind: Some(AttachmentTargetKind::Task),
            task_ref: Some(task_key.clone()),
            title: Some("Delivery Summary".into()),
            description: Some("Rendered in office".into()),
            artifact_status: Some(ArtifactStatus::Review),
            ..Default::default()
        };
        let render_req = Request::new::<AttachmentRender>(20, &md_params).unwrap();
        let render_resp = dispatch_attachment_rpc(&office, Some(&agent), &render_req).unwrap();
        assert!(
            bound_a_file(&render_req, &render_resp),
            "hosted render must wake outbox shipper"
        );
        let rendered: org_proto::attachments::AttachmentRenderResult = match render_resp.outcome {
            Outcome::Result(v) => serde_json::from_value(v).unwrap(),
            Outcome::Error(e) => panic!("unexpected error: {e:?}"),
        };
        let att_id = rendered.attachment_id.clone().expect("attachment_id");
        assert!(rendered
            .canonical_url
            .as_deref()
            .unwrap()
            .ends_with(&format!("/attachments/{att_id}")));
        assert!(rendered.page_count >= 1);
        assert!(rendered.byte_count > 100);

        // Inspect & fetch rendered PDF bytes via existing attachment RPCs!
        let inspected = call::<AttachmentInspect>(
            &office,
            &owner,
            &AttachmentInspectParams {
                attachment_id: att_id.clone(),
            },
        )
        .unwrap();
        assert_eq!(inspected.attachment.mime_type, "application/pdf");
        assert_eq!(inspected.attachment.filename, "summary.pdf");
        assert_eq!(inspected.attachment.target_kind, AttachmentTargetKind::Task);
        assert_eq!(
            inspected.attachment.target_id.as_deref(),
            Some(task_id.as_str())
        );
        assert_eq!(inspected.attachment.page_count, Some(rendered.page_count));

        let chunk = call::<AttachmentReadChunk>(
            &office,
            &owner,
            &AttachmentReadChunkParams {
                attachment_id: att_id,
                offset: 0,
                max_bytes: None,
            },
        )
        .unwrap();
        let pdf_bytes = STANDARD.decode(chunk.data_base64).unwrap();
        assert!(pdf_bytes.starts_with(b"%PDF-"));

        // 2. HTML + CSS multi-page table with @page header/footer in local return_bytes mode.
        let mut rows = String::new();
        for i in 1..=70 {
            rows.push_str(&format!(
                "<tr><td>R{i}</td><td>Multi-page row {i}</td></tr>"
            ));
        }
        let html_params = AttachmentRenderParams {
            format: RenderFormat::Pdf,
            source_kind: RenderSourceKind::Html,
            source: format!(
                "<h1>Matrix</h1><table><thead><tr><th>ID</th><th>Label</th></tr></thead><tbody>{rows}</tbody></table>"
            ),
            css: Some(
                "@page { margin: 54pt; @top-left { content: \"Report\"; } @bottom-right { content: \"Page \" counter(page) \" of \" counter(pages); } }"
                    .into(),
            ),
            return_bytes: true,
            ..Default::default()
        };
        let local_req = Request::new::<AttachmentRender>(21, &html_params).unwrap();
        let local_resp = dispatch_attachment_rpc(&office, Some(&agent), &local_req).unwrap();
        assert!(!bound_a_file(&local_req, &local_resp));
        let local_res: org_proto::attachments::AttachmentRenderResult = match local_resp.outcome {
            Outcome::Result(v) => serde_json::from_value(v).unwrap(),
            Outcome::Error(e) => panic!("unexpected html error: {e:?}"),
        };
        assert!(local_res.attachment_id.is_none());
        assert!(local_res.page_count >= 2, "expected multi-page PDF");
        let local_bytes = STANDARD.decode(local_res.pdf_base64.unwrap()).unwrap();
        assert!(local_bytes.starts_with(b"%PDF-"));
        assert_eq!(local_bytes.len() as u64, local_res.byte_count);
    }
}

#[test]
fn attachment_render_enforces_private_space_ssrf_and_attachment_image_authz() {
    use org_proto::attachments::{
        AttachmentRender, AttachmentRenderParams, RenderFormat, RenderSourceKind,
    };

    let office = Arc::new(Office::for_test(TestOffice {
        engine: EngineChoice::Memory,
    }));
    let owner = office.owner_member_id().to_owned();
    let agent = hire_agent(&office, &owner);

    // 1. Agent requesting UserPrivate fails closed with 1:1 DM hint.
    let priv_err = call::<AttachmentRender>(
        &office,
        &agent,
        &AttachmentRenderParams {
            format: RenderFormat::Pdf,
            source_kind: RenderSourceKind::Markdown,
            source: "# Secret".into(),
            target_kind: Some(AttachmentTargetKind::UserPrivate),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(priv_err.code, ErrorCode::NotAuthorized);
    assert!(
        priv_err.message.contains("1:1 direct message"),
        "{priv_err:?}"
    );

    // 2. Owner CAN render to UserPrivate.
    let owner_priv = call::<AttachmentRender>(
        &office,
        &owner,
        &AttachmentRenderParams {
            format: RenderFormat::Pdf,
            source_kind: RenderSourceKind::Markdown,
            source: "# Owner Private Memo".into(),
            target_kind: Some(AttachmentTargetKind::UserPrivate),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(owner_priv.attachment_id.is_some());

    // 3. SSRF / local exfil / active script / svg data URI all fail closed via RPC.
    for (src, css) in [
        (r#"<img src="file:///etc/passwd">"#, None),
        (r#"<img src="https://evil.example/a.png">"#, None),
        (r#"<script>alert(1)</script><p>x</p>"#, None),
        (r#"<img src="data:image/svg+xml;base64,PHN2Zy8+">"#, None),
        (
            "<p>styled</p>",
            Some("body { background: url(file:///etc/shadow); }"),
        ),
    ] {
        let err = call::<AttachmentRender>(
            &office,
            &agent,
            &AttachmentRenderParams {
                format: RenderFormat::Pdf,
                source_kind: RenderSourceKind::Html,
                source: src.into(),
                css: css.map(str::to_owned),
                return_bytes: true,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotAuthorized, "{src}: {err:?}");
    }

    // 4. Pre-authorized `attachment:<id>` images obey AuthorizationPolicy::can_read:
    //    Owner uploads one shared PNG and one UserPrivate PNG.
    let png = tiny_png_bytes();
    let upload_png = |member: &str, name: &str| {
        let begun = call::<AttachmentUploadBegin>(
            &office,
            member,
            &AttachmentUploadBeginParams {
                filename: name.into(),
                mime_type: "image/png".into(),
                byte_count: png.len() as u64,
            },
        )
        .unwrap();
        let uid = begun.upload.upload_id;
        call::<AttachmentUploadChunk>(
            &office,
            member,
            &AttachmentUploadChunkParams {
                upload_id: uid.clone(),
                offset: 0,
                data_base64: STANDARD.encode(&png),
            },
        )
        .unwrap();
        call::<AttachmentUploadCommit>(
            &office,
            member,
            &AttachmentUploadRef {
                upload_id: uid.clone(),
            },
        )
        .unwrap();
        uid
    };
    let shared_uid = upload_png(&owner, "logo.png");
    let shared_img = attach(
        &office,
        &owner,
        &shared_uid,
        AttachmentTargetKind::Organization,
    )
    .unwrap();
    let priv_uid = upload_png(&owner, "secret.png");
    let priv_img = attach(
        &office,
        &owner,
        &priv_uid,
        AttachmentTargetKind::UserPrivate,
    )
    .unwrap();

    // Agent CAN embed shared org image.
    let ok = call::<AttachmentRender>(
        &office,
        &agent,
        &AttachmentRenderParams {
            format: RenderFormat::Pdf,
            source_kind: RenderSourceKind::Html,
            source: format!(r#"<h1>Doc</h1><img src="attachment:{shared_img}">"#),
            return_bytes: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(ok.page_count >= 1);

    // Agent CANNOT embed owner's UserPrivate image (fails closed!).
    let denied = call::<AttachmentRender>(
        &office,
        &agent,
        &AttachmentRenderParams {
            format: RenderFormat::Pdf,
            source_kind: RenderSourceKind::Html,
            source: format!(r#"<h1>Leak</h1><img src="attachment:{priv_img}">"#),
            return_bytes: true,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(denied.code, ErrorCode::NotAuthorized);
}

#[test]
fn attachment_render_supports_chunked_large_input_and_chunked_large_pdf_output() {
    use org_doc_render::{DocumentRenderer, RenderError, RenderRequest, RenderedPdf};
    use org_proto::attachments::{
        AttachmentRender, AttachmentRenderParams, RenderFormat, RenderSourceKind,
    };

    // Mock renderer returning a valid > 512 KiB PDF so multi-chunk output streaming is exercised.
    struct BigPdfRenderer;
    impl DocumentRenderer for BigPdfRenderer {
        fn render_markdown_to_pdf(&self, _: &RenderRequest) -> Result<RenderedPdf, RenderError> {
            let mut bytes = b"%PDF-1.7\n1 0 obj<< /Type /Catalog /Pages 2 0 R >>endobj\n2 0 obj<< /Type /Pages /Kids [3 0 R] /Count 1 >>endobj\n3 0 obj<< /Type /Page /Parent 2 0 R >>endobj\n%".to_vec();
            bytes.resize(700_000, b'A');
            bytes.extend_from_slice(
                b"\nxref\n0 1\n0000000000 65535 f \ntrailer<<>>\nstartxref\n10\n%%EOF\n",
            );
            Ok(RenderedPdf {
                byte_count: bytes.len() as u64,
                page_count: 1,
                bytes,
            })
        }
        fn render_html_to_pdf(&self, req: &RenderRequest) -> Result<RenderedPdf, RenderError> {
            self.render_markdown_to_pdf(req)
        }
    }

    let office = Arc::new(Office::for_test(TestOffice {
        engine: EngineChoice::Memory,
    }));
    let owner = office.owner_member_id().to_owned();
    let agent = hire_agent(&office, &owner);
    let runtime = RenderRuntime::with_renderer(Arc::new(BigPdfRenderer));

    let call_rt = |member: &str, params: &AttachmentRenderParams| {
        let req = Request::new::<AttachmentRender>(30, params).unwrap();
        let resp = dispatch_attachment_rpc_with(&office, &runtime, Some(member), &req).unwrap();
        let mut wire = Vec::new();
        frame::write_message(&mut wire, &resp).expect("every chunk reply fits MAX_FRAME_BYTES");
        match resp.outcome {
            Outcome::Result(v) => Ok(serde_json::from_value::<
                org_proto::attachments::AttachmentRenderResult,
            >(v)
            .unwrap()),
            Outcome::Error(e) => Err(e),
        }
    };

    // 1. Stream input across 2 chunks (input_eof=false -> input_eof=true).
    let step1 = call_rt(
        &agent,
        &AttachmentRenderParams {
            format: RenderFormat::Pdf,
            source_kind: RenderSourceKind::Markdown,
            source_chunk_base64: Some(STANDARD.encode(b"# Part 1\n")),
            input_eof: Some(false),
            return_bytes: true,
            ..Default::default()
        },
    )
    .unwrap();
    let stage_id = step1.render_id.expect("staged input id");

    let first_out = call_rt(
        &agent,
        &AttachmentRenderParams {
            format: RenderFormat::Pdf,
            source_kind: RenderSourceKind::Markdown,
            render_id: Some(stage_id),
            source_chunk_base64: Some(STANDARD.encode(b"Part 2\n")),
            input_eof: Some(true),
            return_bytes: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(first_out.eof, Some(false));
    let out_id = first_out.render_id.clone().expect("chunked output handle");
    let mut assembled = STANDARD.decode(first_out.pdf_base64.unwrap()).unwrap();
    assert_eq!(assembled.len(), ATTACHMENT_CHUNK_MAX_BYTES as usize);

    // 2. Another caller MUST NOT be able to read agent's buffered PDF!
    let stolen = call_rt(
        &owner,
        &AttachmentRenderParams {
            return_bytes: true,
            render_id: Some(out_id.clone()),
            chunk_offset: Some(assembled.len() as u64),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(stolen.code, ErrorCode::NotFound);

    // 3. Original caller finishes reading and buffer evicts cleanly on EOF.
    let second_out = call_rt(
        &agent,
        &AttachmentRenderParams {
            return_bytes: true,
            render_id: Some(out_id.clone()),
            chunk_offset: Some(assembled.len() as u64),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(second_out.eof, Some(true));
    assert!(second_out.render_id.is_none());
    assembled.extend(STANDARD.decode(second_out.pdf_base64.unwrap()).unwrap());
    assert_eq!(assembled.len() as u64, first_out.byte_count);
    assert!(assembled.starts_with(b"%PDF-"));

    // Evicted after EOF.
    let evicted = call_rt(
        &agent,
        &AttachmentRenderParams {
            return_bytes: true,
            render_id: Some(out_id),
            chunk_offset: Some(0),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(evicted.code, ErrorCode::NotFound);
}
