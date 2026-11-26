use std::fs;

use tempfile::TempDir;

use super::*;
use crate::error::{CliDiagnostic, EXIT_FAILURE, EXIT_USAGE};

fn lookup<'a>(env: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
    move |name| {
        env.iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| OsString::from(value))
    }
}

/// `root/agentic-org.toml` with `manifest`, and `root/a/b` to start from.
fn tree(manifest: &str) -> (TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(MANIFEST_FILE), manifest).unwrap();
    let nested = root.path().join("a").join("b");
    fs::create_dir_all(&nested).unwrap();
    (root, nested)
}

fn select(input: &SelectionInput<'_>, env: &[(&str, &str)]) -> Result<Selection, ShellError> {
    resolve_selection(input, &lookup(env))
}

fn selected(value: &str, tier: Tier) -> Selected {
    Selected {
        value: Some(value.to_owned()),
        tier,
    }
}

#[test]
fn every_tier_wins_over_the_lower_ones() {
    let (_root, start) = tree("instance = \"from-manifest\"\noffice = \"manifest-office\"\n");
    let full = SelectionInput {
        flag_instance: Some("from-flag"),
        flag_office: Some("flag-office"),
        saved_instance: Some("from-saved"),
        saved_office: Some("saved-office"),
        default_instance: Some("default"),
        start_dir: Some(&start),
    };
    let env = [(INSTANCE_ENV, "from-env"), (OFFICE_ENV, "env-office")];

    let all = select(&full, &env).unwrap();
    assert_eq!(all.instance, selected("from-flag", Tier::Flag));
    assert_eq!(all.office, selected("flag-office", Tier::Flag));
    assert_eq!(
        all.manifest, None,
        "flags and env decided: no manifest read"
    );

    let no_flags = SelectionInput {
        flag_instance: None,
        flag_office: None,
        ..full
    };
    let from_env = select(&no_flags, &env).unwrap();
    assert_eq!(from_env.instance, selected("from-env", Tier::Env));
    assert_eq!(from_env.office, selected("env-office", Tier::Env));

    let from_manifest = select(&no_flags, &[]).unwrap();
    assert_eq!(
        from_manifest.instance,
        selected("from-manifest", Tier::Manifest)
    );
    assert_eq!(
        from_manifest.office,
        selected("manifest-office", Tier::Manifest)
    );
    assert_eq!(
        from_manifest.manifest.as_deref(),
        Some(_root.path().join(MANIFEST_FILE).as_path()),
        "discovered from a nested directory"
    );

    let no_manifest = SelectionInput {
        start_dir: None,
        ..no_flags
    };
    let from_saved = select(&no_manifest, &[]).unwrap();
    assert_eq!(from_saved.instance, selected("from-saved", Tier::Saved));
    assert_eq!(from_saved.office, selected("saved-office", Tier::Saved));

    let nothing = SelectionInput {
        default_instance: Some("default"),
        ..SelectionInput::default()
    };
    let defaults = select(&nothing, &[]).unwrap();
    assert_eq!(defaults.instance, selected("default", Tier::Default));
    assert_eq!(
        defaults.office,
        Selected {
            value: None,
            tier: Tier::Default
        }
    );
}

#[test]
fn values_mix_tiers() {
    let (_root, start) = tree("office = \"manifest-office\"\n");
    let input = SelectionInput {
        flag_instance: Some("flagged"),
        saved_office: Some("saved-office"),
        start_dir: Some(&start),
        ..SelectionInput::default()
    };
    let mixed = select(&input, &[]).unwrap();
    assert_eq!(mixed.instance, selected("flagged", Tier::Flag));
    assert_eq!(mixed.office, selected("manifest-office", Tier::Manifest));
}

#[test]
fn empty_env_and_saved_are_unset() {
    let input = SelectionInput {
        saved_instance: Some(""),
        default_instance: Some("default"),
        ..SelectionInput::default()
    };
    let chosen = select(&input, &[(INSTANCE_ENV, ""), (OFFICE_ENV, "")]).unwrap();
    assert_eq!(chosen.instance, selected("default", Tier::Default));
    assert_eq!(chosen.office.tier, Tier::Default);
}

/// B-06: a saved context edited to carry control, line-breaking or bidirectional characters
/// fails closed (exit 1) and the error never repeats it, whatever tier would decide.
#[test]
fn a_poisoned_saved_value_fails_closed_and_is_not_echoed() {
    for bad in [
        "lab\x1b[2J",
        "lab\u{202e}gnp",
        "lab\u{2066}x\u{2069}",
        "lab\u{9b}31m",
        "lab\u{2028}x",
        &"lab".repeat(50),
    ] {
        for input in [
            SelectionInput {
                saved_instance: Some(bad),
                ..SelectionInput::default()
            },
            SelectionInput {
                saved_office: Some(bad),
                ..SelectionInput::default()
            },
            SelectionInput {
                flag_instance: Some("flagged"),
                flag_office: Some("flagged-office"),
                saved_instance: Some(bad),
                ..SelectionInput::default()
            },
        ] {
            let error = select(&input, &[]).unwrap_err();
            assert!(
                matches!(error, ShellError::InvalidConfig { .. }),
                "{bad:?}: {error:?}"
            );
            assert_eq!(error.exit_code(), EXIT_FAILURE);
            assert!(error.to_string().contains("saved context"), "{error}");
            assert!(!error.to_string().contains("lab"), "echoed: {error}");
        }
    }
}

#[test]
fn invalid_env_and_flags_are_not_echoed() {
    let input = SelectionInput::default();
    for bad in ["s3cr3t\x1b[31m", "s3cr3t\u{202e}", &"s3cr3t".repeat(30)] {
        let error = select(&input, &[(INSTANCE_ENV, bad)]).unwrap_err();
        assert!(
            matches!(error, ShellError::InvalidConfig { .. }),
            "{error:?}"
        );
        assert_eq!(error.exit_code(), EXIT_FAILURE);
        assert!(
            !format!("{error} {:?}", error.help()).contains("s3cr3t"),
            "{error}"
        );
        assert!(error.to_string().contains(INSTANCE_ENV));
    }
    let flagged = SelectionInput {
        flag_office: Some("s3cr3t\n"),
        ..SelectionInput::default()
    };
    let error = select(&flagged, &[]).unwrap_err();
    assert!(
        matches!(error, ShellError::InvalidArguments { .. }),
        "{error:?}"
    );
    assert_eq!(error.exit_code(), EXIT_USAGE);
    assert!(!error.to_string().contains("s3cr3t") && error.to_string().contains(OFFICE_FLAG));
}

#[test]
fn malformed_manifest_is_an_error_without_its_text() {
    let cases = [
        ("token = \"s3cr3t\"\n", 1, 1),
        ("instance = \"ok\"\noffice = 5\n# s3cr3t\n", 2, 10),
        ("instance = \"ok\"\ns3cr3t-garbage !!!\n", 2, 16),
        ("instance = 5 # s3cr3t\n", 1, 12),
        ("[s3cr3t]\nx = 1\n", 1, 2),
    ];
    for (text, line, column) in cases {
        let (root, start) = tree(text);
        let input = SelectionInput {
            start_dir: Some(&start),
            ..SelectionInput::default()
        };
        let error = select(&input, &[]).unwrap_err();
        let shown = format!("{error} | {:?} | {error:?}", error.help());
        assert!(!shown.contains("s3cr3t"), "{shown}");
        assert!(
            !shown.contains("unknown field") && !shown.contains("invalid type"),
            "{shown}"
        );
        let path = root.path().join(MANIFEST_FILE).display().to_string();
        assert_eq!(
            error.to_string(),
            format!("{path}: not a valid agentic-org.toml at line {line}, column {column}"),
            "{text:?}"
        );
        assert_eq!(error.code(), "cli::config::invalid");
        assert_eq!(error.exit_code(), EXIT_FAILURE);
    }
}

#[test]
fn bad_manifest_values_are_not_echoed() {
    let long = format!("instance = \"{}\"\n", "s3cr3t".repeat(30));
    for text in [
        "instance = \"s3cr3t\\u001b[31m\"\n",
        "office = \"s3cr3t\\u202e\"\n",
        "office = \"\"\n",
        long.as_str(),
    ] {
        let (_root, start) = tree(text);
        let error = load_manifest(&find_manifest(&start).unwrap()).unwrap_err();
        assert!(
            matches!(error, ShellError::InvalidConfig { .. }),
            "{error:?}"
        );
        assert!(!format!("{error:?}").contains("s3cr3t"), "{error:?}");
    }
}

#[test]
fn manifest_limits() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(MANIFEST_FILE);
    let mut big = String::from("instance = \"x\"\n");
    big.push_str(&"# padding\n".repeat(7000));
    fs::write(&path, big).unwrap();
    let error = load_manifest(&path).unwrap_err();
    assert!(error.to_string().contains("larger than 64 KiB"), "{error}");

    fs::write(&path, b"instance = \"ok\"\noffice = \"s3cr3t\xff\"\n").unwrap();
    let error = load_manifest(&path).unwrap_err();
    assert!(
        error.to_string().ends_with("at line 2, column 17"),
        "{error}"
    );
    assert!(!format!("{error:?}").contains("s3cr3t"));
}

#[test]
fn valid_manifests() {
    let (_root, start) = tree("# comment\ninstance = \"prod\"\n");
    let manifest = load_manifest(&find_manifest(&start).unwrap()).unwrap();
    assert_eq!(
        manifest,
        Manifest {
            instance: Some("prod".into()),
            office: None,
        }
    );
    let (_root, start) = tree("");
    assert_eq!(
        load_manifest(&find_manifest(&start).unwrap()).unwrap(),
        Manifest::default()
    );
}

#[test]
fn discovery_takes_the_nearest_file() {
    let (root, start) = tree("instance = \"outer\"\n");
    let inner = root.path().join("a");
    fs::write(inner.join(MANIFEST_FILE), "instance = \"inner\"\n").unwrap();
    // A directory with the manifest's name is not a manifest.
    fs::create_dir(start.join(MANIFEST_FILE)).unwrap();
    assert_eq!(find_manifest(&start), Some(inner.join(MANIFEST_FILE)));
    let input = SelectionInput {
        start_dir: Some(&start),
        ..SelectionInput::default()
    };
    assert_eq!(
        select(&input, &[]).unwrap().instance,
        selected("inner", Tier::Manifest)
    );
    let empty = tempfile::tempdir().unwrap();
    // The temporary directory's ancestors are not expected to hold a manifest.
    assert_eq!(find_manifest(&empty.path().join("missing")), None);
}

#[test]
fn a_directory_is_not_loaded() {
    let root = tempfile::tempdir().unwrap();
    let error = load_manifest(root.path()).unwrap_err();
    assert!(error.to_string().contains("not a regular file"), "{error}");
    let missing = load_manifest(&root.path().join("nope.toml")).unwrap_err();
    assert!(matches!(missing, ShellError::Io { .. }));
}

/// P1-2: a manifest that is a symbolic link (to a file, dangling) or a FIFO is refused by
/// name, never followed, never blocked on, and never skipped for one further up.
#[test]
fn a_linked_or_special_manifest_is_refused_and_not_skipped() {
    let (root, start) = tree("instance = \"outer\"\n");
    let elsewhere = root.path().join("elsewhere.toml");
    fs::write(&elsewhere, "instance = \"redirected\"\n").unwrap();
    let inner = start.join(MANIFEST_FILE);
    for target in [elsewhere.clone(), root.path().join("missing.toml")] {
        let _ = fs::remove_file(&inner);
        std::os::unix::fs::symlink(&target, &inner).unwrap();
        assert_eq!(find_manifest(&start), Some(inner.clone()));
        let error = load_manifest(&inner).unwrap_err();
        assert!(error.to_string().contains("not a regular file"), "{error}");
        let input = SelectionInput {
            start_dir: Some(&start),
            ..SelectionInput::default()
        };
        assert!(
            select(&input, &[]).is_err(),
            "a linked manifest fell through"
        );
    }
    fs::remove_file(&inner).unwrap();
    let status = std::process::Command::new("/usr/bin/mkfifo")
        .arg(&inner)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(find_manifest(&start), Some(inner.clone()));
    let error = load_manifest(&inner).unwrap_err();
    assert!(error.to_string().contains("not a regular file"), "{error}");
    let input = SelectionInput {
        start_dir: Some(&start),
        ..SelectionInput::default()
    };
    assert!(select(&input, &[]).is_err(), "a FIFO manifest fell through");
}

#[test]
fn positions() {
    assert_eq!(line_column("abc", 0), (1, 1));
    assert_eq!(line_column("abc", 2), (1, 3));
    assert_eq!(line_column("a\nbc", 3), (2, 2));
    assert_eq!(line_column("é\nx", 3), (2, 1));
    assert_eq!(line_column("ab", 99), (1, 3));
    assert_eq!(Tier::Manifest.as_str(), "manifest");
}
