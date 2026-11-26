use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};

use super::*;

fn candidate(name: &str, help: Option<&str>) -> CachedCandidate {
    CachedCandidate {
        name: name.to_owned(),
        help: help.map(str::to_owned),
    }
}

fn entries(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn round_trip() {
    let root = tempfile::tempdir().unwrap();
    let cache = CompletionCache::new(root.path());
    assert_eq!(cache.dir(), root.path());
    let list = vec![
        candidate("acme", Some("Acme Corp")),
        candidate("beta", None),
    ];
    cache.store(&["prod", "org_1"], "projects", &list).unwrap();
    assert_eq!(cache.load(&["prod", "org_1"], "projects"), list);
    assert!(
        cache.load(&["prod"], "projects").is_empty(),
        "partitions are separate"
    );
    assert!(
        cache.load(&["prod", "org_1"], "offices").is_empty(),
        "kinds are separate"
    );
    let dir = root.path().join("completion").join("prod").join("org_1");
    assert_eq!(
        entries(&dir),
        BTreeSet::from(["projects.json".to_owned(), "projects.lock".to_owned()])
    );
    cache
        .store(&["prod", "org_1"], "projects", &list[..1])
        .unwrap();
    assert_eq!(
        cache.load(&["prod", "org_1"], "projects"),
        &list[..1],
        "replaced"
    );
}

/// B-05: the folders that list the names are the user's alone (`0700`): made so from the
/// cache directory down when it is missing, and tightened when they exist more open. A
/// cache directory that exists is the caller's and is left as it is.
#[test]
fn cache_folders_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let root = tempfile::tempdir().unwrap();
    let fresh = root.path().join("new-cache");
    let cache = CompletionCache::new(&fresh);
    cache
        .store(&["prod", "org_1"], "projects", &[candidate("a", None)])
        .unwrap();
    for dir in [
        fresh.clone(),
        fresh.join("completion"),
        fresh.join("completion/prod"),
        fresh.join("completion/prod/org_1"),
    ] {
        assert_eq!(mode(&dir), 0o700, "{}", dir.display());
    }
    // An existing, more open cache: its listing folders are tightened, the root is not.
    let shared = root.path().join("shared");
    fs::create_dir_all(shared.join("completion/prod")).unwrap();
    for dir in [
        &shared,
        &shared.join("completion"),
        &shared.join("completion/prod"),
    ] {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let cache = CompletionCache::new(&shared);
    cache
        .store(&["prod"], "orgs", &[candidate("a", None)])
        .unwrap();
    cache
        .store(&[], "instances", &[candidate("prod", None)])
        .unwrap();
    assert_eq!(mode(&shared.join("completion")), 0o700);
    assert_eq!(mode(&shared.join("completion/prod")), 0o700);
    assert_eq!(
        mode(&shared),
        0o755,
        "the caller's directory is left as it is"
    );
}

fn symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

fn mkfifo(path: &Path) {
    let status = std::process::Command::new("/usr/bin/mkfifo")
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success(), "mkfifo {}", path.display());
}

/// P1-2: a symbolic link anywhere from the cache directory down (the directory itself,
/// `completion`, a partition folder) is refused by the walk, and nothing is written through
/// it; a trailing slash on the cache directory does not make the link followed.
#[test]
fn the_folder_walk_refuses_symbolic_links() {
    let root = tempfile::tempdir().unwrap();
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let list = [candidate("x", None)];

    let linked_root = root.path().join("linked-cache");
    symlink(&outside, &linked_root);
    for dir in [
        linked_root.clone(),
        Path::new(&format!("{}/", linked_root.display())).to_path_buf(),
        Path::new(&format!("{}/.", linked_root.display())).to_path_buf(),
    ] {
        assert!(
            CompletionCache::new(&dir)
                .store(&["p"], "orgs", &list)
                .is_err(),
            "{}",
            dir.display()
        );
    }

    let cache_dir = root.path().join("cache-a");
    fs::create_dir(&cache_dir).unwrap();
    symlink(&outside, &cache_dir.join(COMPLETION_DIR));
    assert!(CompletionCache::new(&cache_dir)
        .store(&["p"], "orgs", &list)
        .is_err());

    let cache_dir = root.path().join("cache-b");
    fs::create_dir_all(cache_dir.join(COMPLETION_DIR)).unwrap();
    symlink(&outside, &cache_dir.join(COMPLETION_DIR).join("p"));
    assert!(CompletionCache::new(&cache_dir)
        .store(&["p"], "orgs", &list)
        .is_err());

    // A more open caller's directory: a linked `completion` is still refused.
    let shared = root.path().join("cache-c");
    fs::create_dir(&shared).unwrap();
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o755)).unwrap();
    symlink(&outside, &shared.join(COMPLETION_DIR));
    assert!(CompletionCache::new(&shared)
        .store(&["p"], "orgs", &list)
        .is_err());

    // A caller's directory that another account could write is refused outright.
    let open_root = root.path().join("cache-d");
    fs::create_dir(&open_root).unwrap();
    fs::set_permissions(&open_root, fs::Permissions::from_mode(0o777)).unwrap();
    assert!(CompletionCache::new(&open_root)
        .store(&["p"], "orgs", &list)
        .is_err());
    assert!(
        entries(&open_root).is_empty(),
        "made something in an open root"
    );

    assert!(entries(&outside).is_empty(), "written through a link");
}

/// `open_completion` fails closed whenever `OwnerOnlyDir` refuses the cache directory: one it
/// has just made that does not then open as the user's alone (a race, or not the account
/// expected), and an existing one that is writable by anyone or another account's, checked on
/// its opened handle. Nothing is made inside a refused directory.
#[test]
fn a_refused_cache_directory_is_never_written_in() {
    let uid = org_platform::fs::effective_uid().unwrap();
    let other = uid.wrapping_add(1);
    let root = tempfile::tempdir().unwrap();
    let refused = |dir: &Path, uid: u32, kind: io::ErrorKind| {
        let error = CompletionCache::new(dir).open_completion(uid).unwrap_err();
        assert_eq!(error.kind(), kind, "{}: {error}", dir.display());
    };

    // Made just now, yet refused when opened: never kept and written in.
    let fresh = root.path().join("fresh");
    refused(&fresh, other, io::ErrorKind::PermissionDenied);
    assert!(fresh.is_dir());
    assert!(
        entries(&fresh).is_empty(),
        "made `completion` in a cache directory that did not verify"
    );

    // Existing and writable by anyone (sticky or not), or another account's.
    for (name, mode, uid) in [
        ("open", 0o777, uid),
        ("sticky", 0o1777, uid),
        ("other-writable", 0o757, uid),
        ("foreign", 0o755, other),
    ] {
        let dir = root.path().join(name);
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(mode)).unwrap();
        refused(&dir, uid, io::ErrorKind::PermissionDenied);
        assert!(entries(&dir).is_empty(), "made something in {name}");
    }

    // A FIFO where the cache directory should be: refused, without blocking.
    let fifo = root.path().join("fifo");
    mkfifo(&fifo);
    refused(&fifo, uid, io::ErrorKind::InvalidInput);

    // The user's own, more open but not writable by others: `completion` is made owner-only.
    let shared = root.path().join("shared");
    fs::create_dir(&shared).unwrap();
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o775)).unwrap();
    CompletionCache::new(&shared).open_completion(uid).unwrap();
    let mode = fs::symlink_metadata(shared.join(COMPLETION_DIR))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
}

/// A reader never follows a linked `completion` or partition folder to planted candidates.
#[test]
fn a_reader_refuses_linked_folders() {
    let root = tempfile::tempdir().unwrap();
    let planted = root.path().join("planted");
    fs::create_dir_all(planted.join("p")).unwrap();
    fs::write(
        planted.join("p").join("orgs.json"),
        r#"[{"name":"evil","help":null}]"#,
    )
    .unwrap();
    let cache_dir = root.path().join("cache");
    fs::create_dir(&cache_dir).unwrap();
    symlink(&planted, &cache_dir.join(COMPLETION_DIR));
    let cache = CompletionCache::new(&cache_dir);
    assert!(
        cache.load(&["p"], "orgs").is_empty(),
        "followed `completion`"
    );
    fs::remove_file(cache_dir.join(COMPLETION_DIR)).unwrap();
    fs::create_dir(cache_dir.join(COMPLETION_DIR)).unwrap();
    symlink(
        &planted.join("p"),
        &cache_dir.join(COMPLETION_DIR).join("p"),
    );
    assert!(
        cache.load(&["p"], "orgs").is_empty(),
        "followed a partition"
    );

    // A linked cache directory is read as `store` sees it: refused, trailing `/` or `/.` too.
    let real = root.path().join("real-cache");
    let list = [candidate("x", None)];
    CompletionCache::new(&real)
        .store(&["p"], "orgs", &list)
        .unwrap();
    assert_eq!(CompletionCache::new(&real).load(&["p"], "orgs"), list);
    let linked = root.path().join("linked-cache");
    symlink(&real, &linked);
    for dir in [
        linked.clone(),
        PathBuf::from(format!("{}/", linked.display())),
        PathBuf::from(format!("{}/.", linked.display())),
    ] {
        assert!(
            CompletionCache::new(&dir).load(&["p"], "orgs").is_empty(),
            "followed the linked cache directory {}",
            dir.display()
        );
        assert!(CompletionCache::new(&dir)
            .store(&["p"], "orgs", &list)
            .is_err());
    }
}

/// P1-2: a lock that is a symbolic link (live or dangling) or a FIFO is refused without
/// following it, creating its target or blocking.
#[test]
fn a_linked_or_special_lock_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let cache = CompletionCache::new(root.path());
    cache.store(&["p"], "orgs", &[]).unwrap();
    let dir = root.path().join(COMPLETION_DIR).join("p");
    let lock_path = dir.join("orgs.lock");
    let elsewhere = root.path().join("elsewhere.lock");
    fs::write(&elsewhere, b"").unwrap();
    let dangling = root.path().join("never-created");
    for target in [&elsewhere, &dangling] {
        fs::remove_file(&lock_path).unwrap();
        symlink(target, &lock_path);
        let error = cache
            .store(&["p"], "orgs", &[candidate("x", None)])
            .unwrap_err();
        assert!(matches!(error, ShellError::Io { .. }), "{error:?}");
    }
    assert!(!dangling.exists(), "the dangling link's target was created");
    fs::remove_file(&lock_path).unwrap();
    mkfifo(&lock_path);
    let started = Instant::now();
    assert!(lock(&lock_path).is_err());
    assert!(started.elapsed() < LOCK_WAIT, "blocked on a FIFO");
    assert!(
        cache.load(&["p"], "orgs").is_empty(),
        "the old list is intact"
    );
}

/// P1-2: a list that is a symbolic link, a FIFO or larger than the bound reads as empty,
/// without following the link or blocking.
#[test]
fn a_linked_special_or_oversized_list_reads_empty() {
    let root = tempfile::tempdir().unwrap();
    let cache = CompletionCache::new(root.path());
    let list = vec![candidate("real", None)];
    cache.store(&["p"], "orgs", &list).unwrap();
    let dir = root.path().join(COMPLETION_DIR).join("p");
    let elsewhere = root.path().join("elsewhere.json");
    fs::write(&elsewhere, r#"[{"name":"planted","help":null}]"#).unwrap();
    fs::remove_file(dir.join("orgs.json")).unwrap();
    symlink(&elsewhere, &dir.join("orgs.json"));
    assert!(cache.load(&["p"], "orgs").is_empty(), "a link was followed");
    fs::remove_file(dir.join("orgs.json")).unwrap();
    mkfifo(&dir.join("orgs.json"));
    let started = Instant::now();
    assert!(cache.load(&["p"], "orgs").is_empty());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "blocked on a FIFO"
    );
    fs::remove_file(dir.join("orgs.json")).unwrap();
    let mut big = String::from("[");
    big.push_str(&" ".repeat(usize::try_from(MAX_LIST_BYTES).unwrap()));
    big.push(']');
    fs::write(dir.join("orgs.json"), big).unwrap();
    assert!(cache.load(&["p"], "orgs").is_empty(), "an oversized list");
}

#[test]
fn keys_cannot_escape_the_cache() {
    assert_eq!(partition_key("prod-1.a_b"), "prod-1.a_b");
    assert_eq!(partition_key(".."), "%2E%2E");
    assert_eq!(partition_key("."), "%2E");
    assert_eq!(partition_key(""), "%");
    assert_eq!(partition_key("../x"), "..%2Fx");
    assert_eq!(partition_key("a/b\\c"), "a%2Fb%5Cc");
    assert_eq!(partition_key("\x1b[2J\n"), "%1B%5B2J%0A");
    assert_eq!(partition_key("50%"), "50%25");
    assert_eq!(partition_key("é"), "%C3%A9");
    for part in ["..", ".", "", "../..", "/", "a/../../b", "\0", "%2E%2E"] {
        let key = partition_key(part);
        assert!(!key.contains('/') && !key.contains('\\'), "{key}");
        assert!(key != ".." && key != "." && !key.is_empty(), "{key}");
        assert!(!key.chars().any(char::is_control), "{key}");
    }
    assert_ne!(partition_key("%2E%2E"), partition_key(".."));

    let root = tempfile::tempdir().unwrap();
    let base = root.path().join("cache");
    let cache = CompletionCache::new(&base);
    let list = vec![candidate("x", None)];
    cache
        .store(&["..", "..", "/etc"], "../../passwd", &list)
        .unwrap();
    assert_eq!(cache.load(&["..", "..", "/etc"], "../../passwd"), list);
    assert_eq!(entries(root.path()), BTreeSet::from(["cache".to_owned()]));
    let inner = base
        .join("completion")
        .join("%2E%2E")
        .join("%2E%2E")
        .join("%2Fetc");
    assert!(inner.join("..%2F..%2Fpasswd.json").is_file());
}

#[test]
fn missing_and_corrupt_lists_are_empty() {
    let root = tempfile::tempdir().unwrap();
    let cache = CompletionCache::new(root.path());
    assert!(cache.load(&["none"], "orgs").is_empty());
    let dir = root.path().join("completion").join("prod");
    fs::create_dir_all(&dir).unwrap();
    for corrupt in ["", "{", "{\"name\":1}", "[{\"nom\":\"x\"}]", "\u{0}garbage"] {
        fs::write(dir.join("orgs.json"), corrupt).unwrap();
        assert!(cache.load(&["prod"], "orgs").is_empty(), "{corrupt:?}");
    }
    fs::remove_file(dir.join("orgs.json")).unwrap();
    fs::create_dir(dir.join("orgs.json")).unwrap();
    assert!(cache.load(&["prod"], "orgs").is_empty(), "a directory");
}

#[test]
fn unsafe_names_are_dropped_and_help_sanitized() {
    let root = tempfile::tempdir().unwrap();
    let cache = CompletionCache::new(root.path());
    let list = vec![
        candidate("good", Some("fine \x1b[31mred")),
        candidate("bad\x1b[2J", None),
        candidate("tab\tbed", None),
        candidate("rtl\u{202e}", None),
        candidate("", None),
        candidate(&"n".repeat(MAX_NAME_BYTES + 1), None),
    ];
    cache.store(&["p"], "orgs", &list).unwrap();
    assert_eq!(
        cache.load(&["p"], "orgs"),
        vec![candidate("good", Some("fine \\u{1b}[31mred"))]
    );
    // A list written by someone else is filtered on the way out too.
    let dir = root.path().join("completion").join("p");
    fs::write(
        dir.join("orgs.json"),
        r#"[{"name":"ok","help":"a\u009bb"},{"name":"x\u0007","help":null}]"#,
    )
    .unwrap();
    assert_eq!(
        cache.load(&["p"], "orgs"),
        vec![candidate("ok", Some("a\\u{9b}b"))]
    );
}

#[test]
fn lists_are_capped() {
    let root = tempfile::tempdir().unwrap();
    let cache = CompletionCache::new(root.path());
    let many: Vec<CachedCandidate> = (0..MAX_CANDIDATES + 50)
        .map(|index| candidate(&format!("n{index}"), None))
        .collect();
    cache.store(&["p"], "orgs", &many).unwrap();
    assert_eq!(cache.load(&["p"], "orgs"), &many[..MAX_CANDIDATES]);
}

#[test]
fn a_held_lock_times_out() {
    let root = tempfile::tempdir().unwrap();
    let cache = CompletionCache::new(root.path());
    cache.store(&["p"], "orgs", &[]).unwrap();
    let held = lock(&root.path().join("completion").join("p").join("orgs.lock")).unwrap();
    let started = Instant::now();
    let error = cache
        .store(&["p"], "orgs", &[candidate("x", None)])
        .unwrap_err();
    assert!(started.elapsed() >= LOCK_WAIT, "{:?}", started.elapsed());
    match error {
        ShellError::Io { source, .. } => assert_eq!(source.kind(), io::ErrorKind::TimedOut),
        other => panic!("{other:?}"),
    }
    drop(held);
    assert!(
        cache.load(&["p"], "orgs").is_empty(),
        "the old list is intact"
    );
}

#[test]
fn concurrent_writers_never_tear_a_list() {
    const WRITERS: usize = 30;
    let root = tempfile::tempdir().unwrap();
    let cache = CompletionCache::new(root.path());
    let sets: Vec<Vec<CachedCandidate>> = (0..WRITERS)
        .map(|writer| {
            (0..40 + writer)
                .map(|index| candidate(&format!("w{writer}-{index}"), Some(&"h".repeat(writer))))
                .collect()
        })
        .collect();
    let sets = Arc::new(sets);
    let barrier = Arc::new(Barrier::new(WRITERS + 1));
    let done = Arc::new(AtomicBool::new(false));
    let reader = {
        let (cache, sets, done) = (cache.clone(), Arc::clone(&sets), Arc::clone(&done));
        thread::spawn(move || {
            let mut reads = 0_usize;
            while !done.load(Ordering::SeqCst) {
                let seen = cache.load(&["inst"], "orgs");
                assert!(seen.is_empty() || sets.contains(&seen), "torn read");
                reads += 1;
            }
            reads
        })
    };
    let writers: Vec<_> = (0..WRITERS)
        .map(|writer| {
            let (cache, sets, barrier) = (cache.clone(), Arc::clone(&sets), Arc::clone(&barrier));
            thread::spawn(move || {
                barrier.wait();
                cache.store(&["inst"], "orgs", &sets[writer])
            })
        })
        .collect();
    barrier.wait();
    for writer in writers {
        writer.join().unwrap().unwrap();
    }
    done.store(true, Ordering::SeqCst);
    assert!(reader.join().unwrap() > 0);
    let last = cache.load(&["inst"], "orgs");
    assert!(sets.contains(&last), "one writer's full set");
    let dir = root.path().join("completion").join("inst");
    assert_eq!(
        entries(&dir),
        BTreeSet::from(["orgs.json".to_owned(), "orgs.lock".to_owned()]),
        "no temporary files left"
    );
}

fn args(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

#[test]
fn flag_scan_forms() {
    let names = ["--name", "--instance"];
    let scan = |words: &[&str]| scan_flag(&args(words), &names);
    assert_eq!(scan(&["agentic-org", "org", "ls"]), None);
    assert_eq!(
        scan(&["agentic-org", "--name", "prod", "org"]).as_deref(),
        Some("prod")
    );
    assert_eq!(
        scan(&["agentic-org", "--name=prod"]).as_deref(),
        Some("prod")
    );
    assert_eq!(
        scan(&["agentic-org", "--name", "=", "prod"]).as_deref(),
        Some("prod")
    );
    assert_eq!(
        scan(&["agentic-org", "--instance", "a", "--name=b"]).as_deref(),
        Some("b")
    );
    assert_eq!(
        scan(&["agentic-org", "--name=a", "--instance", "c"]).as_deref(),
        Some("c")
    );
}

#[test]
fn flag_scan_edges() {
    let names = ["--name"];
    let scan = |words: &[&str]| scan_flag(&args(words), &names);
    assert_eq!(scan(&["x", "--name"]), None, "no value yet");
    assert_eq!(scan(&["x", "--name", "="]), None);
    assert_eq!(scan(&["x", "--name="]), None);
    assert_eq!(
        scan(&["x", "--name", "--office", "o"]),
        None,
        "a flag is not a value"
    );
    assert_eq!(scan(&["x", "--name", "a", "--name"]).as_deref(), Some("a"));
    assert_eq!(scan(&["x", "--", "--name", "a"]), None, "stops at --");
    assert_eq!(
        scan(&["x", "--names=a", "--named", "b"]),
        None,
        "exact flag names only"
    );
}
