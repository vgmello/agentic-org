//! What HQ checks before it keeps a satellite's file for every office (`TASK-98.6`): the name
//! fits the type, an image carries pixels alone, the record's type, dimensions and page count
//! are the bytes' own, a new attachment always sends its bytes (a hash alone binds nothing),
//! the same bytes under two attachments are kept once, and the uploader is decided again when
//! the bytes arrive.

use org_proto::attachments::{AttachmentTargetKind, AttachmentView};
use org_proto::ErrorCode;
use org_store::Id;

use super::custody::{check, shipped, SATELLITE};
use super::update_meta::ArchiveProject;
use super::{attach_upload, engines, read_all, upload, Fixture};
use org_media::testkit::{gif, jpeg_with_exif, png_with_text, webp, GPS_MARKER};
use org_office::attachments::media::{self, MediaType};
use org_office::attachments::records::AttachmentRecord;
use org_office::attachments::testkit::{sample_pdf, sample_png};
use org_office::testkit_support::store_open::EngineChoice;

/// Checks `record` from the satellite and commits `bytes` for it: the refusal of either.
fn ship(fx: &Fixture, record: &AttachmentRecord, bytes: &[u8]) -> Result<(), org_proto::ApiError> {
    let checked = check(fx, SATELLITE, record)?;
    assert!(!checked.present, "a new attachment always sends its bytes");
    fx.office
        .attachment_ingest_commit(&checked, Some(bytes.to_vec()))
        .unwrap()
}

fn bound(fx: &Fixture, record: &AttachmentRecord) -> bool {
    read_all(&fx.office, &fx.owner, record.id.as_str(), 4096).is_ok()
}

fn org_file(fx: &Fixture, mime: &str, bytes: &[u8]) -> AttachmentRecord {
    shipped(
        &fx.person,
        AttachmentTargetKind::Organization,
        None,
        mime,
        bytes,
    )
}

#[test]
fn hq_never_keeps_an_image_that_still_carries_metadata_or_bytes_after_its_end() {
    let fx = Fixture::new(EngineChoice::Memory);
    let before = fx.office.attachment_blob_counts().unwrap();
    let clean = sample_png(12, 8);
    let polyglot = [clean.as_slice(), b"PK\x05\x06 a zip after IEND"].concat();
    let animated_clean = media::validate(MediaType::Gif, gif(6, 6, 2)).unwrap().bytes;
    let gif_trailing = [animated_clean.as_slice(), b"<script>"].concat();
    for (mime, raw) in [
        // Straight from an uploader's camera: EXIF with GPS and an orientation.
        ("image/jpeg", jpeg_with_exif(16, 8, 6, GPS_MARKER)),
        ("image/png", png_with_text(8, 8, GPS_MARKER)),
        ("image/png", polyglot),
        ("image/gif", gif_trailing),
    ] {
        let record = org_file(&fx, mime, &raw);
        let refused = ship(&fx, &record, &raw).unwrap_err();
        assert_eq!(
            refused.code,
            ErrorCode::InvalidRequest,
            "{mime}: {refused:?}"
        );
        assert!(!bound(&fx, &record), "{mime}");
    }
    assert_eq!(fx.office.attachment_blob_counts().unwrap(), before);

    // The same pictures as an office keeps them go through.
    for (mime, raw) in [
        ("image/png", clean),
        ("image/gif", animated_clean),
        (
            "image/jpeg",
            media::validate(MediaType::Jpeg, jpeg_with_exif(16, 8, 6, GPS_MARKER))
                .unwrap()
                .bytes,
        ),
        (
            "image/webp",
            media::validate(MediaType::Webp, webp(9, 7)).unwrap().bytes,
        ),
    ] {
        let record = org_file(&fx, mime, &raw);
        ship(&fx, &record, &raw).unwrap_or_else(|e| panic!("{mime}: {e:?}"));
        assert!(bound(&fx, &record), "{mime}");
    }
}

#[test]
fn hq_refuses_a_shipped_name_that_does_not_fit_its_type() {
    let fx = Fixture::new(EngineChoice::Memory);
    let text = b"plain notes".to_vec();
    let png = sample_png(4, 4);
    for (mime, bytes, name, code) in [
        ("text/plain", &text, "evil.html", ErrorCode::Unsupported),
        ("text/plain", &text, "notes", ErrorCode::Unsupported),
        ("image/png", &png, "dropper.exe", ErrorCode::Unsupported),
        ("image/png", &png, "../shot.png", ErrorCode::InvalidRequest),
        ("image/png", &png, " shot.png", ErrorCode::InvalidRequest),
        ("image/png", &png, ".png", ErrorCode::InvalidRequest),
    ] {
        let mut record = org_file(&fx, mime, bytes);
        record.filename = name.into();
        let refused = check(&fx, SATELLITE, &record).unwrap_err();
        assert_eq!(refused.code, code, "{name}: {refused:?}");
    }
    // A name an upload is kept under in another type is fine: an animated `.webp` is kept as
    // GIF, a large still `.png` as JPEG.
    let animated = media::validate(MediaType::Gif, gif(5, 5, 2)).unwrap().bytes;
    let mut record = org_file(&fx, "image/gif", &animated);
    record.filename = "loop.webp".into();
    ship(&fx, &record, &animated).unwrap();
    let mut record = org_file(&fx, "image/png", &png);
    record.mime_type = "image/jpeg".into();
    record.filename = "photo.png".into();
    check(&fx, SATELLITE, &record).unwrap();
}

#[test]
fn hq_refuses_a_record_whose_type_dimensions_or_page_count_are_not_its_bytes() {
    let fx = Fixture::new(EngineChoice::Memory);
    let before = fx.office.attachment_blob_counts().unwrap();
    let png = sample_png(12, 8);
    let pdf = sample_pdf(3, 0);
    let text = b"a,b\n1,2\n".to_vec();
    let mut forged: Vec<(AttachmentRecord, &[u8])> = Vec::new();
    let mut wide = org_file(&fx, "image/png", &png);
    wide.width = Some(13);
    forged.push((wide, &png));
    let mut tall = org_file(&fx, "image/png", &png);
    tall.height = None;
    forged.push((tall, &png));
    let mut pages = org_file(&fx, "application/pdf", &pdf);
    assert_eq!(pages.page_count, Some(3));
    pages.page_count = Some(1);
    forged.push((pages, &pdf));
    let mut sized_text = org_file(&fx, "text/csv", &text);
    sized_text.width = Some(5);
    forged.push((sized_text, &text));
    // A PNG announced as a GIF (named `.gif`): its bytes are not what its type says.
    let mut retyped = org_file(&fx, "image/png", &png);
    retyped.mime_type = "image/gif".into();
    retyped.filename = "shot.gif".into();
    forged.push((retyped, &png));
    for (record, bytes) in &forged {
        let refused = ship(&fx, record, bytes).unwrap_err();
        assert!(
            matches!(
                refused.code,
                ErrorCode::InvalidRequest | ErrorCode::Unsupported
            ),
            "{}: {refused:?}",
            record.mime_type
        );
        assert!(!bound(&fx, record));
    }
    assert_eq!(fx.office.attachment_blob_counts().unwrap(), before);
    // The honest records of the same bytes go through.
    for (mime, bytes) in [
        ("image/png", &png),
        ("application/pdf", &pdf),
        ("text/csv", &text),
    ] {
        let record = org_file(&fx, mime, bytes);
        ship(&fx, &record, bytes).unwrap();
    }
}

#[test]
fn a_new_attachment_never_binds_bytes_hq_holds_without_sending_them() {
    for engine in engines() {
        let fx = Fixture::new(engine.clone());
        // A project file HQ holds, which only the project's team may add to.
        let secret = b"the platform team's roadmap".to_vec();
        let staged = upload(&fx.office, &fx.person, "plan.txt", "text/plain", &secret).unwrap();
        let project_file: AttachmentView = attach_upload(
            &fx.office,
            &fx.person,
            &staged.upload_id,
            AttachmentTargetKind::Project,
            Some(&fx.project),
        )
        .unwrap();
        let counts = fx.office.attachment_blob_counts().unwrap();

        // A compromised satellite announces a new organization-wide file naming those bytes by
        // their hash alone: HQ asks for the bytes and binds nothing without them.
        let claim = shipped(
            &fx.other_person,
            AttachmentTargetKind::Organization,
            None,
            "text/plain",
            &secret,
        );
        assert_eq!(claim.content_sha256.as_str(), project_file.content_sha256);
        let checked = check(&fx, SATELLITE, &claim).unwrap();
        assert!(!checked.present, "{engine:?}: no hash-existence oracle");
        let refused = fx
            .office
            .attachment_ingest_commit(&checked, None)
            .unwrap()
            .unwrap_err();
        assert_eq!(refused.code, ErrorCode::Conflict);
        assert_eq!(
            read_all(&fx.office, &fx.other_person, claim.id.as_str(), 4096)
                .unwrap_err()
                .code,
            ErrorCode::NotFound,
            "{engine:?}: nothing was bound"
        );
        // The same claim into the project itself is refused outright: the uploader may not
        // add to it.
        let into_project = shipped(
            &fx.other_person,
            AttachmentTargetKind::Project,
            Some(&fx.project),
            "text/plain",
            &secret,
        );
        assert_eq!(
            check(&fx, SATELLITE, &into_project).unwrap_err().code,
            ErrorCode::NotAuthorized
        );
        assert_eq!(fx.office.attachment_blob_counts().unwrap(), counts);
    }
}

#[test]
fn two_attachments_of_the_same_bytes_both_send_them_and_are_kept_once() {
    for engine in engines() {
        let fx = Fixture::new(engine.clone());
        let bytes = sample_png(10, 6);
        let (shared_before, private_before) = fx.office.attachment_blob_counts().unwrap();
        let first = org_file(&fx, "image/png", &bytes);
        let second = shipped(
            &fx.person,
            AttachmentTargetKind::Project,
            Some(&fx.project),
            "image/png",
            &bytes,
        );
        assert_eq!(first.content_sha256, second.content_sha256);
        assert_ne!(first.id, second.id);
        ship(&fx, &first, &bytes).unwrap();
        // HQ holds the bytes now, yet the second attachment still sends them.
        ship(&fx, &second, &bytes).unwrap();
        assert_eq!(
            fx.office.attachment_blob_counts().unwrap(),
            (shared_before + 1, private_before),
            "{engine:?}: one copy on disk"
        );
        for record in [&first, &second] {
            assert_eq!(
                read_all(&fx.office, &fx.person, record.id.as_str(), 4096).unwrap(),
                bytes
            );
        }
        // A retry of either is answered from what HQ bound, without its bytes.
        let again = check(&fx, SATELLITE, &second).unwrap();
        assert!(again.present);
        fx.office
            .attachment_ingest_commit(&again, None)
            .unwrap()
            .unwrap();
        // The bound record's type is part of it: the same ID under another type conflicts.
        let mut retyped = second.clone();
        retyped.mime_type = "image/jpeg".into();
        retyped.filename = "shipped.png".into();
        assert_eq!(
            check(&fx, SATELLITE, &retyped).unwrap_err().code,
            ErrorCode::Conflict
        );
    }
}

#[test]
fn the_uploader_is_decided_again_when_the_bytes_arrive() {
    let fx = Fixture::new(EngineChoice::Sqlite(org_office_sqlite::sqlite_backend()));
    let bytes = b"written before the project closed".to_vec();
    let record = shipped(
        &fx.person,
        AttachmentTargetKind::Project,
        Some(&fx.project),
        "text/plain",
        &bytes,
    );
    let checked = check(&fx, SATELLITE, &record).unwrap();
    let counts = fx.office.attachment_blob_counts().unwrap();
    // The project is archived while the bytes are on their way.
    fx.office
        .store_for_test()
        .run(ArchiveProject(Id::parse(&fx.project).unwrap()));
    let refused = fx
        .office
        .attachment_ingest_commit(&checked, Some(bytes))
        .unwrap()
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotAuthorized);
    assert!(!bound(&fx, &record));
    assert_eq!(
        fx.office.attachment_blob_counts().unwrap(),
        counts,
        "nothing is kept for a refused file"
    );
}

#[test]
fn an_empty_shipped_file_is_an_invalid_request_not_too_large() {
    let fx = Fixture::new(EngineChoice::Memory);
    let empty = org_file(&fx, "text/plain", b"");
    assert_eq!(empty.byte_count, 0);
    let refused = check(&fx, SATELLITE, &empty).unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidRequest, "{refused:?}");
}
