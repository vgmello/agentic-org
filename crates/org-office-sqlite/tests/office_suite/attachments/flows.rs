//! Uploads, bindings and reads end to end on both engines.

use org_proto::attachments::{
    ArtifactStatus, AttachmentInspectParams, AttachmentListParams, AttachmentReadChunkParams,
    AttachmentScope, AttachmentTargetKind, AttachmentUpdateMetaParams, AttachmentUploadBeginParams,
    AttachmentUploadChunkParams, AttachmentUploadRef,
};
use org_proto::ErrorCode;

use super::*;
use org_media::testkit as fixtures;
use org_office::attachments::blobs::ContentHash;
use org_office::attachments::staging::{MAX_STAGED_FILES, STAGED_TTL_MS};

#[test]
fn a_multi_chunk_upload_is_attached_listed_inspected_and_read_back_on_both_engines() {
    for engine in engines() {
        let fx = Fixture::new(engine.clone());
        let office = &fx.office;
        let csv = big_csv();
        let staged = upload(office, &fx.agent, "ledger.csv", "text/csv", &csv).unwrap();
        assert_eq!(staged.byte_count, csv.len() as u64);
        assert_eq!(
            staged.content_sha256.as_deref(),
            Some(ContentHash::of(&csv).as_str())
        );
        let file = attach_upload(
            office,
            &fx.agent,
            &staged.upload_id,
            AttachmentTargetKind::Project,
            Some(&fx.project),
        )
        .unwrap();
        assert!(org_proto::is_uuid_v7(&file.id));
        assert_ne!(file.id, staged.upload_id, "a binding gets its own id");
        assert_eq!(file.title, "ledger.csv");
        assert_eq!(file.artifact_status, ArtifactStatus::Final);
        assert_eq!(file.uploaded_by_employee_id, fx.agent);
        assert_eq!(file.scope, AttachmentScope::Shared);
        // The upload is consumed by the binding.
        let gone = office
            .attachment_upload_abort(
                &fx.agent,
                AttachmentUploadRef {
                    upload_id: staged.upload_id.clone(),
                },
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(gone.code, ErrorCode::NotFound);
        // Read back in odd-sized chunks and in maximal ones.
        assert_eq!(
            read_all(office, &fx.person, &file.id, 300_001).unwrap(),
            csv
        );
        assert_eq!(
            read_all(
                office,
                &fx.other_person,
                &file.id,
                ATTACHMENT_CHUNK_MAX_BYTES
            )
            .unwrap(),
            csv
        );
        let inspected = office
            .attachment_inspect(
                &fx.outsider_agent,
                AttachmentInspectParams {
                    attachment_id: file.id.clone(),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(inspected.attachment, file);
        let listed = office
            .attachment_list(
                &fx.person,
                AttachmentListParams {
                    target_kind: Some(AttachmentTargetKind::Project),
                    target_id: Some(fx.project.clone()),
                    scope: None,
                    limit: None,
                    cursor: None,
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(listed.attachments, vec![file.clone()]);
        assert_eq!(listed.next_cursor, None);
        // A read past the end, and a request for more than a chunk, are refused.
        let past = office
            .attachment_read_chunk(
                &fx.person,
                AttachmentReadChunkParams {
                    attachment_id: file.id.clone(),
                    offset: csv.len() as u64 + 1,
                    max_bytes: None,
                },
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(past.code, ErrorCode::InvalidRequest);
        let greedy = office
            .attachment_read_chunk(
                &fx.person,
                AttachmentReadChunkParams {
                    attachment_id: file.id.clone(),
                    offset: 0,
                    max_bytes: Some(ATTACHMENT_CHUNK_MAX_BYTES + 1),
                },
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(greedy.code, ErrorCode::InvalidRequest);
    }
}

#[test]
fn an_uploaded_photo_is_sanitized_before_hashing_and_has_a_model_derivative() {
    for engine in engines() {
        let fx = Fixture::new(engine.clone());
        let office = &fx.office;
        let raw = fixtures::jpeg_with_exif(64, 32, 6, fixtures::GPS_MARKER);
        let staged = upload(office, &fx.person, "IMG_0001.jpeg", "image/jpeg", &raw).unwrap();
        assert_eq!((staged.width, staged.height), (Some(32), Some(64)));
        let file = attach_upload(
            office,
            &fx.person,
            &staged.upload_id,
            AttachmentTargetKind::Organization,
            None,
        )
        .unwrap();
        let stored = read_all(office, &fx.agent, &file.id, ATTACHMENT_CHUNK_MAX_BYTES).unwrap();
        let marker = fixtures::GPS_MARKER.as_bytes();
        assert!(!stored.windows(marker.len()).any(|w| w == marker));
        assert!(!stored.windows(4).any(|w| w == b"Exif"));
        assert_eq!(file.content_sha256, ContentHash::of(&stored).to_string());
        assert_ne!(file.content_sha256, ContentHash::of(&raw).to_string());
        assert_eq!(file.byte_count, stored.len() as u64);
        let derivative = office
            .attachment_model_derivative(&fx.agent, &file.id)
            .unwrap()
            .unwrap();
        assert!(derivative.bytes.len() <= 1024 * 1024);
        assert_eq!((derivative.width, derivative.height), (32, 64));
        // Someone who may not read the file gets no derivative either.
        let csv = upload(office, &fx.person, "a.csv", "text/csv", b"a,b\n").unwrap();
        let private = attach_upload(
            office,
            &fx.person,
            &csv.upload_id,
            AttachmentTargetKind::UserPrivate,
            None,
        )
        .unwrap();
        let hidden = office
            .attachment_model_derivative(&fx.agent, &private.id)
            .unwrap()
            .unwrap_err();
        assert_eq!(hidden.code, ErrorCode::NotFound);
    }
}

#[test]
fn uploads_are_refused_when_blocked_mismatched_oversized_or_misnamed() {
    let fx = Fixture::new(EngineChoice::Memory);
    let office = &fx.office;
    let begin = |filename: &str, mime: &str, bytes: u64| {
        office
            .attachment_upload_begin(
                &fx.person,
                AttachmentUploadBeginParams {
                    filename: filename.into(),
                    mime_type: mime.into(),
                    byte_count: bytes,
                },
            )
            .unwrap()
            .map(|r| r.upload.upload_id)
    };
    for (name, mime) in [
        ("page.html", "text/html"),
        ("logo.svg", "image/svg+xml"),
        ("app.js", "application/javascript"),
        ("blob.bin", "application/octet-stream"),
        ("form.txt", "multipart/form-data"),
        ("run.exe", "text/plain"),
    ] {
        assert_eq!(
            begin(name, mime, 10).unwrap_err().code,
            ErrorCode::Unsupported,
            "{mime}"
        );
    }
    assert_eq!(
        begin("big.png", "image/png", 10 * 1024 * 1024 + 1)
            .unwrap_err()
            .code,
        ErrorCode::TooLarge
    );
    assert_eq!(
        begin("big.pdf", "application/pdf", 25 * 1024 * 1024 + 1)
            .unwrap_err()
            .code,
        ErrorCode::TooLarge
    );
    assert_eq!(
        begin("empty.txt", "text/plain", 0).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    // An executable declared as text uploads, then fails validation on commit.
    let elf = b"\x7fELF\x02\x01\x01\x00payload".to_vec();
    let refused = upload(office, &fx.person, "notes.txt", "text/plain", &elf).unwrap_err();
    assert_eq!(refused.code, ErrorCode::Unsupported);
    // A chunk larger than 512 KiB is refused before it is decoded or stored.
    let id = begin("big.csv", "text/csv", 600 * 1024).unwrap();
    let too_big = office
        .attachment_upload_chunk(
            &fx.person,
            AttachmentUploadChunkParams {
                upload_id: id.clone(),
                offset: 0,
                data_base64: STANDARD.encode(vec![b'a'; 512 * 1024 + 1]),
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(too_big.code, ErrorCode::TooLarge);
    let not_base64 = office
        .attachment_upload_chunk(
            &fx.person,
            AttachmentUploadChunkParams {
                upload_id: id,
                offset: 0,
                data_base64: "@@@".into(),
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(not_base64.code, ErrorCode::InvalidRequest);
}

#[test]
fn metadata_updates_are_journaled_and_bounded() {
    for engine in engines() {
        let fx = Fixture::new(engine.clone());
        let office = &fx.office;
        let staged = upload(
            office,
            &fx.agent,
            "spec.pdf",
            "application/pdf",
            &fixtures::pdf(4),
        )
        .unwrap();
        assert_eq!(staged.page_count, Some(4));
        let file = attach_upload(
            office,
            &fx.agent,
            &staged.upload_id,
            AttachmentTargetKind::Project,
            Some(&fx.project),
        )
        .unwrap();
        assert_eq!(file.page_count, Some(4));
        let update = |actor: &str, title: Option<&str>, description: Option<&str>| {
            office
                .attachment_update_meta(
                    actor,
                    AttachmentUpdateMetaParams {
                        command_id: next_uuid(),
                        attachment_id: file.id.clone(),
                        title: title.map(str::to_owned),
                        description: description.map(str::to_owned),
                        artifact_status: Some(ArtifactStatus::Review),
                    },
                )
                .unwrap()
        };
        let updated = update(&fx.agent, Some("Checkout spec"), Some("Draft for review")).unwrap();
        assert_eq!(updated.attachment.title, "Checkout spec");
        assert_eq!(
            updated.attachment.description.as_deref(),
            Some("Draft for review")
        );
        assert_eq!(updated.attachment.artifact_status, ArtifactStatus::Review);
        assert_eq!(updated.attachment.content_sha256, file.content_sha256);
        // The owner may edit any file; another uploader may not; bounds hold.
        update(&fx.owner, None, Some("")).unwrap();
        assert_eq!(
            update(&fx.person, Some("Mine now"), None).unwrap_err().code,
            ErrorCode::NotAuthorized
        );
        assert_eq!(
            update(&fx.agent, Some(&"t".repeat(161)), None)
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            update(&fx.agent, None, Some(&"d".repeat(281)))
                .unwrap_err()
                .code,
            ErrorCode::TooLarge
        );
        let now = office
            .attachment_inspect(
                &fx.person,
                AttachmentInspectParams {
                    attachment_id: file.id.clone(),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(now.attachment.description, None);
        assert_eq!(now.attachment.title, "Checkout spec");
    }
}

#[test]
fn lists_page_with_a_cursor_and_a_bounded_limit() {
    let fx = Fixture::new(EngineChoice::Sqlite(org_office_sqlite::sqlite_backend()));
    let office = &fx.office;
    let mut ids = Vec::new();
    for n in 0..5 {
        let staged = upload(
            office,
            &fx.owner,
            &format!("n{n}.txt"),
            "text/plain",
            format!("{n}").as_bytes(),
        )
        .unwrap();
        ids.push(
            attach_upload(
                office,
                &fx.owner,
                &staged.upload_id,
                AttachmentTargetKind::Organization,
                None,
            )
            .unwrap()
            .id,
        );
    }
    ids.sort();
    let page = |cursor: Option<String>, limit: Option<u32>| {
        office
            .attachment_list(
                &fx.agent,
                AttachmentListParams {
                    target_kind: Some(AttachmentTargetKind::Organization),
                    target_id: None,
                    scope: None,
                    limit,
                    cursor,
                },
            )
            .unwrap()
    };
    let first = page(None, Some(2)).unwrap();
    let second = page(first.next_cursor.clone(), Some(2)).unwrap();
    let third = page(second.next_cursor.clone(), Some(2)).unwrap();
    let seen: Vec<String> = [
        first.attachments,
        second.attachments,
        third.attachments.clone(),
    ]
    .concat()
    .into_iter()
    .map(|a| a.id)
    .collect();
    assert_eq!(seen, ids);
    assert_eq!(third.next_cursor, None);
    assert_eq!(
        page(None, Some(0)).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        page(None, Some(201)).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        page(Some("../x".into()), None).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert!(page(None, Some(200)).is_ok());
}

#[test]
fn staged_uploads_expire_after_a_day_and_the_quota_holds_through_the_office() {
    let fx = Fixture::new(EngineChoice::Memory);
    let office = &fx.office;
    let begin = |n: u64| {
        office
            .attachment_upload_begin(
                &fx.agent,
                AttachmentUploadBeginParams {
                    filename: format!("f{n}.txt"),
                    mime_type: "text/plain".into(),
                    byte_count: 1,
                },
            )
            .unwrap()
    };
    for n in 0..MAX_STAGED_FILES as u64 {
        begin(n).unwrap();
    }
    assert_eq!(begin(99).unwrap_err().code, ErrorCode::RateLimited);
    // Another employee is not affected.
    upload(office, &fx.person, "x.txt", "text/plain", b"x").unwrap();
    let root = office.attachment_staging_root();
    assert_eq!(
        std::fs::read_dir(&root).unwrap().count(),
        MAX_STAGED_FILES + 1
    );
    assert_eq!(
        office.attachment_gc_staged().unwrap(),
        0,
        "nothing has expired yet"
    );
    let swept = office
        .attachment_gc_staged_at(CREATION_AT.0 + STAGED_TTL_MS)
        .unwrap();
    assert_eq!(swept, MAX_STAGED_FILES + 1);
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
}
