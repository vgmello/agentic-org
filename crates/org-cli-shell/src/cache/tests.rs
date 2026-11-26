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
