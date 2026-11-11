use org_supervise::recovery::probe::{ErrnoName, ErrorKindName, Reason};

fn fixed_vectors() -> Vec<(&'static str, Reason, bool)> {
    vec![
        ("kill", Reason::Kill, false),
        ("checkpoint", Reason::Checkpoint, false),
        ("scan", Reason::Scan, false),
        ("reap", Reason::Reap, false),
        ("ownership_lost", Reason::OwnershipLost, false),
        ("deadline", Reason::Deadline, false),
        ("scanner_unavailable", Reason::ScannerUnavailable, false),
        ("inventory_overflow", Reason::InventoryOverflow, true),
        ("recovery: deadline", Reason::RecoveryDeadline, false),
        ("permissions", Reason::Permissions, true),
        ("foreign", Reason::Foreign, true),
        ("mount", Reason::Mount, true),
        ("uchg", Reason::Uchg, true),
        ("depth", Reason::Depth, true),
        ("replaced", Reason::Replaced, false),
        ("budget", Reason::Budget, false),
    ]
}

#[test]
fn fixed_probe_reason_text_has_exact_public_roundtrips() {
    let vectors = fixed_vectors();
    assert_eq!(vectors.len(), 16);
    for (text, expected, _) in vectors {
        assert_eq!(Reason::parse(text), expected, "{text}");
        assert_eq!(expected.to_string(), text);
        assert_eq!(Reason::parse(&expected.to_string()), expected);
    }
}

#[test]
fn only_the_six_persistent_probe_reasons_require_manual_action() {
    let vectors = fixed_vectors();
    assert_eq!(
        vectors
            .iter()
            .filter(|(_, _, persistent)| *persistent)
            .count(),
        6
    );
    for (text, reason, persistent) in vectors {
        assert_eq!(reason.is_persistent(), persistent, "{text}");
        assert!(!Reason::Unknown(text.to_owned()).is_persistent());
    }
}

#[test]
fn parameterized_probe_reasons_use_only_frozen_grammar() {
    let vectors = [
        (
            "recovery: NotFound",
            Reason::Recovery(ErrorKindName::parse("NotFound").unwrap()),
        ),
        (
            "recovery: FutureKind",
            Reason::Recovery(ErrorKindName::parse("FutureKind").unwrap()),
        ),
        (
            "recovery: Permissions",
            Reason::Recovery(ErrorKindName::parse("Permissions").unwrap()),
        ),
        (
            "anchor: notfound",
            Reason::Anchor(ErrorKindName::parse("notfound").unwrap()),
        ),
        (
            "anchor: A",
            Reason::Anchor(ErrorKindName::parse("A").unwrap()),
        ),
        (
            "anchor: z",
            Reason::Anchor(ErrorKindName::parse("z").unwrap()),
        ),
        (
            "anchor: Mount",
            Reason::Anchor(ErrorKindName::parse("Mount").unwrap()),
        ),
        (
            "error: EACCES",
            Reason::Error(ErrnoName::parse("EACCES").unwrap()),
        ),
        (
            "error: EIO",
            Reason::Error(ErrnoName::parse("EIO").unwrap()),
        ),
        ("error: E1", Reason::Error(ErrnoName::parse("E1").unwrap())),
        ("error: E0", Reason::Error(ErrnoName::parse("E0").unwrap())),
        (
            "error: EA9Z",
            Reason::Error(ErrnoName::parse("EA9Z").unwrap()),
        ),
        (
            "error: errno 1",
            Reason::Error(ErrnoName::parse("errno 1").unwrap()),
        ),
        (
            "error: errno 2",
            Reason::Error(ErrnoName::parse("errno 2").unwrap()),
        ),
        (
            "error: errno 2147483646",
            Reason::Error(ErrnoName::parse("errno 2147483646").unwrap()),
        ),
        (
            "error: errno 2147483647",
            Reason::Error(ErrnoName::parse("errno 2147483647").unwrap()),
        ),
    ];
    assert_eq!(vectors.len(), 16);
    for (text, expected) in vectors {
        assert_eq!(Reason::parse(text), expected, "{text}");
        assert_eq!(expected.to_string(), text);
        assert_eq!(Reason::parse(text).to_string(), text);
        assert!(!expected.is_persistent());
    }
}

#[test]
fn malformed_probe_parameters_are_lossless_unknown() {
    let vectors = [
        "recovery: ",
        "anchor: ",
        "error: ",
        "Recovery: NotFound",
        "ANCHOR: NotFound",
        "Anchor: NotFound",
        "Error: EIO",
        "recovery:NotFound",
        "anchor:\tNotFound",
        "error:EIO",
        "recovery:  NotFound",
        "anchor:  NotFound",
        "error:  EIO",
        "recovery: NotFound ",
        "anchor: NotFound\n",
        "error: EIO\0",
        "recovery: Not_Found",
        "anchor: NotFound1",
        "recovery: Not1Found",
        "recovery: 1NotFound",
        "anchor: NötFound",
        "recovery: 权限",
        "error: E",
        "error: eIO",
        "error: EAa",
        "error: EA_A",
        "error: EÉ",
        "error: errno ",
        "error: errno 0",
        "error: errno 00",
        "error: errno 01",
        "error: errno +1",
        "error: errno -1",
        "error: errno -2147483648",
        "error: errno 2147483648",
        "error: errno 9999999999999999999999999999999999",
        "error: errno 1 ",
        "error: errno  1",
        "error: errno 1\n",
        "error: errno ١",
        "error: errno 1a",
        "error: errno 1.0",
        "error: errno 1+1",
        "error: Errno 1",
        "future: EIO",
        "busy",
    ];
    assert_eq!(vectors.len(), 46);
    for text in vectors {
        assert_eq!(
            Reason::parse(text),
            Reason::Unknown(text.to_owned()),
            "{text:?}"
        );
        assert_eq!(Reason::parse(text).to_string(), text);
        assert!(!Reason::parse(text).is_persistent());
    }
}

#[test]
fn validated_probe_parameters_preserve_their_text() {
    for text in ["A", "z", "FutureKind", "notfound", "aZ"] {
        let name = ErrorKindName::parse(text).unwrap();
        assert_eq!(name.as_str(), text);
        assert_eq!(
            Reason::Recovery(name.clone()).to_string(),
            format!("recovery: {text}")
        );
        assert_eq!(Reason::Anchor(name).to_string(), format!("anchor: {text}"));
    }
    for text in [
        "",
        "Not1Found",
        "1NotFound",
        "Not_Found",
        " NotFound",
        "NotFound ",
        "NötFound",
        "\0",
    ] {
        assert_eq!(ErrorKindName::parse(text), None, "{text:?}");
    }
    for text in ["EIO", "E1", "EA9Z", "errno 1", "errno 2147483647"] {
        let name = ErrnoName::parse(text).unwrap();
        assert_eq!(name.as_str(), text);
        assert_eq!(Reason::Error(name).to_string(), format!("error: {text}"));
    }
    for text in [
        "",
        "E",
        "eIO",
        "EAa",
        "EA_A",
        "EÉ",
        "errno ",
        "errno 0",
        "errno 01",
        "errno +1",
        "errno -1",
        "errno 2147483648",
        "errno 1a",
        "errno ١",
    ] {
        assert_eq!(ErrnoName::parse(text), None, "{text:?}");
    }
}

#[test]
fn unknown_probe_reasons_keep_every_byte_and_stay_nonpersistent() {
    for text in [
        "",
        "busy",
        "Kill",
        "KILL",
        "permissions ",
        " inventory_overflow",
        "future_reason",
        "\0future\nreason",
        "权限",
    ] {
        assert_eq!(Reason::parse(text), Reason::Unknown(text.to_owned()));
        assert_eq!(Reason::parse(text).to_string(), text);
        assert!(!Reason::parse(text).is_persistent(), "{text:?}");
    }
    for (text, _, _) in fixed_vectors() {
        for near in [text.to_uppercase(), format!("{text} "), format!(" {text}")] {
            assert_eq!(Reason::parse(&near), Reason::Unknown(near.clone()));
            assert_eq!(Reason::parse(&near).to_string(), near);
            assert!(!Reason::parse(&near).is_persistent());
        }
    }
}
