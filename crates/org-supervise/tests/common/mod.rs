//! Helpers shared by the integration tests: short temp dirs under `/private/tmp` (real
//! paths), test environments, launch plans, and checks that nothing is left behind.

#![allow(dead_code)]

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use org_runtime::{EnvName, LaunchPlan, SpecPath, Stdio, StopPlan};
use org_supervise::{
    orphans, AgentSessionEnv, BaseEnv, Env, EnvBuilder, PlanTargets, SessionToken,
};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory `/private/tmp/osv-<pid>-<n>`, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!("/private/tmp/osv-{}-{n}", process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        TempDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// A subdirectory, created.
    pub fn dir(&self, name: &str) -> PathBuf {
        let p = self.0.join(name);
        fs::create_dir_all(&p).unwrap();
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn spec(p: &Path) -> SpecPath {
    SpecPath::try_from(p.to_str().unwrap()).unwrap()
}

pub fn name(s: &str) -> EnvName {
    EnvName::try_from(s).unwrap()
}

/// A mark no other test uses.
pub fn mark(tag: &str) -> String {
    format!(
        "osv-{tag}-{}-{}",
        process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

pub const LOGIN_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

pub fn base_env(tmp: &TempDir) -> BaseEnv {
    BaseEnv {
        office_home: tmp.path().join("office"),
        office_id: "office-1".into(),
        login_path: LOGIN_PATH.into(),
        home: tmp.path().to_path_buf(),
        lang: Some("en_US.UTF-8".into()),
        user: "tester".into(),
        logname: "tester".into(),
        shell: "/bin/zsh".into(),
        tmpdir: tmp.path().join("t"),
    }
}

pub fn agent_session(tmp: &TempDir, ws: &Path) -> AgentSessionEnv {
    AgentSessionEnv {
        agent: "ac-dev-ada".into(),
        token: SessionToken::new("tok-SECRET-123".into()),
        workspace: ws.to_path_buf(),
        agent_home: tmp.path().join("home"),
        git_config_global: tmp.path().join("rt/gitconfig"),
        git_hooks_dir: tmp.path().join("rt/hooks"),
        gh_config_dir: tmp.path().join("rt/gh"),
    }
}

/// An agent session's environment with `mark`.
pub fn session_env(tmp: &TempDir, ws: &Path, mark: &str) -> Env {
    EnvBuilder::new(base_env(tmp))
        .agent_session(agent_session(tmp, ws))
        .mark(mark.to_owned())
        .build()
        .unwrap()
}

/// A probe's environment (no session values, no mark).
pub fn probe_env(tmp: &TempDir) -> Env {
    EnvBuilder::new(base_env(tmp)).build().unwrap()
}

/// A plan running `program args` in `ws`, piped, with no files and an empty stop plan.
pub fn plan(program: &str, args: &[&str], ws: &Path) -> LaunchPlan {
    LaunchPlan {
        program: PathBuf::from(program),
        args: args.iter().map(|s| s.to_string()).collect(),
        cwd: spec(ws),
        env_add: Vec::new(),
        dirs: Vec::new(),
        files: Vec::new(),
        stdio: Stdio::Piped,
        runtime_session_id: None,
        stop: StopPlan { steps: Vec::new() },
        cleanup_after_exit: Vec::new(),
        secret_env_names: Vec::new(),
    }
}

/// Targets with a runtime dir and nothing else.
pub fn targets(rt: &Path) -> PlanTargets {
    PlanTargets {
        runtime_dir: spec(rt),
        git_exclude_file: None,
        cleanup_root: None,
    }
}

/// An ad-hoc-signed copy of `/bin/sleep` in `tmp`: a stand-in for a third-party binary
/// (Claude, a dev server), whose environment, unlike an Apple platform binary's, macOS shows
/// to `KERN_PROCARGS2`, so the mark scan can see it.
pub fn third_party_sleep(tmp: &TempDir) -> PathBuf {
    let dest = tmp.path().join("tsleep");
    fs::copy("/bin/sleep", &dest).unwrap();
    let ok = process::Command::new("/usr/bin/codesign")
        .args(["-f", "-s", "-"])
        .arg(&dest)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "codesign failed");
    dest
}

/// `ps -o <field>= -p <pid>`, trimmed; empty if there's no such process.
pub fn ps(pid: u32, field: &str) -> String {
    let out = process::Command::new("/bin/ps")
        .args(["-o", &format!("{field}="), "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Whether `ps` lists `pid` at all (a zombie included).
pub fn listed(pid: u32) -> bool {
    !ps(pid, "stat").is_empty()
}

/// Waits up to 5 s for `f` to hold.
pub fn eventually(what: &str, mut f: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !f() {
        assert!(Instant::now() < until, "timed out waiting for: {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// Asserts `pid` is gone (no zombie either) and nothing carries `mark`.
pub fn assert_clean(pids: &[u32], mark: Option<&str>) {
    for &pid in pids {
        eventually(&format!("pid {pid} gone"), || !listed(pid));
    }
    if let Some(mark) = mark {
        eventually("no marked process left", || {
            orphans::find_by_mark(mark).unwrap().is_empty()
        });
    }
}

/// Reads `r` on a thread; `collect_until` gathers its output until `needle` shows up.
pub struct Reader {
    rx: mpsc::Receiver<Vec<u8>>,
    pub seen: Vec<u8>,
}

impl Reader {
    pub fn spawn(mut r: impl Read + Send + 'static) -> Reader {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match r.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Reader {
            rx,
            seen: Vec::new(),
        }
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.seen).into_owned()
    }

    /// Reads until `needle` appears in everything seen so far (5 s at most).
    pub fn until(&mut self, needle: &str) -> String {
        let until = Instant::now() + Duration::from_secs(5);
        while !self.text().contains(needle) {
            let left = until.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(chunk) => self.seen.extend(chunk),
                Err(_) => panic!("never saw {needle:?}; got {:?}", self.text()),
            }
        }
        self.text()
    }

    /// Reads until `n` whole lines have arrived (5 s at most); returns them.
    pub fn lines(&mut self, n: usize) -> Vec<String> {
        let until = Instant::now() + Duration::from_secs(5);
        while self.seen.iter().filter(|&&b| b == b'\n').count() < n {
            let left = until.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(chunk) => self.seen.extend(chunk),
                Err(_) => panic!("never got {n} lines; got {:?}", self.text()),
            }
        }
        self.text().lines().take(n).map(str::to_owned).collect()
    }

    /// Reads until the stream ends (5 s at most).
    pub fn read_to_end(&mut self) -> String {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(chunk) => self.seen.extend(chunk),
                Err(mpsc::RecvTimeoutError::Disconnected) => return self.text(),
                Err(mpsc::RecvTimeoutError::Timeout) => panic!("stream never ended"),
            }
        }
    }
}
