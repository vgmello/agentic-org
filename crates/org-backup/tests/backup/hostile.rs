use std::io::Write;
use std::path::Path;

use flate2::{write::GzEncoder, Compression};
use org_backup::{
    verify, BackupError, FileMark, JournalMark, Limits, Manifest, RelPath, VerifyRequest, FORMAT,
    VERSION,
};
use sha2::{Digest, Sha256};
use tar::{EntryType, Header};

use crate::round_trip::{passphrase, take_backup, verify_request};
use crate::support::Org;

const ORG: &str = "019a0000-0000-7000-8000-0000000000a1";
const OFFICE: &str = "019a0000-0000-7000-8000-0000000000a2";

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A manifest listing folder `a` and `files` (path and content) below it.
fn manifest(files: &[(&str, &[u8])]) -> Manifest {
    Manifest {
        format: FORMAT.into(),
        version: VERSION,
        created_at: "2026-11-26T00:00:00.000Z".into(),
        org_id: ORG.into(),
        org_name: "Acme".into(),
        office_id: OFFICE.into(),
        office_role: "hq".into(),
        journal: JournalMark {
            incarnation: OFFICE.into(),
            head_ordinal: 1,
            head_sha256: "0".repeat(64),
            torn_tail: false,
        },
        directories: vec![RelPath::parse("a").unwrap()],
        files: files
            .iter()
            .map(|(path, bytes)| FileMark {
                path: RelPath::parse(path).unwrap(),
                length: bytes.len() as u64,
                sha256: hex(bytes),
            })
            .collect(),
    }
}

/// One crafted member: its raw name, type, declared size, data and link name.
struct Raw<'a> {
    name: &'a [u8],
    kind: EntryType,
    size: u64,
    data: &'a [u8],
    link: &'a [u8],
}

fn raw<'a>(name: &'a str, kind: EntryType, data: &'a [u8]) -> Raw<'a> {
    Raw {
        name: name.as_bytes(),
        kind,
        size: data.len() as u64,
        data,
        link: b"",
    }
}

/// Writes a plain archive of the manifest and `members`, names written as given (no checks).
fn craft(out: &Path, manifest: &Manifest, members: &[Raw<'_>], trailer: &[u8]) {
    let mut gz = GzEncoder::new(std::fs::File::create(out).unwrap(), Compression::fast());
    let manifest = serde_json::to_vec(manifest).unwrap();
    let mut all = vec![raw("manifest.json", EntryType::Regular, &manifest)];
    all.extend(members.iter().map(|m| Raw { ..*m }));
    for member in &all {
        let mut header = Header::new_ustar();
        header.set_entry_type(member.kind);
        header.set_size(member.size);
        header.set_mode(0o600);
        header.set_mtime(0);
        let bytes = header.as_mut_bytes();
        bytes[..member.name.len()].copy_from_slice(member.name);
        bytes[157..157 + member.link.len()].copy_from_slice(member.link);
        header.set_cksum();
        gz.write_all(header.as_bytes()).unwrap();
        gz.write_all(member.data).unwrap();
        let pad = (512 - member.data.len() % 512) % 512;
        gz.write_all(&vec![0u8; pad]).unwrap();
    }
    gz.write_all(&[0u8; 1024]).unwrap();
    gz.write_all(trailer).unwrap();
    gz.finish().unwrap();
}

/// Verifies `archive` with a private work folder, and checks nothing escaped it.
fn refused(dir: &Path, archive: &Path, limits: Limits) -> String {
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let error = verify(&VerifyRequest {
        archive,
        passphrase: None,
        to: None,
        work_dir: Some(&work),
        limits,
    })
    .unwrap_err();
    assert!(!dir.join("escape").exists() && !work.join("escape").exists());
    assert_eq!(
        std::fs::read_dir(&work).unwrap().count(),
        0,
        "work left behind"
    );
    match error {
        BackupError::Invalid(why) => why,
        other => panic!("expected an invalid archive, got {other:?}"),
    }
}

#[test]
fn members_that_climb_out_are_absolute_links_or_devices_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path().canonicalize().unwrap();
    let listed = manifest(&[("a/f", b"data")]);
    let dirm = raw("durable/a", EntryType::Directory, b"");
    let cases: Vec<(Raw<'_>, &str)> = vec![
        (
            raw("durable/a/../../escape", EntryType::Regular, b"data"),
            "climbs out",
        ),
        (raw("/escape", EntryType::Regular, b"data"), "absolute"),
        (raw("../escape", EntryType::Regular, b"data"), "climbs out"),
        (
            Raw {
                link: b"/etc/passwd",
                ..raw("durable/a/f", EntryType::Symlink, b"")
            },
            "link member",
        ),
        (
            Raw {
                link: b"durable/a",
                ..raw("durable/a/f", EntryType::Link, b"")
            },
            "link member",
        ),
        (raw("durable/a/f", EntryType::Fifo, b""), "special member"),
        (raw("durable/a/f", EntryType::Char, b""), "special member"),
        (
            raw("durable/a/f", EntryType::GNULongName, b"durable/escape\0"),
            "special member",
        ),
    ];
    for (n, (member, why)) in cases.into_iter().enumerate() {
        let archive = dir.join(format!("hostile-{n}.tar.gz"));
        craft(
            &archive,
            &listed,
            &[raw("durable/a", EntryType::Directory, b""), member],
            b"",
        );
        let reason = refused(&dir, &archive, Limits::default());
        assert!(reason.contains(why), "case {n}: {reason}");
    }
    // The members in order are fine, which shows the cases above fail for their reason.
    let archive = dir.join("listed.tar.gz");
    craft(
        &archive,
        &listed,
        &[dirm, raw("durable/a/f", EntryType::Regular, b"data")],
        b"",
    );
    let reason = refused(&dir, &archive, Limits::default());
    assert!(reason.contains("journal"), "{reason}");
}

#[test]
fn unlisted_duplicated_reordered_and_missing_members_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path().canonicalize().unwrap();
    let listed = manifest(&[("a/f", b"data"), ("a/g", b"more")]);
    let d = || raw("durable/a", EntryType::Directory, b"");
    let f = || raw("durable/a/f", EntryType::Regular, b"data");
    let g = || raw("durable/a/g", EntryType::Regular, b"more");
    let cases: Vec<(Vec<Raw<'_>>, &str)> = vec![
        (
            vec![d(), f(), raw("durable/a/h", EntryType::Regular, b"x"), g()],
            "not listed",
        ),
        (vec![d(), f(), f(), g()], "duplicated or out of order"),
        (vec![d(), g(), f()], "duplicated or out of order"),
        (vec![d(), f()], "a/g is missing"),
        (
            vec![d(), f(), raw("durable/a/g", EntryType::Regular, b"MORE")],
            "digest",
        ),
        (vec![f(), d(), g()], "duplicated or out of order"),
    ];
    for (n, (members, why)) in cases.into_iter().enumerate() {
        let archive = dir.join(format!("order-{n}.tar.gz"));
        craft(&archive, &listed, &members, b"");
        let reason = refused(&dir, &archive, Limits::default());
        assert!(reason.contains(why), "case {n}: {reason}");
    }
}

#[test]
fn oversized_and_bomb_like_streams_are_refused_within_their_bound() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path().canonicalize().unwrap();
    // A member far larger than its manifest entry.
    let bomb = vec![0u8; 8 << 20];
    let listed = manifest(&[("a/f", b"tiny")]);
    let archive = dir.join("bomb.tar.gz");
    craft(
        &archive,
        &listed,
        &[
            raw("durable/a", EntryType::Directory, b""),
            raw("durable/a/f", EntryType::Regular, &bomb),
        ],
        b"",
    );
    assert!(std::fs::metadata(&archive).unwrap().len() < 1 << 20);
    let reason = refused(&dir, &archive, Limits::default());
    assert!(reason.contains("manifest says 4"), "{reason}");

    // Zeros past the end of the tar stream, far beyond what the manifest accounts for.
    let archive = dir.join("trailer.tar.gz");
    craft(
        &archive,
        &listed,
        &[
            raw("durable/a", EntryType::Directory, b""),
            raw("durable/a/f", EntryType::Regular, b"tiny"),
        ],
        &bomb,
    );
    let reason = refused(&dir, &archive, Limits::default());
    assert!(reason.contains("expands past"), "{reason}");

    // A manifest that declares more than the limits allow is refused before extraction.
    let big = manifest(&[("a/f", &vec![1u8; 4096])]);
    let archive = dir.join("declared.tar.gz");
    craft(&archive, &big, &[], b"");
    let limits = Limits {
        max_total_bytes: 1024,
        ..Limits::default()
    };
    let reason = refused(&dir, &archive, limits);
    assert!(reason.contains("over the 1024-byte limit"), "{reason}");

    // An extended header larger than its bound.
    let archive = dir.join("pax.tar.gz");
    let pax = vec![b'7'; 8192];
    craft(
        &archive,
        &listed,
        &[
            raw("durable/a", EntryType::Directory, b""),
            raw("././@PaxHeader", EntryType::XHeader, &pax),
        ],
        b"",
    );
    let reason = refused(&dir, &archive, Limits::default());
    assert!(reason.contains("extended header"), "{reason}");

    // Not an archive at all.
    let archive = dir.join("text.tar.gz");
    std::fs::write(&archive, b"hello, not a backup").unwrap();
    let reason = refused(&dir, &archive, Limits::default());
    assert!(reason.contains("neither"), "{reason}");
}

#[test]
fn a_flipped_byte_and_a_wrong_or_missing_passphrase_are_detected() {
    let org = Org::seeded();
    let plain = org.root.join("plain.tar.gz");
    let sealed = org.root.join("sealed.tar.gz");
    let key = passphrase();
    take_backup(&org, &plain, None);
    take_backup(&org, &sealed, Some(&key));
    let flip = |path: &Path, at: f64| {
        let mut bytes = std::fs::read(path).unwrap();
        let i = ((bytes.len() as f64) * at) as usize;
        bytes[i] ^= 0x20;
        let out = path.with_extension(format!("flipped-{}", (at * 100.0) as u32));
        std::fs::write(&out, bytes).unwrap();
        out
    };
    for at in [0.3, 0.6, 0.97] {
        let broken = flip(&plain, at);
        assert!(
            verify(&verify_request(&broken, None, None)).is_err(),
            "plain at {at}"
        );
        let broken = flip(&sealed, at);
        let error = verify(&verify_request(&broken, Some(&key), None)).unwrap_err();
        assert!(
            matches!(error, BackupError::Authentication),
            "sealed at {at}: {error}"
        );
    }
    // A cut encrypted archive (its final record lost) never opens.
    let bytes = std::fs::read(&sealed).unwrap();
    let cut = org.root.join("cut.tar.gz");
    std::fs::write(&cut, &bytes[..bytes.len() - 100]).unwrap();
    let error = verify(&verify_request(&cut, Some(&key), None)).unwrap_err();
    assert!(matches!(error, BackupError::Authentication), "{error}");
    let wrong = org_backup::Passphrase::new(b"incorrect horse battery".to_vec()).unwrap();
    let error = verify(&verify_request(&sealed, Some(&wrong), None)).unwrap_err();
    assert!(matches!(error, BackupError::Authentication), "{error}");
    let error = verify(&verify_request(&sealed, None, None)).unwrap_err();
    assert!(matches!(error, BackupError::PassphraseRequired), "{error}");
    assert!(!error.to_string().contains("horse"));
    // The sealed archive's bytes hold none of the private note in the clear.
    let needle = b"rotate the HSM first";
    assert!(!bytes.windows(needle.len()).any(|w| w == needle));
    // A `--to` folder is made only once the archive verifies.
    let to = org.root.join("never-made");
    let broken = flip(&plain, 0.5);
    assert!(verify(&verify_request(&broken, None, Some(&to))).is_err());
    assert!(!to.exists());
}
