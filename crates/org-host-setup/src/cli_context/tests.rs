use super::*;

const INST: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607101";
const OFF1: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607102";
const OFF2: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607103";

#[test]
fn context_round_trips_owner_only_and_enforces_selection_rules() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cli-context.json");
    assert_eq!(load_from(&path).unwrap(), None);

    // Missing both explicit and saved context fails with actionable hint.
    let err = resolve_selection(None, None, None).unwrap_err();
    assert!(err.contains("--office"), "{err}");

    // Invalid non-UUIDv7 office rejected immediately.
    assert!(CliContext::new(INST.into(), None, "not-uuid".into()).is_err());

    let ctx = CliContext::new(INST.into(), Some("dev".into()), OFF1.into()).unwrap();
    save_to(&path, &ctx).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert_eq!(load_from(&path).unwrap(), Some(ctx.clone()));

    // Saved context resolves when omitted; explicit office overrides.
    assert_eq!(
        resolve_selection(None, None, Some(&ctx)).unwrap(),
        (Some("dev".into()), OFF1.into())
    );
    assert_eq!(
        resolve_selection(Some("other"), Some(OFF2), Some(&ctx)).unwrap(),
        (Some("other".into()), OFF2.into())
    );
    // Mismatched explicit instance without explicit office refuses rather than crossing instances.
    assert!(resolve_selection(Some("other"), None, Some(&ctx)).is_err());

    assert!(clear_at(&path).unwrap());
    assert!(!clear_at(&path).unwrap());
}

/// B-01: `AGENTIC_ORG_CONFIG_DIR` decides where the context lives, before the development
/// base and the account folder; a relative one is refused, never resolved against the
/// working directory; an empty one is unset.
#[test]
fn the_config_dir_variable_places_the_context_and_a_relative_one_is_refused() {
    fn lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        }
    }
    let account = || Some(PathBuf::from("/home/a/acct"));
    let dev = org_host::human::DEV_ACCOUNT_BASE;
    let path =
        |pairs: &[(&str, &str)], development| context_path_in(&lookup(pairs), development, account);
    assert_eq!(
        path(&[(CONFIG_DIR_ENV, "/cfg"), (dev, "/dev-base")], true),
        Ok(PathBuf::from("/cfg/cli-context.json"))
    );
    // The variable is the user's, so a release build honours it too.
    assert_eq!(
        path(&[(CONFIG_DIR_ENV, "/cfg")], false),
        Ok(PathBuf::from("/cfg/cli-context.json"))
    );
    for relative in ["cfg", "./cfg", "../cfg", "~/cfg"] {
        let error = path(&[(CONFIG_DIR_ENV, relative)], true).unwrap_err();
        assert!(error.contains(CONFIG_DIR_ENV), "{relative}: {error}");
    }
    // Empty is unset: the development base, then the account folder.
    assert_eq!(
        path(&[(CONFIG_DIR_ENV, ""), (dev, "/dev-base")], true),
        Ok(PathBuf::from("/dev-base/cli-context.json"))
    );
    assert_eq!(
        path(&[], true),
        Ok(PathBuf::from("/home/a/acct/cli-context.json"))
    );
    // The development base: absolute, and a development build's only.
    assert!(path(&[(dev, "rel")], true).is_err());
    assert!(path(&[(dev, "/dev-base")], false)
        .unwrap_err()
        .contains("release build"));
    assert!(context_path_in(&lookup(&[]), true, || None).is_err());
}

/// B-06: a context file edited to carry control, line-breaking or bidirectional
/// characters (or any name that is not an instance name) fails closed when it is read
/// and is never written.
#[test]
fn a_poisoned_instance_name_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cli-context.json");
    for name in [
        "lab\u{1b}[2J",
        "lab\u{202e}gnp",
        "lab\u{2066}x",
        "lab\u{9b}31m",
        "lab\nnext",
        "Lab",
        "",
        "-lab",
        "a-very-long-instance-name-past-32-bytes",
    ] {
        assert!(
            CliContext::new(INST.into(), Some(name.into()), OFF1.into()).is_err(),
            "{name:?}"
        );
        let poisoned = serde_json::json!({
            "instance_id": INST,
            "instance_name": name,
            "office_id": OFF1,
        });
        fs::write(&path, poisoned.to_string()).unwrap();
        assert!(load_from(&path).is_err(), "{name:?} was loaded");
        let unsaved = CliContext {
            instance_id: INST.into(),
            instance_name: Some(name.into()),
            office_id: OFF1.into(),
        };
        assert!(save_to(&path, &unsaved).is_err(), "{name:?} was saved");
    }
    // A poisoned office ID fails closed the same way.
    let poisoned = serde_json::json!({"instance_id": INST, "office_id": "x\u{202e}y"});
    fs::write(&path, poisoned.to_string()).unwrap();
    assert!(load_from(&path).is_err());
}

fn saved(dir: &Path) -> (PathBuf, CliContext) {
    let path = dir.join(CONTEXT_FILE);
    let ctx = CliContext::new(INST.into(), Some("dev".into()), OFF1.into()).unwrap();
    (path, ctx)
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// P1-2: a saved context that is a symbolic link (live or dangling), a FIFO, a directory or
/// larger than the bound fails closed without being followed, blocked on or read whole.
#[test]
fn a_linked_special_or_oversized_context_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let (path, ctx) = saved(dir.path());
    let elsewhere = dir.path().join("elsewhere.json");
    save_to(&elsewhere, &ctx).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &path).unwrap();
    let error = load_from(&path).unwrap_err();
    assert!(error.contains("not a regular file"), "{error}");
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(dir.path().join("missing"), &path).unwrap();
    assert!(
        load_from(&path).is_err(),
        "a dangling link is not `no context`"
    );
    fs::remove_file(&path).unwrap();
    let status = std::process::Command::new("/usr/bin/mkfifo")
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    let started = std::time::Instant::now();
    assert!(load_from(&path).unwrap_err().contains("not a regular file"));
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(load_from(&path).unwrap_err().contains("not a regular file"));
    fs::remove_dir(&path).unwrap();
    let padded = format!(
        "{}{}",
        serde_json::to_string(&ctx).unwrap(),
        " ".repeat(usize::try_from(MAX_CLI_CONTEXT_BYTES).unwrap())
    );
    fs::write(&path, padded).unwrap();
    assert!(load_from(&path).unwrap_err().contains("larger than 64 KiB"));
}

/// P1-2: the save never opens a name planted in advance (the old `cli-context.json.tmp`
/// link is left alone and its target untouched), leaves no temporary file behind, and
/// replaces a link at the context's own name instead of writing through it.
#[test]
fn the_save_uses_a_fresh_temporary_name_and_never_follows_a_link() {
    let dir = tempfile::tempdir().unwrap();
    let (path, ctx) = saved(dir.path());
    let victim = dir.path().join("victim");
    fs::write(&victim, b"untouched").unwrap();
    let planted = path.with_extension("json.tmp");
    std::os::unix::fs::symlink(&victim, &planted).unwrap();
    save_to(&path, &ctx).unwrap();
    assert_eq!(fs::read(&victim).unwrap(), b"untouched");
    assert_eq!(load_from(&path).unwrap(), Some(ctx.clone()));
    assert_eq!(
        names(dir.path()),
        ["cli-context.json", "cli-context.json.tmp", "victim"]
    );
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&victim, &path).unwrap();
    save_to(&path, &ctx).unwrap();
    assert!(!fs::symlink_metadata(&path)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read(&victim).unwrap(), b"untouched");
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

/// P1-2: a save that cannot be installed (a directory holds the context's name) fails and
/// removes its temporary file; concurrent saves never share a temporary name.
#[test]
fn a_failed_save_cleans_up_and_concurrent_saves_do_not_collide() {
    let dir = tempfile::tempdir().unwrap();
    let (path, ctx) = saved(dir.path());
    fs::create_dir(&path).unwrap();
    fs::write(path.join("keep"), b"x").unwrap();
    assert!(save_to(&path, &ctx).is_err());
    assert_eq!(names(dir.path()), ["cli-context.json"]);
    fs::remove_dir_all(&path).unwrap();
    let writers: Vec<_> = (0..16)
        .map(|_| {
            let (path, ctx) = (path.clone(), ctx.clone());
            std::thread::spawn(move || save_to(&path, &ctx))
        })
        .collect();
    for writer in writers {
        writer.join().unwrap().unwrap();
    }
    assert_eq!(load_from(&path).unwrap(), Some(ctx));
    assert_eq!(names(dir.path()), ["cli-context.json"]);
}
