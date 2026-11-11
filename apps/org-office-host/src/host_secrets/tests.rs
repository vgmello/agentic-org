use super::*;
use std::collections::BTreeMap;
use std::sync::Mutex;

thread_local! { pub(super) static WIPES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[derive(Default)]
struct Memory(Mutex<BTreeMap<Name, Value>>);
impl HostSecrets for Memory {
    fn put(&self, n: &Name, v: &Value) -> Result<(), Error> {
        let mut values = self.0.lock().unwrap();
        if n.as_str().starts_with("tls.") && values.contains_key(n) {
            return Ok(());
        }
        values.insert(n.clone(), Value::new(v.expose().to_vec())?);
        Ok(())
    }
    fn load(&self, n: &Name) -> Result<Option<Value>, Error> {
        self.0
            .lock()
            .unwrap()
            .get(n)
            .map(|v| Value::new(v.expose().to_vec()))
            .transpose()
    }
    fn delete(&self, n: &Name) -> Result<(), Error> {
        self.0.lock().unwrap().remove(n);
        Ok(())
    }
    fn list(&self) -> Result<Vec<Name>, Error> {
        Ok(self.0.lock().unwrap().keys().cloned().collect())
    }
}
fn conformance(store: &dyn HostSecrets) {
    let registry = Name::parse("registry").unwrap();
    let recovery = Name::parse("recovery.Abc123").unwrap();
    assert!(store.load(&registry).unwrap().is_none());
    store.delete(&registry).unwrap();
    assert!(store.list().unwrap().is_empty());
    store
        .put(&registry, &Value::new(vec![1; 32]).unwrap())
        .unwrap();
    assert_eq!(store.load(&registry).unwrap().unwrap().expose(), &[1; 32]);
    store
        .put(&registry, &Value::new(vec![9; MAX_BYTES]).unwrap())
        .unwrap();
    assert_eq!(
        store.load(&registry).unwrap().unwrap().expose(),
        vec![9; MAX_BYTES]
    );
    store
        .put(&recovery, &Value::new(Vec::new()).unwrap())
        .unwrap();
    assert_eq!(
        store.list().unwrap(),
        vec![recovery.clone(), registry.clone()]
    );
    assert!(store.load(&recovery).unwrap().unwrap().expose().is_empty());
    store.delete(&registry).unwrap();
    store.delete(&registry).unwrap();
    assert!(store.load(&registry).unwrap().is_none());
    assert_eq!(store.list().unwrap(), vec![recovery.clone()]);
    store.delete(&recovery).unwrap();
}
#[test]
fn memory_obeys_the_operation_matrix() {
    conformance(&Memory::default());
}
#[test]
fn names_accept_only_the_three_frozen_forms() {
    for valid in [
        "registry".to_owned(),
        format!("tls.{}", "0123456789abcdef".repeat(4)),
        format!("recovery.{}", "Z".repeat(64)),
        "recovery.a".into(),
    ] {
        assert_eq!(Name::parse(&valid).unwrap().as_str(), valid);
    }
    for bad in [
        "",
        "Registry",
        "registry/x",
        "registry\0",
        "tls.",
        "recovery.",
        "recovery...",
        "recovery.a/b",
        "recovery.a_b",
        "recovery.é",
        ".put.X",
    ] {
        assert_eq!(Name::parse(bad), Err(Error::InvalidName), "{bad:?}");
    }
    for bad in [
        format!("tls.{}", "a".repeat(63)),
        format!("tls.{}", "a".repeat(65)),
        format!("tls.{}", "A".repeat(64)),
        format!("tls.{}", "g".repeat(64)),
        format!("recovery.{}", "a".repeat(65)),
    ] {
        assert_eq!(Name::parse(&bad), Err(Error::InvalidName));
    }
}
#[test]
fn values_are_bounded_redacted_and_wiped() {
    assert!(Value::new(Vec::new()).is_ok());
    assert!(Value::new(vec![0; MAX_BYTES]).is_ok());
    assert_eq!(
        Value::new(vec![1; MAX_BYTES + 1]).unwrap_err(),
        Error::TooLarge
    );
    let mut value = Value::new(b"do-not-log-this".to_vec()).unwrap();
    assert_eq!(format!("{value:?}"), "<redacted>");
    assert_eq!(format!("{value}"), "<redacted>");
    value.wipe();
    assert_eq!(value.expose(), &[0; 15]);
    let before = WIPES.with(|n| n.get());
    drop(value);
    assert_eq!(WIPES.with(|n| n.get()), before + 1);
}

#[cfg(debug_assertions)]
mod development {
    use super::*;
    use crate::host_secrets::dev::DevSecrets;
    use std::fs;
    use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
    const B: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";
    struct Fixture {
        _temp: tempfile::TempDir,
        base: PathBuf,
        home: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let t = tempfile::tempdir().unwrap();
            let root = t.path().canonicalize().unwrap();
            let (base, home) = (root.join("keys"), root.join("office"));
            fs::create_dir(&base).unwrap();
            fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
            fs::create_dir(&home).unwrap();
            Self {
                _temp: t,
                base,
                home,
            }
        }
        fn open(&self, id: &str) -> Result<DevSecrets, Error> {
            DevSecrets::open(&self.base, &self.home, id)
        }
        fn dir(&self, id: &str) -> PathBuf {
            self.base.join("host-secrets").join(id)
        }
    }
    fn write(path: &Path, bytes: &[u8]) {
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    fn output(command: &mut std::process::Command) -> std::process::Output {
        use std::process::Stdio;
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!("host-secret test child timed out: {output:?}");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        child.wait_with_output().unwrap()
    }
    #[test]
    fn dev_operations_reopen_and_permissions_follow_the_contract() {
        let f = Fixture::new();
        let store = f.open(A).unwrap();
        conformance(&store);
        let n = Name::parse("registry").unwrap();
        store.put(&n, &Value::new(vec![17; 32]).unwrap()).unwrap();
        for dir in [&f.base, &f.base.join("host-secrets"), &f.dir(A)] {
            assert_eq!(fs::metadata(dir).unwrap().mode() & 0o7777, 0o700);
        }
        assert_eq!(
            fs::metadata(f.dir(A).join("registry")).unwrap().mode() & 0o7777,
            0o600
        );
        assert!(matches!(f.open(A), Err(Error::InUse)));
        drop(store);
        let reopened = f.open(A).unwrap();
        assert_eq!(reopened.load(&n).unwrap().unwrap().expose(), &[17; 32]);
    }
    #[test]
    fn instances_office_keys_and_sweeps_do_not_share_a_namespace() {
        let f = Fixture::new();
        let a = f.open(A).unwrap();
        let b = f.open(B).unwrap();
        let n = Name::parse("registry").unwrap();
        a.put(&n, &Value::new(vec![1; 32]).unwrap()).unwrap();
        b.put(&n, &Value::new(vec![2; 32]).unwrap()).unwrap();
        let office = crate::keys::store_for(Some(&f.base), &f.home, Some(A), || {
            panic!("debug never signs")
        })
        .unwrap();
        office
            .create("OFFICE", &org_proto::DbKey::from_bytes([3; 32]))
            .unwrap();
        assert_eq!(office.list().unwrap(), vec!["OFFICE"]);
        assert_eq!(a.list().unwrap(), vec![n.clone()]);
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000);
        assert!(
            crate::keys::sweep_orphans(&*office, &f.home, |ids| Ok(ids.to_vec()), now).is_empty()
        );
        assert_eq!(
            crate::keys::sweep_orphans(
                &*office,
                &f.home,
                |ids| Ok(ids.to_vec()),
                now + std::time::Duration::from_secs(7 * 86400)
            ),
            vec!["OFFICE"]
        );
        assert_eq!(a.load(&n).unwrap().unwrap().expose(), &[1; 32]);
        assert!(office.load("OFFICE").unwrap().is_none());
        a.delete(&n).unwrap();
        assert_eq!(b.load(&n).unwrap().unwrap().expose(), &[2; 32]);
    }
    #[test]
    fn invalid_roots_and_directory_symlinks_have_no_secret_effects() {
        let f = Fixture::new();
        for id in [
            "",
            "../escape",
            "not-an-instance",
            "8ZZZZZZZZZZZZZZZZZZZZZZZZZ",
        ] {
            assert!(f.open(id).is_err());
        }
        assert!(DevSecrets::open(Path::new("relative"), &f.home, A).is_err());
        let parent_components = f.base.join("..").join("keys");
        assert!(matches!(
            DevSecrets::open(&parent_components, &f.home, A),
            Err(Error::Unsafe)
        ));
        fs::set_permissions(&f.home, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(
            DevSecrets::open(&f.home, &f.home, A),
            Err(Error::Unsafe)
        ));
        let nested = f.home.join("nested");
        fs::create_dir(&nested).unwrap();
        fs::set_permissions(&nested, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(
            DevSecrets::open(&nested, &f.home, A),
            Err(Error::Unsafe)
        ));
        assert!(!nested.join("host-secrets").exists());
        assert!(!f.home.join("host-secrets").exists());
        let alias = f.base.with_file_name("alias");
        symlink(&f.base, &alias).unwrap();
        assert!(DevSecrets::open(&alias, &f.home, A).is_err());
        for spelling in [
            PathBuf::from(format!("{}/", alias.display())),
            alias.join("."),
        ] {
            assert!(
                DevSecrets::open(&spelling, &f.home, A).is_err(),
                "{spelling:?}"
            );
        }
        assert!(!f.base.join("host-secrets").exists());
        let external = f.base.with_file_name("external");
        fs::create_dir(&external).unwrap();
        symlink(&external, f.base.join("host-secrets")).unwrap();
        assert!(f.open(A).is_err());
        assert!(!external.join(A).exists());
    }
    #[test]
    fn symlinks_nonregular_items_and_oversize_files_fail_closed() {
        let f = Fixture::new();
        let s = f.open(A).unwrap();
        let n = Name::parse("registry").unwrap();
        let path = f.dir(A).join(n.as_str());
        let external = f.base.with_file_name("untouched");
        write(&external, b"keep");
        symlink(&external, &path).unwrap();
        assert!(s.load(&n).is_err());
        assert!(s.put(&n, &Value::new(vec![1]).unwrap()).is_err());
        assert!(s.delete(&n).is_err());
        assert!(s.list().is_err());
        assert_eq!(fs::read(&external).unwrap(), b"keep");
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(s.load(&n).is_err());
        assert!(s.put(&n, &Value::new(vec![1]).unwrap()).is_err());
        assert!(s.delete(&n).is_err());
        fs::remove_dir(&path).unwrap();
        write(&path, &vec![7; MAX_BYTES + 1]);
        assert_eq!(s.load(&n).unwrap_err(), Error::Corrupt);
        write(&path, b"ok");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(s.load(&n).unwrap_err(), Error::Unsafe);
    }
    #[test]
    fn crash_temporary_cleanup_keeps_the_published_value() {
        let f = Fixture::new();
        let s = f.open(A).unwrap();
        let n = Name::parse("registry").unwrap();
        s.put(&n, &Value::new(b"old".to_vec()).unwrap()).unwrap();
        drop(s);
        let pending = f.dir(A).join(format!(".put.{B}"));
        write(&pending, b"unpublished");
        write(&f.dir(A).join("unrelated"), b"preserve");
        let s = f.open(A).unwrap();
        assert!(!pending.exists());
        assert_eq!(s.load(&n).unwrap().unwrap().expose(), b"old");
        assert_eq!(s.list().unwrap(), vec![n.clone()]);
        s.put(&n, &Value::new(b"new".to_vec()).unwrap()).unwrap();
        assert!(!pending.exists());
        drop(s);
        assert_eq!(
            f.open(A).unwrap().load(&n).unwrap().unwrap().expose(),
            b"new"
        );
        assert_eq!(fs::read(f.dir(A).join("unrelated")).unwrap(), b"preserve");
    }
    #[test]
    fn drop_unlocks_even_while_a_forked_child_holds_the_descriptor() {
        use std::os::fd::AsRawFd;
        let f = Fixture::new();
        let store = f.open(A).unwrap();
        // Standard-library sockets retain CLOEXEC; a byte releases the child even if
        // another fork briefly inherits a socket, so the test never depends on EOF.
        let (mut release, wait) = std::os::unix::net::UnixStream::pair().unwrap();
        let (release_fd, wait_fd) = (release.as_raw_fd(), wait.as_raw_fd());
        // SAFETY: the child only uses async-signal-safe close/read/_exit before exiting.
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            // SAFETY: close the parent's socket, then retain the inherited custody fd
            // until the parent has attempted its post-drop reopen and sends one byte.
            unsafe {
                libc::close(release_fd);
                let mut byte = 0u8;
                let read = libc::read(wait_fd, (&mut byte as *mut u8).cast(), 1);
                libc::_exit(if read == 1 && byte == 1 { 0 } else { 42 });
            }
        }
        assert!(pid > 0, "fork failed");
        drop(wait);
        drop(store);
        let reopened = f.open(A);
        let released = std::io::Write::write_all(&mut release, &[1]);
        drop(release);
        // Reap before assertions so the regression failure never leaves a child behind.
        let mut status = 0;
        // SAFETY: wait only for the child this test created.
        let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
        released.unwrap();
        assert_eq!(waited, pid);
        assert_eq!(status, 0);
        assert!(
            reopened.is_ok(),
            "parent drop must release custody lock even with an inherited fd: {:?}",
            reopened.err()
        );
    }

    #[test]
    fn replacements_are_atomic_to_readers() {
        let f = Fixture::new();
        let s = std::sync::Arc::new(f.open(A).unwrap());
        let n = Name::parse("registry").unwrap();
        s.put(&n, &Value::new(vec![1; MAX_BYTES]).unwrap()).unwrap();
        std::thread::scope(|scope| {
            let writer = std::sync::Arc::clone(&s);
            let key = n.clone();
            scope.spawn(move || {
                for i in 0..40 {
                    writer
                        .put(&key, &Value::new(vec![1 + i % 2; MAX_BYTES]).unwrap())
                        .unwrap();
                }
            });
            for _ in 0..80 {
                let value = s.load(&n).unwrap().unwrap();
                assert_eq!(value.expose().len(), MAX_BYTES);
                assert!(value.expose().iter().all(|b| *b == value.expose()[0]));
            }
        });
    }

    #[test]
    fn lock_is_exclusive_across_processes_and_released_on_drop() {
        const MARKER: &str = "SAGE_HOST_SECRET_LOCK_CHILD";
        if let Some(root) = std::env::var_os(MARKER) {
            let root = PathBuf::from(root);
            assert!(matches!(
                DevSecrets::open(Path::new("keys"), &root.join("office"), B),
                Err(Error::Unsafe)
            ));
            assert!(matches!(
                DevSecrets::open(&root.join("keys"), &root.join("office"), A),
                Err(Error::InUse)
            ));
            return;
        }
        let f = Fixture::new();
        let store = f.open(A).unwrap();
        let temporary = f.dir(A).join(format!(".put.{B}"));
        write(&temporary, b"active owner");
        let output = output(std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "host_secrets::tests::development::lock_is_exclusive_across_processes_and_released_on_drop", "--nocapture"])
            .current_dir(f.base.parent().unwrap())
            .env(MARKER, f.base.parent().unwrap()));
        assert!(output.status.success(), "{output:?}");
        assert!(
            temporary.exists(),
            "contender must not clean the active owner's temp"
        );
        assert!(
            !f.dir(B).exists(),
            "relative root must be refused before namespace creation"
        );
        assert_eq!(
            fs::metadata(f.dir(A).join("lock")).unwrap().mode() & 0o7777,
            0o600
        );
        drop(store);
        assert!(f.open(A).is_ok());
        assert!(!temporary.exists());
    }
    #[test]
    fn unsafe_directories_and_lock_files_are_refused() {
        for (level, mode) in [("base", 0o750), ("namespace", 0o777), ("instance", 0o755)] {
            let f = Fixture::new();
            drop(f.open(A).unwrap());
            let path = match level {
                "base" => f.base.clone(),
                "namespace" => f.base.join("host-secrets"),
                _ => f.dir(A),
            };
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            assert!(matches!(f.open(A), Err(Error::Unsafe)), "{level}");
        }
        for kind in ["symlink", "directory", "permissions"] {
            let f = Fixture::new();
            drop(f.open(A).unwrap());
            let lock = f.dir(A).join("lock");
            fs::remove_file(&lock).unwrap();
            match kind {
                "symlink" => symlink(&f.base, &lock).unwrap(),
                "directory" => fs::create_dir(&lock).unwrap(),
                _ => {
                    write(&lock, b"");
                    fs::set_permissions(&lock, fs::Permissions::from_mode(0o644)).unwrap();
                }
            }
            assert!(f.open(A).is_err(), "{kind}");
        }
        let f = Fixture::new();
        drop(f.open(A).unwrap());
        fs::remove_dir_all(f.dir(A)).unwrap();
        symlink(&f.home, f.dir(A)).unwrap();
        assert!(f.open(A).is_err());
        assert!(!f.home.join("lock").exists());
        // Root's extra topology probe can exercise uid independently of mode permissions.
        if crate::home::euid() == 0 {
            for level in ["base", "namespace", "instance", "lock", "item"] {
                let f = Fixture::new();
                let store = f.open(A).unwrap();
                let n = Name::parse("registry").unwrap();
                store.put(&n, &Value::new(vec![1]).unwrap()).unwrap();
                drop(store);
                let path = match level {
                    "base" => f.base.clone(),
                    "namespace" => f.base.join("host-secrets"),
                    "instance" => f.dir(A),
                    "lock" => f.dir(A).join("lock"),
                    _ => f.dir(A).join("registry"),
                };
                std::os::unix::fs::chown(&path, Some(1), None).unwrap();
                if level == "item" {
                    assert_eq!(f.open(A).unwrap().load(&n).unwrap_err(), Error::Unsafe);
                } else {
                    assert!(matches!(f.open(A), Err(Error::Unsafe)), "{level}");
                }
            }
        }
    }
    #[test]
    fn oversize_put_and_fifo_leave_published_bytes_untouched() {
        let f = Fixture::new();
        let s = f.open(A).unwrap();
        let n = Name::parse("registry").unwrap();
        s.put(&n, &Value::new(b"old".to_vec()).unwrap()).unwrap();
        assert_eq!(
            s.put(&n, &Value(vec![1; MAX_BYTES + 1])),
            Err(Error::TooLarge)
        );
        assert_eq!(s.load(&n).unwrap().unwrap().expose(), b"old");
        let path = f.dir(A).join("registry");
        fs::remove_file(&path).unwrap();
        use std::os::unix::ffi::OsStrExt;
        let raw = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: live, NUL-terminated temporary fixture path.
        assert_eq!(unsafe { libc::mkfifo(raw.as_ptr(), 0o600) }, 0);
        assert_eq!(s.load(&n).unwrap_err(), Error::Unsafe);
        assert!(s.put(&n, &Value::new(vec![]).unwrap()).is_err());
        assert!(s.delete(&n).is_err());
    }
    #[test]
    fn stale_temp_symlinks_never_touch_their_target() {
        let f = Fixture::new();
        drop(f.open(A).unwrap());
        let target = f.base.with_file_name("victim");
        write(&target, b"keep");
        symlink(&target, f.dir(A).join(format!(".put.{B}"))).unwrap();
        assert!(f.open(A).is_err());
        assert_eq!(fs::read(target).unwrap(), b"keep");
    }
    #[test]
    fn unknown_names_are_logged_without_echoing_untrusted_bytes() {
        const MARKER: &str = "SAGE_HOST_SECRET_LIST_CHILD";
        if std::env::var_os(MARKER).is_some() {
            #[cfg(target_os = "linux")]
            use std::os::unix::ffi::OsStringExt;
            let f = Fixture::new();
            let s = f.open(A).unwrap();
            write(
                &f.dir(A).join("DO_NOT_ECHO_THIS_FILENAME"),
                b"DO_NOT_ECHO_THIS_VALUE",
            );
            #[cfg(target_os = "linux")]
            write(
                &f.dir(A).join(std::ffi::OsString::from_vec(vec![0xff])),
                b"nonutf8",
            );
            write(&f.dir(A).join(format!(".put.{B}")), b"temporary");
            assert!(s.list().unwrap().is_empty());
            return;
        }
        let output = output(std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "host_secrets::tests::development::unknown_names_are_logged_without_echoing_untrusted_bytes", "--nocapture"])
            .env(MARKER, "1"));
        assert!(output.status.success(), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            stderr
                .matches("ignored an invalid host-secret item name")
                .count(),
            if cfg!(target_os = "linux") { 3 } else { 2 }
        );
        assert!(!stderr.contains("DO_NOT_ECHO_THIS"));
    }
    #[test]
    fn changed_directory_is_refused_before_any_operation() {
        let f = Fixture::new();
        let s = f.open(A).unwrap();
        let n = Name::parse("registry").unwrap();
        s.put(&n, &Value::new(b"old".to_vec()).unwrap()).unwrap();
        let moved = f.dir(A).with_file_name("moved");
        fs::rename(f.dir(A), &moved).unwrap();
        fs::create_dir(f.dir(A)).unwrap();
        fs::set_permissions(f.dir(A), fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(s.load(&n).unwrap_err(), Error::Unsafe);
        assert_eq!(s.delete(&n), Err(Error::Unsafe));
        assert_eq!(s.put(&n, &Value::new(vec![]).unwrap()), Err(Error::Unsafe));
        assert_eq!(s.list(), Err(Error::Unsafe));
        assert_eq!(fs::read(moved.join("registry")).unwrap(), b"old");
    }

    #[test]
    fn case_aliases_fail_closed_in_both_orders() {
        for (first, alias) in [("recovery.A", "recovery.a"), ("recovery.a", "recovery.A")] {
            let f = Fixture::new();
            let store = f.open(A).unwrap();
            let first = Name::parse(first).unwrap();
            let alias = Name::parse(alias).unwrap();
            store
                .put(&first, &Value::new(b"keep".to_vec()).unwrap())
                .unwrap();
            assert_eq!(
                store.put(&alias, &Value::new(b"wrong".to_vec()).unwrap()),
                Err(Error::CaseAlias)
            );
            assert_eq!(store.load(&alias).unwrap_err(), Error::CaseAlias);
            assert_eq!(store.delete(&alias), Err(Error::CaseAlias));
            assert_eq!(store.load(&first).unwrap().unwrap().expose(), b"keep");
            assert_eq!(store.list().unwrap(), vec![first]);
            #[cfg(target_os = "linux")]
            {
                write(&f.dir(A).join(alias.as_str()), b"external alias");
                assert_eq!(store.list(), Err(Error::CaseAlias));
            }
        }
    }

    #[test]
    fn concurrent_case_alias_publications_have_one_winner() {
        for _ in 0..8 {
            let f = Fixture::new();
            let store = std::sync::Arc::new(f.open(A).unwrap());
            let barrier = std::sync::Barrier::new(2);
            let results = std::thread::scope(|scope| {
                let a = scope.spawn(|| {
                    barrier.wait();
                    store.put(
                        &Name::parse("recovery.A").unwrap(),
                        &Value::new(vec![1]).unwrap(),
                    )
                });
                let b = scope.spawn(|| {
                    barrier.wait();
                    store.put(
                        &Name::parse("recovery.a").unwrap(),
                        &Value::new(vec![2]).unwrap(),
                    )
                });
                (a.join().unwrap(), b.join().unwrap())
            });
            assert!(matches!(
                results,
                (Ok(()), Err(Error::CaseAlias)) | (Err(Error::CaseAlias), Ok(()))
            ));
            let name = store.list().unwrap().pop().unwrap();
            let expected = if name.as_str() == "recovery.A" { 1 } else { 2 };
            assert_eq!(store.load(&name).unwrap().unwrap().expose(), &[expected]);
            assert_eq!(store.list().unwrap(), vec![name]);
        }
    }

    #[test]
    fn office_home_case_aliases_are_refused_after_canonicalization() {
        let f = Fixture::new();
        fs::set_permissions(&f.home, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(
            DevSecrets::open(&f.home, &f.home, A),
            Err(Error::Unsafe)
        ));
        let nested = f.home.join("keys");
        fs::create_dir(&nested).unwrap();
        fs::set_permissions(&nested, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(
            DevSecrets::open(&nested, &f.home, A),
            Err(Error::Unsafe)
        ));
        let alias = f.home.with_file_name("OFFICE");
        if alias.exists() {
            assert!(matches!(
                DevSecrets::open(&alias, &f.home, A),
                Err(Error::Unsafe)
            ));
            assert!(matches!(
                DevSecrets::open(&alias.join("keys"), &f.home, A),
                Err(Error::Unsafe)
            ));
        }
        assert!(!f.home.join("host-secrets").exists());
        assert!(!nested.join("host-secrets").exists());
    }

    #[test]
    fn restrictive_umask_cannot_publish_an_unreadable_replacement() {
        const MARKER: &str = "SAGE_HOST_SECRET_UMASK_CHILD";
        if std::env::var_os(MARKER).is_some() {
            let f = Fixture::new();
            let store = f.open(A).unwrap();
            let name = Name::parse("registry").unwrap();
            store
                .put(&name, &Value::new(b"old".to_vec()).unwrap())
                .unwrap();
            // SAFETY: this exact-test child is isolated from the parent's parallel tests.
            let prior = unsafe { libc::umask(0o400) };
            let result = store.put(&name, &Value::new(b"new".to_vec()).unwrap());
            // SAFETY: restore the process mask before assertions and fixture cleanup.
            unsafe {
                libc::umask(prior);
            }
            result.unwrap();
            assert_eq!(
                fs::metadata(f.dir(A).join("registry")).unwrap().mode() & 0o7777,
                0o600
            );
            assert_eq!(store.load(&name).unwrap().unwrap().expose(), b"new");
            return;
        }
        let output = output(std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "host_secrets::tests::development::restrictive_umask_cannot_publish_an_unreadable_replacement", "--nocapture"])
            .env(MARKER, "1"));
        assert!(output.status.success(), "{output:?}");
    }

    #[test]
    fn tls_native_publish_collision_and_error_preserve_state_and_clean_temps() {
        for mode in ["existing", "symlink", "missing-temporary"] {
            let f = Fixture::new();
            let store = f.open(A).unwrap();
            let raw = format!("tls.{}", "a".repeat(64));
            let n = Name::parse(&raw).unwrap();
            let other = f.base.join("untouched");
            write(&other, b"outside");
            let target = other.clone();
            super::PUBLISH_HOOK.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move |dir| {
                    let leaf = dir.join(&raw);
                    match mode {
                        "existing" => write(&leaf, b"raced first"),
                        "symlink" => symlink(&target, &leaf).unwrap(),
                        _ => {
                            for entry in fs::read_dir(dir).unwrap() {
                                let entry = entry.unwrap();
                                if entry.file_name().to_str().unwrap().starts_with(".put.") {
                                    fs::remove_file(entry.path()).unwrap();
                                }
                            }
                        }
                    }
                }))
            });
            let result = store.put(&n, &Value::new(b"new".to_vec()).unwrap());
            match mode {
                "existing" => {
                    result.unwrap();
                    assert_eq!(store.load(&n).unwrap().unwrap().expose(), b"raced first");
                }
                "symlink" => {
                    assert!(result.is_err());
                    assert!(fs::symlink_metadata(f.dir(A).join(n.as_str()))
                        .unwrap()
                        .file_type()
                        .is_symlink());
                }
                _ => {
                    assert_eq!(result, Err(Error::Io(std::io::ErrorKind::NotFound)));
                    assert!(store.load(&n).unwrap().is_none());
                }
            }
            assert_eq!(fs::read(&other).unwrap(), b"outside");
            assert!(fs::read_dir(f.dir(A)).unwrap().all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".put.")));
        }
    }

    #[test]
    fn tls_replacement_preserves_first_bytes_and_reopens() {
        let f = Fixture::new();
        let store = f.open(A).unwrap();
        let name = Name::parse(&format!("tls.{}", "a".repeat(64))).unwrap();
        store
            .put(&name, &Value::new(b"first".to_vec()).unwrap())
            .unwrap();
        store
            .put(&name, &Value::new(b"second".to_vec()).unwrap())
            .unwrap();
        assert_eq!(store.load(&name).unwrap().unwrap().expose(), b"first");
        drop(store);
        let store = f.open(A).unwrap();
        store
            .put(&name, &Value::new(b"third".to_vec()).unwrap())
            .unwrap();
        assert_eq!(store.load(&name).unwrap().unwrap().expose(), b"first");
        assert_eq!(store.list().unwrap(), vec![name]);
    }
}

const HOST_INSTANCE: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
const HOST_GROUP: &str = "TEAMID1234.org.agentic.host";
fn signing() -> crate::code_signing::Signing {
    crate::code_signing::Signing {
        flags: Some(crate::code_signing::HARDENED_RUNTIME),
        entitlements: Some(vec![]),
        keychain_access_groups: Some(vec![HOST_GROUP.to_owned()]),
    }
}
fn selection_error(
    result: Result<std::sync::Arc<dyn HostSecrets>, SelectionError>,
) -> SelectionError {
    match result {
        Err(error) => error,
        Ok(_) => panic!("selection unexpectedly accepted"),
    }
}
#[test]
fn release_selection_validates_instance_then_group_before_signing() {
    for instance in [
        "",
        "../bad",
        "01aaaaaaaaaaaaaaaaaaaaaaaa",
        "8ZZZZZZZZZZZZZZZZZZZZZZZZZ",
    ] {
        assert_eq!(
            selection_error(release_store(None, instance, || panic!(
                "invalid identity must not inspect signing"
            ))),
            SelectionError::InvalidInstance
        );
        assert_eq!(
            selection_error(store_for(
                None,
                std::path::Path::new("/"),
                instance,
                || panic!("invalid identity must not inspect signing")
            )),
            SelectionError::InvalidInstance
        );
    }
    for group in [None, Some(""), Some(" "), Some("\t\n")] {
        assert_eq!(
            selection_error(release_store(group, HOST_INSTANCE, || panic!(
                "missing group must not inspect signing"
            ))),
            SelectionError::NoGroup
        );
    }
    assert_eq!(
        selection_error(release_store(Some(HOST_GROUP), HOST_INSTANCE, || None)),
        SelectionError::NotHardened
    );
}
#[test]
fn release_selection_exact_entitlement_precedes_hardening() {
    for groups in [
        None,
        Some(vec![]),
        Some(vec![HOST_GROUP.to_lowercase()]),
        Some(vec![format!("{HOST_GROUP}.suffix")]),
    ] {
        let mut signature = signing();
        signature.keychain_access_groups = groups;
        signature.flags = None;
        assert_eq!(
            selection_error(release_store(Some(HOST_GROUP), HOST_INSTANCE, || Some(
                signature
            ))),
            SelectionError::NotEntitled
        );
    }
    assert_eq!(
        selection_error(release_store(
            Some(&format!(" {HOST_GROUP} ")),
            HOST_INSTANCE,
            || Some(signing())
        )),
        SelectionError::NotEntitled
    );
    for flags in [None, Some(0), Some(!crate::code_signing::HARDENED_RUNTIME)] {
        let mut signature = signing();
        signature.flags = flags;
        assert_eq!(
            selection_error(release_store(Some(HOST_GROUP), HOST_INSTANCE, || Some(
                signature
            ))),
            SelectionError::NotHardened
        );
    }
    for entitlement in crate::code_signing::REJECTED_ENTITLEMENTS {
        for rejected_value in [None, Some(true)] {
            let mut signature = signing();
            signature.entitlements = Some(vec![(entitlement.to_owned(), rejected_value)]);
            assert_eq!(
                selection_error(release_store(Some(HOST_GROUP), HOST_INSTANCE, || Some(
                    signature
                ))),
                SelectionError::NotHardened
            );
        }
    }
}
#[test]
fn release_selection_constructs_only_an_eligible_platform_backend_without_io() {
    for entitlements in [
        None,
        Some(vec![]),
        Some(
            crate::code_signing::REJECTED_ENTITLEMENTS
                .iter()
                .map(|key| ((*key).to_owned(), Some(false)))
                .collect(),
        ),
    ] {
        let mut signature = signing();
        signature.entitlements = entitlements;
        signature.flags = Some(crate::code_signing::HARDENED_RUNTIME | 1);
        signature.keychain_access_groups = Some(vec!["unrelated".into(), HOST_GROUP.into()]);
        let calls = std::cell::Cell::new(0);
        let result = release_store(Some(HOST_GROUP), HOST_INSTANCE, || {
            calls.set(calls.get() + 1);
            Some(signature)
        });
        assert_eq!(calls.get(), 1);
        #[cfg(target_os = "macos")]
        assert!(result.is_ok()); // Construction does not call SecItem* or create any item.
        #[cfg(not(target_os = "macos"))]
        assert_eq!(selection_error(result), SelectionError::Unsupported);
    }
}
#[cfg(target_os = "macos")]
#[test]
fn actual_unsigned_binary_fails_release_selection_before_keychain_io() {
    let signature = crate::code_signing::own_signing();
    assert!(
        !signature.as_ref().is_some_and(|s| s
            .keychain_access_groups
            .as_ref()
            .is_some_and(|groups| groups.iter().any(|group| group == HOST_GROUP))
            && crate::code_signing::hardened_and_clean(s)),
        "this probe is intended for an unsigned test binary"
    );
    let error = selection_error(release_store(Some(HOST_GROUP), HOST_INSTANCE, || signature));
    assert!(matches!(
        error,
        SelectionError::NotHardened | SelectionError::NotEntitled
    ));
}
#[cfg(debug_assertions)]
#[test]
fn debug_factory_requires_safe_existing_base_and_never_probes_signing() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let (base, home) = (root.join("base"), root.join("home"));
    for dir in [&base, &home] {
        std::fs::create_dir(dir).unwrap();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let store = store_for(Some(&base), &home, HOST_INSTANCE, || {
        panic!("debug must never sign")
    })
    .unwrap();
    let n = Name::parse("registry").unwrap();
    store
        .put(&n, &Value::new(b"dev only".to_vec()).unwrap())
        .unwrap();
    assert_eq!(store.load(&n).unwrap().unwrap().expose(), b"dev only");
    assert_eq!(
        selection_error(store_for(None, &home, HOST_INSTANCE, || panic!(
            "debug must never sign"
        ))),
        SelectionError::NoDevBase
    );
    assert_eq!(
        selection_error(store_for(Some(&home), &home, HOST_INSTANCE, || panic!(
            "debug must never sign"
        ))),
        SelectionError::Storage(Error::Unsafe)
    );
    assert_eq!(
        selection_error(store_for(
            Some(&root.join("missing")),
            &home,
            HOST_INSTANCE,
            || panic!("debug must never sign")
        )),
        SelectionError::Storage(Error::Io(std::io::ErrorKind::NotFound))
    );
}
#[cfg(not(debug_assertions))]
#[test]
fn release_factory_ignores_dev_base_and_uses_only_compiled_group() {
    let group_present =
        option_env!("ORG_HOST_KEYCHAIN_GROUP").is_some_and(|g| !g.trim().is_empty());
    let calls = std::cell::Cell::new(0);
    let error = selection_error(store_for(
        Some(std::path::Path::new("/never-created-host-secret-test")),
        std::path::Path::new("/"),
        HOST_INSTANCE,
        || {
            calls.set(calls.get() + 1);
            None
        },
    ));
    assert_eq!(calls.get(), usize::from(group_present));
    assert_eq!(
        error,
        if group_present {
            SelectionError::NotHardened
        } else {
            SelectionError::NoGroup
        }
    );
}

#[cfg(debug_assertions)]
type PublishHook = Box<dyn FnOnce(&std::path::Path)>;
#[cfg(debug_assertions)]
thread_local! { static PUBLISH_HOOK: std::cell::RefCell<Option<PublishHook>> = const { std::cell::RefCell::new(None) }; }
#[cfg(debug_assertions)]
pub(super) fn before_publish(path: &std::path::Path) {
    let hook = PUBLISH_HOOK.with(|hook| hook.borrow_mut().take());
    if let Some(hook) = hook {
        hook(path);
    }
}
