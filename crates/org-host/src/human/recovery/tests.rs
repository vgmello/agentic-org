use super::*;
#[test]
fn export_creates_one_private_exact_file_and_never_overwrites_or_follows_symlink() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("public.package");
    export(&path, b"encrypted public fixture").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"encrypted public fixture");
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(export(&path, b"replacement").is_err());
    let link = dir.path().join("link");
    symlink(&path, &link).unwrap();
    assert!(export(&link, b"replacement").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"encrypted public fixture");
    let dirlink = dir.path().join("dirlink");
    symlink(dir.path(), &dirlink).unwrap();
    assert!(export(&dirlink.join("nope"), b"public").is_err());
    assert!(!dir.path().join("nope").exists());
    assert!(export(Path::new("relative"), b"public").is_err());
    assert!(export(&dir.path().join("over"), &vec![0; 4476]).is_err());
}
#[test]
fn malformed_cli_arguments_do_not_collect_credentials_or_open_a_socket() {
    for args in [
        vec!["bad"],
        vec!["release", "--redeliver", "--redeliver"],
        vec!["status", "--package", "/tmp/x"],
        vec!["release", "--force"],
        vec!["release", "--login"],
        vec!["release", "--login", "one", "--login", "two"],
    ] {
        assert!(recovery(&args.into_iter().map(str::to_owned).collect::<Vec<_>>()).is_err());
    }
}

#[test]
fn an_explicit_recovery_target_takes_only_a_canonical_uuid_v7_instance() {
    let target = |instance: &str| {
        let options = BTreeMap::from([
            ("--host-socket", "/tmp/agentic-org-test/host.sock"),
            ("--instance-id", instance),
        ]);
        recovery_target(&options).map(|t| t.instance)
    };
    let canonical = "01936b2e-1e5a-7c3d-8a4b-0123456789ab";
    assert_eq!(target(canonical).as_deref(), Ok(canonical));
    for refused in [
        "01JD5Z00000000000000000NST",           // a ULID
        "01936B2E-1E5A-7C3D-8A4B-0123456789AB", // uppercase UUIDv7
        "01936b2e-1e5a-4c3d-8a4b-0123456789ab", // version 4
    ] {
        assert!(target(refused).is_err(), "{refused}");
    }
    let relative = BTreeMap::from([("--host-socket", "host.sock"), ("--instance-id", canonical)]);
    assert!(recovery_target(&relative).is_err());
}
