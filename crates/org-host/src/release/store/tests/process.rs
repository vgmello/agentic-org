//! Actual child SIGKILL recovery; private generated-key mechanical fixtures only.
use super::*;
use std::{
    io::Write,
    os::unix::process::ExitStatusExt,
    process::{Command, Stdio},
};

#[test]
fn release_crash_child_entry() {
    if std::env::var_os("ORG_RELEASE_CRASH_CHILD").is_none() {
        return;
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        root: PathBuf,
        marker: Vec<u8>,
        boundary: String,
    }
    let input: Input = serde_json::from_reader(std::io::stdin().take(4096)).unwrap();
    assert!(input.root.starts_with("/private/tmp"));
    assert!(input
        .root
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("org-release-crash-"));
    assert_eq!(input.marker.len(), 32);
    assert_eq!(fs::read(input.root.join("marker")).unwrap(), input.marker);
    let bytes = fs::read(input.root.join("registry.json")).unwrap();
    let keys = TrustedKeys::from_registry_bytes(&bytes).unwrap();
    let policy = TrustedPolicy::new(
        org_release::Platform::Linux,
        org_release::Architecture::X86_64,
        1,
        1,
    )
    .unwrap();
    let bundle = DirectoryBundle::with_policy(&input.root.join("input"), &policy, &keys).unwrap();
    let mut store = ReleaseStore::with_policy(&input.root.join("store"), policy, keys).unwrap();
    let kill = || {
        // SAFETY: kill only this explicitly marked disposable child, never an
        // externally supplied PID. SIGKILL creates no core dump.
        unsafe {
            libc::kill(libc::getpid(), libc::SIGKILL);
        }
        panic!("SIGKILL unexpectedly returned");
    };
    let result = store.install_directory_with(&bundle, synthetic_platform, |step| {
        if format!("{step:?}") == input.boundary {
            kill();
        }
        Ok(())
    });
    panic!("crash boundary was not reached: {:?}", result.err());
}

#[test]
fn killed_process_reopens_frozen_publication_without_new_decisions_or_source_reads() {
    for boundary in [
        Step::Payloads,
        Step::Controls,
        Step::Sealed,
        Step::Staged,
        Step::Intent,
        Step::Published,
        Step::Pinned,
    ] {
        let mut f = Fixture::new();
        // Only this fixture uses the child protocol; no environmental pathname
        // can make the helper operate on an unmarked directory.
        let child_root = tempfile::Builder::new()
            .prefix("org-release-crash-")
            .tempdir_in("/private/tmp")
            .unwrap();
        let old = std::mem::replace(&mut f.temp, child_root);
        fs::rename(&f.input, f.temp.path().join("input")).unwrap();
        f.input = f.temp.path().join("input");
        f.store = f.temp.path().join("store");
        drop(old);
        let mut marker = [0_u8; 32];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut marker).unwrap();
        fs::write(f.temp.path().join("marker"), marker).unwrap();
        let registry=serde_json::to_vec(&serde_json::json!({"version":1,"keys":[{"key_id":f.key_id,"public_key_hex":hex(f.key.public_key().as_ref()),"min_sequence":1,"max_sequence":100}]})).unwrap();
        fs::write(f.temp.path().join("registry.json"), registry).unwrap();
        let mut store = f.open();
        let first = install(&mut store, &f.bundle("first", 1));
        drop(store);
        f.bundle("second", 2);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "release::store::tests::process::release_crash_child_entry",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("ORG_RELEASE_CRASH_CHILD", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let bytes=serde_json::to_vec(&serde_json::json!({"root":f.temp.path(),"marker":marker.to_vec(),"boundary":format!("{boundary:?}")})).unwrap();
        child.stdin.take().unwrap().write_all(&bytes).unwrap();
        assert_eq!(
            child.wait().unwrap().signal(),
            Some(libc::SIGKILL),
            "{boundary:?}"
        );
        fs::write(
            f.input.join("ui/index.html"),
            b"source changed after process death",
        )
        .unwrap();
        let mut store = f.open();
        let recovered = store
            .recover_with(synthetic_platform, true)
            .unwrap()
            .unwrap();
        let committed = matches!(boundary, Step::Intent | Step::Published | Step::Pinned);
        assert_eq!(
            recovered.release_id,
            if committed { "second" } else { "first" }
        );
        assert_eq!(records(&store.pins).unwrap(), if committed { 2 } else { 1 });
        assert_eq!(
            fs::read(f.store.join("releases/first/ui/index.html")).unwrap(),
            b"abc"
        );
        assert_eq!(first.sequence, 1);
        if committed {
            assert_eq!(
                fs::read(f.store.join("releases/second/ui/index.html")).unwrap(),
                b"abc"
            );
        }
        println!("actual generated release child SIGKILL {boundary:?}: exact frozen recovery PASS");
    }
}
