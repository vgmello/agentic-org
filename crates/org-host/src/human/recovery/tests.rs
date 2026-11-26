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
        vec![
            "release",
            "--org",
            "Acme",
            "--org-id",
            "0193a5c0-0000-7000-8000-000000000001",
        ],
        vec!["status", "--org"],
    ] {
        let mut out = Vec::new();
        let refused = recovery(
            &args.into_iter().map(str::to_owned).collect::<Vec<_>>(),
            &mut out,
        )
        .unwrap_err();
        assert_eq!(refused.kind, crate::setup_flow::HostCliErrorKind::Usage);
        assert_eq!(refused.help.as_deref(), Some(HELP));
        assert!(out.is_empty());
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

#[test]
fn an_organization_name_resolves_among_the_accounts_linked_organizations_only() {
    let linked = |id: &str, name: &str| ops::LinkedOrg {
        org_id: id.into(),
        name: name.into(),
        employee_id: "0193a5c0-0000-7000-8000-000000000006".into(),
        office_ids: vec![],
    };
    let orgs = [
        linked("0193a5c0-0000-7000-8000-000000000001", "Acme"),
        linked("0193a5c0-0000-7000-8000-000000000002", "Other \u{1b}[31m"),
        // An answer from a host that sends no names.
        linked("0193a5c0-0000-7000-8000-000000000003", ""),
    ];
    for name in ["Acme", "acme", "ACME", " Acme "] {
        assert_eq!(
            resolve_org(&orgs, name).as_deref(),
            Ok("0193a5c0-0000-7000-8000-000000000001"),
            "{name}"
        );
    }
    // Unknown, or empty: refused, naming what the account can see with nothing a
    // terminal would interpret.
    for name in ["Missing", ""] {
        let refused = resolve_org(&orgs, name).unwrap_err().message;
        assert!(refused.contains("\"Acme\""), "{refused}");
        assert!(
            refused.contains("Other") && !refused.contains('\u{1b}'),
            "{refused}"
        );
    }
    assert!(resolve_org(&[], "Acme")
        .unwrap_err()
        .message
        .ends_with("it can see: none"));
    // The key is the host's: a name the host would refuse has none and selects nothing,
    // even where a plain trim-and-lowercase comparison would call the two equal.
    let odd = [linked(
        "0193a5c0-0000-7000-8000-000000000004",
        "Odd\u{200b}Name",
    )];
    assert!(resolve_org(&odd, "odd\u{200b}name").is_err());
    let school = [linked("0193a5c0-0000-7000-8000-000000000005", "ÉCOLE")];
    assert_eq!(
        resolve_org(&school, " école ").as_deref(),
        Ok("0193a5c0-0000-7000-8000-000000000005")
    );
}

#[test]
fn recovery_help_includes_use_when_guidance() {
    assert!(HELP.contains("Use when:"), "{HELP}");
    for args in [
        vec!["--help"],
        vec!["-h"],
        vec!["status", "--help"],
        vec!["status", "-h"],
        vec!["release", "--help"],
        vec!["release", "-h"],
    ] {
        let owned: Vec<String> = args.into_iter().map(str::to_owned).collect();
        let mut out = Vec::new();
        assert_eq!(recovery(&owned, &mut out), Ok(()), "{owned:?}");
        assert_eq!(String::from_utf8(out).unwrap(), format!("{HELP}\n"));
    }
}

#[test]
fn a_closed_standard_output_is_an_error_and_never_a_panic() {
    struct Closed;
    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let refused = recovery(&["--help".to_owned()], &mut Closed).unwrap_err();
    assert!(refused.is_output_closed(), "{refused:?}");
}

fn delivery_status(state: dto::DeliveryState) -> dto::StatusResult {
    dto::StatusResult {
        version: 1,
        org_id: "0193a5c0-0000-7000-8000-000000000001".into(),
        state,
        delivery_ordinal: "2".into(),
        first_shown_ms: Some(1_732_000_000_000),
        last_redelivered_ms: None,
        redelivery_count: "1".into(),
        acknowledged_ms: None,
        acknowledged_by: None,
        audit: vec![dto::Audit {
            ordinal: "1".into(),
            action: "shown".into(),
            account: "Owner\u{1b}[2J".into(),
            at_ms: 1_732_000_000_000,
        }],
        next_after_ordinal: None,
        hq_loss_recovery_supported: false,
    }
}

/// The status reads as words: the state in its wire spelling, times as RFC 3339 UTC or
/// `never`, an absent account as `none`; no `Some(..)`, `None` or variant names, and nothing
/// of the host's words that a terminal would interpret.
#[test]
fn the_status_is_shown_in_words_without_debug_text() {
    let pending = shown(&delivery_status(dto::DeliveryState::AckPending));
    assert_eq!(
        pending[0],
        "Recovery: ack_pending; delivery ordinal 2; first shown 2024-11-19T07:06:40.000Z; \
         last redelivered never; redeliveries 1; acknowledged never by none."
    );
    assert_eq!(
        pending[1],
        "Delivery 1: shown by Owner\\u{1b}[2J at 2024-11-19T07:06:40.000Z"
    );
    let mut acknowledged = delivery_status(dto::DeliveryState::Acknowledged);
    acknowledged.acknowledged_ms = Some(1_732_000_060_000);
    acknowledged.acknowledged_by = Some("Owner".into());
    let lines = shown(&acknowledged);
    assert!(
        lines[0].ends_with("acknowledged 2024-11-19T07:07:40.000Z by Owner."),
        "{}",
        lines[0]
    );
    for line in pending.iter().chain(&lines) {
        for debug in ["Some(", "None", "AckPending", "Acknowledged", "\u{1b}"] {
            assert!(!line.contains(debug), "{line}");
        }
    }
    let mut out = Vec::new();
    show(&acknowledged, &mut out).unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), lines.join("\n") + "\n");
}
