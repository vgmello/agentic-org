//! HQ's private file space (`TASK-98.1`): `private/user-space.db` runs in WAL mode, so a backup
//! carries it as a consistent `VACUUM INTO` copy (never its `-wal`, `-shm` or `-journal`) and a
//! verification runs SQLite's integrity check on it; unattached uploads in
//! `attachment-staging/` are disposable and never carried. An archive that lists the database
//! under another spelling, one of its sidecars or a staged upload is refused.

use std::path::Path;

use org_backup::{restore_in_place, verify, Limits, RestoreRequest};
use org_office::Office;
use org_proto::attachments::{
    AttachmentAttachParams, AttachmentInspectParams, AttachmentReadChunkParams,
    AttachmentTargetKind, AttachmentUploadBeginParams, AttachmentUploadChunkParams,
    AttachmentUploadRef, AttachmentView,
};

use crate::hostile::refused_listing;
use crate::round_trip::{take_backup, verify_request};
use crate::support::Org;

const SIDECARS: [&str; 3] = ["-wal", "-shm", "-journal"];

/// Standard base64 with padding (the test crate carries no encoder of its own).
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Begins an upload of `bytes` as `actor`, sends them all and, when `commit`, commits it.
fn stage(office: &Office, actor: &str, filename: &str, bytes: &[u8], commit: bool) -> String {
    let begun = office
        .attachment_upload_begin(
            actor,
            AttachmentUploadBeginParams {
                filename: filename.into(),
                mime_type: "text/plain".into(),
                byte_count: bytes.len() as u64,
            },
        )
        .unwrap()
        .unwrap();
    let upload_id = begun.upload.upload_id;
    office
        .attachment_upload_chunk(
            actor,
            AttachmentUploadChunkParams {
                upload_id: upload_id.clone(),
                offset: 0,
                data_base64: base64(bytes),
            },
        )
        .unwrap()
        .unwrap();
    if commit {
        office
            .attachment_upload_commit(
                actor,
                AttachmentUploadRef {
                    upload_id: upload_id.clone(),
                },
            )
            .unwrap()
            .unwrap();
    }
    upload_id
}

fn private_file(office: &Office, actor: &str, bytes: &[u8]) -> AttachmentView {
    let upload_id = stage(office, actor, "mine.txt", bytes, true);
    office
        .attachment_attach(
            actor,
            AttachmentAttachParams {
                command_id: office.next_uuid_v7(),
                upload_id: Some(upload_id),
                source_attachment_id: None,
                target_kind: AttachmentTargetKind::UserPrivate,
                target_id: None,
                title: None,
                description: None,
                artifact_status: None,
            },
        )
        .unwrap()
        .unwrap()
        .attachment
}

fn sidecar(db: &Path, suffix: &str) -> std::path::PathBuf {
    let mut name = db.as_os_str().to_os_string();
    name.push(suffix);
    name.into()
}

#[test]
fn a_wal_mode_private_space_is_snapshotted_without_sidecars_and_verifies_on_restore() {
    let org = Org::seeded();
    let db = org.durable().join("private/user-space.db");
    let office = org.open();
    let mine = private_file(&office, &org.owner, b"only in the wal until a checkpoint");
    // An upload still waiting to be attached: disposable, never carried.
    stage(&office, &org.owner, "pending.txt", b"pending", false);
    let staging = office.attachment_staging_root();
    assert!(staging.is_dir());
    // The row is committed into the WAL; keep the three files as they are now, with the
    // office still open, so the stopped office is left exactly like a crash after the write.
    let wal = std::fs::read(sidecar(&db, "-wal")).unwrap();
    assert!(!wal.is_empty(), "the private file's row is in the WAL");
    let main = std::fs::read(&db).unwrap();
    let shm = std::fs::read(sidecar(&db, "-shm")).unwrap();
    office.close().unwrap();
    std::fs::write(&db, main).unwrap();
    std::fs::write(sidecar(&db, "-wal"), wal).unwrap();
    std::fs::write(sidecar(&db, "-shm"), shm).unwrap();

    let out = org.root.join("private.backup.tar.gz");
    let report = take_backup(&org, &out, None);
    let paths: Vec<&str> = report
        .manifest
        .files
        .iter()
        .map(|f| f.path.as_str())
        .chain(report.manifest.directories.iter().map(|d| d.as_str()))
        .collect();
    assert!(paths.contains(&"private/user-space.db"), "{paths:?}");
    assert!(
        !paths
            .iter()
            .any(|p| SIDECARS.iter().any(|s| p.ends_with(s))),
        "{paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.contains("attachment-staging")),
        "{paths:?}"
    );

    let side = org.root.join("verified");
    let verified = verify(&verify_request(&out, None, Some(&side))).expect("it verifies");
    assert!(verified.private_user_space_checked);
    assert!(verified.private_notes_checked);
    let copy = side.join("durable/private/user-space.db");
    assert!(copy.is_file());
    for suffix in SIDECARS {
        assert!(!sidecar(&copy, suffix).exists(), "{suffix}");
    }

    std::fs::remove_dir_all(org.durable()).unwrap();
    restore_in_place(&RestoreRequest {
        archive: &out,
        passphrase: None,
        office_home: &org.office_home,
        org: &org.org_id,
        office: None,
        limits: Limits::default(),
    })
    .expect("it restores");
    assert!(!staging.exists(), "no staged upload comes back");
    let office = org.open();
    let inspected = office
        .attachment_inspect(
            &org.owner,
            AttachmentInspectParams {
                attachment_id: mine.id.clone(),
            },
        )
        .unwrap()
        .unwrap()
        .attachment;
    assert_eq!(inspected, mine);
    let read = office
        .attachment_read_chunk(
            &org.owner,
            AttachmentReadChunkParams {
                attachment_id: mine.id.clone(),
                offset: 0,
                max_bytes: None,
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        read.data_base64,
        base64(b"only in the wal until a checkpoint")
    );
    office.close().unwrap();
}

#[test]
fn an_archive_listing_the_private_space_otherwise_or_a_staged_upload_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path().canonicalize().unwrap();
    let private = vec!["private".to_owned()];
    for (n, spelling) in ["private/User-Space.db", "private/USER-SPACE.DB"]
        .into_iter()
        .enumerate()
    {
        let reason = refused_listing(
            &dir,
            &format!("space-case-{n}"),
            &private,
            &[spelling.into()],
        );
        assert!(reason.contains("another spelling"), "{spelling}: {reason}");
    }
    for (n, file) in [
        "user-space.db-wal",
        "user-space.db-WAL",
        "User-Space.db-Shm",
        "user-space.db-journal",
    ]
    .into_iter()
    .enumerate()
    {
        let reason = refused_listing(
            &dir,
            &format!("space-sidecar-{n}"),
            &private,
            &[format!("private/{file}")],
        );
        assert!(
            reason.contains("a path a backup never carries"),
            "{file}: {reason}"
        );
    }
    let office = "local/offices/019a0000-0000-7000-8000-0000000000a2";
    for (n, staging) in ["attachment-staging", "Attachment-Staging"]
        .into_iter()
        .enumerate()
    {
        let folders = vec![
            "local".to_owned(),
            "local/offices".into(),
            office.into(),
            format!("{office}/{staging}"),
        ];
        let file = format!("{office}/{staging}/x");
        let reason = refused_listing(&dir, &format!("staging-{n}"), &folders, &[file]);
        assert!(
            reason.contains("a path a backup never carries"),
            "{staging}: {reason}"
        );
        let reason = refused_listing(&dir, &format!("staging-folder-{n}"), &folders, &[]);
        assert!(
            reason.contains("a path a backup never carries"),
            "{staging}: {reason}"
        );
    }
}
