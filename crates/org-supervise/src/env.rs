//! The child's environment: exactly what decision 6 of `docs/design/agent-sessions.md` lists,
//! built from the office's values, plus the plan's additions. Nothing is inherited from this
//! process: a spawn passes [`Env`] as the child's whole `envp`.
//!
//! Why a builder: the allowlist is a security boundary (no `SSH_AUTH_SOCK`, `GH_TOKEN`, API
//! keys or the user's git config may reach an agent), so it's built in one place, from named
//! inputs, rather than filtered from whatever this process happens to have.

use std::collections::BTreeMap;
use std::ffi::CString;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use org_runtime::EnvName;

use crate::error::{Error, Result};

/// `TERM` for a take-over terminal when the caller doesn't name one.
pub const DEFAULT_TERM: &str = "xterm-256color";

/// The session token, `ORG_SESSION_TOKEN`: a secret. Its `Debug` never shows the value, and
/// [`Env`]'s `Debug` shows no values at all.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionToken(String);

impl SessionToken {
    /// Wraps a token the office minted.
    pub fn new(token: String) -> Self {
        SessionToken(token)
    }
}

impl fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionToken(<redacted>)")
    }
}

/// The office-wide values every child gets (a probe gets only these).
#[derive(Debug, Clone)]
pub struct BaseEnv {
    /// `ORG_OFFICE_HOME`. Its `bin/` folder (the office's own `org-agent`) comes first in
    /// `PATH`.
    pub office_home: PathBuf,
    /// `ORG_OFFICE`: the office id.
    pub office_id: String,
    /// The `PATH` captured once from the user's login shell. Filtered: see
    /// [`EnvBuilder::build`].
    pub login_path: String,
    /// `HOME`.
    pub home: PathBuf,
    /// `LANG`, if the user has one.
    pub lang: Option<String>,
    /// `USER`.
    pub user: String,
    /// `LOGNAME`.
    pub logname: String,
    /// `SHELL`.
    pub shell: PathBuf,
    /// `TMPDIR`: the session's own directory (or a probe's).
    pub tmpdir: PathBuf,
}

/// The values for an agent's session (not for a probe).
#[derive(Debug, Clone)]
pub struct AgentSessionEnv {
    /// `ORG_AGENT`.
    pub agent: String,
    /// `ORG_SESSION_TOKEN`, fresh for every run.
    pub token: SessionToken,
    /// The session's workspace: `PATH` entries inside it are dropped.
    pub workspace: PathBuf,
    /// The agent's home: `PATH` entries inside it are dropped.
    pub agent_home: PathBuf,
    /// The commit identity's `user.name` (from `projects-git-github.md`).
    pub git_user_name: String,
    /// The commit identity's `user.email`.
    pub git_user_email: String,
    /// For a Codex session: the folder of the real git (`xcrun --find git`, resolved at
    /// preflight), put in `PATH` before `/usr/bin` (decision 6). `None` for other runtimes.
    pub git_bin_dir: Option<PathBuf>,
}

/// Builds an [`Env`] from the office's values.
#[derive(Debug, Clone)]
pub struct EnvBuilder {
    base: BaseEnv,
    session: Option<AgentSessionEnv>,
    mark: Option<String>,
    term: Option<String>,
}

/// Name prefixes the office owns. A plan's `env_add` entry under one of them (or naming a
/// variable the office sets) is dropped: the plan's additions never override the office, and
/// can't add git, `gh`, office or dyld settings of their own (`GIT_DIR`, `GIT_CONFIG_PARAMETERS`
/// and `DYLD_INSERT_LIBRARIES` would undo the isolation).
const RESERVED_PREFIXES: &[&str] = &["ORG_", "GIT_", "GH_", "DYLD_"];

/// Names the office always owns, set or not (`TERM` is set only on a terminal).
const RESERVED_NAMES: &[&str] = &[
    "PATH", "HOME", "LANG", "USER", "LOGNAME", "SHELL", "TERM", "TMPDIR",
];

impl EnvBuilder {
    /// Starts from the office-wide values.
    pub fn new(base: BaseEnv) -> Self {
        EnvBuilder {
            base,
            session: None,
            mark: None,
            term: None,
        }
    }

    /// Adds an agent session's values: `ORG_AGENT`, `ORG_SESSION_TOKEN`, the `GIT_*` set
    /// (`GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null`, and through
    /// `GIT_CONFIG_COUNT`/`KEY_n`/`VALUE_n` the commit identity, an empty `credential.helper`,
    /// `core.hooksPath=/dev/null`, `core.fsmonitor=false`, `gc.auto=0`,
    /// `maintenance.auto=false`) and `GH_CONFIG_DIR=<TMPDIR>/gh/` (the office makes that empty
    /// folder). [`crate::spawn_session`] requires them.
    pub fn agent_session(mut self, session: AgentSessionEnv) -> Self {
        self.session = Some(session);
        self
    }

    /// Sets `ORG_SESSION_MARK`, which orphan cleanup finds the session's processes by (not a
    /// secret). A probe may carry one too, so its leftovers can be found the same way.
    pub fn mark(mut self, mark: String) -> Self {
        self.mark = Some(mark);
        self
    }

    /// Sets the `TERM` a take-over terminal gets (default [`DEFAULT_TERM`]). It's never
    /// passed to a piped child.
    pub fn terminal(mut self, term: String) -> Self {
        self.term = Some(term);
        self
    }

    /// Builds the environment.
    ///
    /// `PATH` is `<office home>/bin`, then the login shell's `PATH` with these entries
    /// dropped: empty or relative ones, ones with a `.`, `..` or empty component, ones inside
    /// the workspace or the agent's home (compared as written and, where the entry exists,
    /// by its real path), the office's bin folder itself, and duplicates.
    ///
    /// Fails if a value holds a NUL byte, a path isn't absolute, the office home holds a `:`
    /// (it would split `PATH`), or the mark is empty or holds `=` or a non-printable byte.
    pub fn build(self) -> Result<Env> {
        let b = &self.base;
        for (what, path) in [
            ("office home", &b.office_home),
            ("HOME", &b.home),
            ("SHELL", &b.shell),
            ("TMPDIR", &b.tmpdir),
        ] {
            require_absolute(what, path)?;
        }
        let office_home = path_str("office home", &b.office_home)?;
        if office_home.contains(':') {
            return Err(Error::invalid("the office home holds a `:`"));
        }

        let mut excludes = Vec::new();
        if let Some(s) = &self.session {
            for (what, path) in [("workspace", &s.workspace), ("agent home", &s.agent_home)] {
                require_absolute(what, path)?;
            }
            if let Some(dir) = &s.git_bin_dir {
                require_absolute("the git folder", dir)?;
                if path_str("the git folder", dir)?.contains(':') {
                    return Err(Error::invalid("the git folder holds a `:`"));
                }
            }
            excludes.push(s.workspace.clone());
            excludes.push(s.agent_home.clone());
        }

        let mut vars = BTreeMap::new();
        let mut set = |name: &str, value: String| vars.insert(name.to_owned(), value);
        set(
            "PATH",
            compose_path(
                &b.office_home,
                &b.login_path,
                &excludes,
                self.session.as_ref().and_then(|s| s.git_bin_dir.as_deref()),
            ),
        );
        set("HOME", path_str("HOME", &b.home)?.to_owned());
        if let Some(lang) = &b.lang {
            set("LANG", lang.clone());
        }
        set("USER", b.user.clone());
        set("LOGNAME", b.logname.clone());
        set("SHELL", path_str("SHELL", &b.shell)?.to_owned());
        set("TMPDIR", path_str("TMPDIR", &b.tmpdir)?.to_owned());
        set("ORG_OFFICE", b.office_id.clone());
        set("ORG_OFFICE_HOME", office_home.to_owned());
        if let Some(mark) = &self.mark {
            check_mark(mark)?;
            set("ORG_SESSION_MARK", mark.clone());
        }
        let session = self.session.is_some();
        if let Some(s) = self.session {
            set("ORG_AGENT", s.agent);
            set("ORG_SESSION_TOKEN", s.token.0);
            // Rev 3.3: no git or gh setting is a file in the runtime dir. They travel in the
            // environment, which overrides every config file.
            set("GIT_CONFIG_NOSYSTEM", "1".to_owned());
            set("GIT_CONFIG_GLOBAL", "/dev/null".to_owned());
            let config = [
                ("user.name", s.git_user_name),
                ("user.email", s.git_user_email),
                ("credential.helper", String::new()),
                ("core.hooksPath", "/dev/null".to_owned()),
                ("core.fsmonitor", "false".to_owned()),
                ("gc.auto", "0".to_owned()),
                ("maintenance.auto", "false".to_owned()),
            ];
            set("GIT_CONFIG_COUNT", config.len().to_string());
            for (i, (key, value)) in config.into_iter().enumerate() {
                set(&format!("GIT_CONFIG_KEY_{i}"), key.to_owned());
                set(&format!("GIT_CONFIG_VALUE_{i}"), value);
            }
            // An empty folder the office makes for the session, in its TMPDIR.
            let gh = b.tmpdir.join("gh");
            set("GH_CONFIG_DIR", format!("{}/", path_str("TMPDIR", &gh)?));
        }
        let env = Env {
            vars,
            term: self.term,
            mark: self.mark,
            session,
        };
        env.check_values()?;
        Ok(env)
    }
}

/// A child's whole environment, ready to spawn with.
///
/// Its `Debug` lists variable **names** only, never values, so it can't leak the session
/// token into a log.
#[derive(Clone)]
pub struct Env {
    vars: BTreeMap<String, String>,
    term: Option<String>,
    mark: Option<String>,
    session: bool,
}

impl Env {
    /// An environment of exactly these variables, for a child that isn't an agent's (an
    /// office server, say). The first value of a repeated name wins. A `TERM` is passed to a
    /// terminal child only.
    pub fn from_vars(vars: impl IntoIterator<Item = (EnvName, String)>) -> Result<Env> {
        let mut map = BTreeMap::new();
        for (name, value) in vars {
            map.entry(name.as_str().to_owned()).or_insert(value);
        }
        // `TERM` belongs to a terminal only, like the builder's.
        let term = map.remove("TERM");
        let mark = map.get("ORG_SESSION_MARK").cloned();
        if let Some(mark) = &mark {
            check_mark(mark)?;
        }
        let env = Env {
            vars: map,
            term,
            mark,
            session: false,
        };
        env.check_values()?;
        Ok(env)
    }

    /// The names of the variables a piped child gets, sorted.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.vars.keys().map(String::as_str)
    }

    /// `ORG_SESSION_MARK`, if set.
    pub fn mark(&self) -> Option<&str> {
        self.mark.as_deref()
    }

    /// Whether it holds an agent session's values ([`EnvBuilder::agent_session`]).
    pub(crate) fn is_agent_session(&self) -> bool {
        self.session
    }

    /// This environment plus `add`, which never overrides it: an entry naming a variable
    /// already set, a reserved name or prefix (see [`RESERVED_PREFIXES`]) is dropped, and
    /// so is a repeat of an earlier entry.
    pub(crate) fn with_additions(&self, add: &[(EnvName, String)]) -> Result<Env> {
        let mut env = self.clone();
        for (name, value) in add {
            let name = name.as_str();
            let reserved = RESERVED_NAMES.contains(&name)
                || RESERVED_PREFIXES.iter().any(|p| name.starts_with(p));
            if reserved || env.vars.contains_key(name) {
                continue;
            }
            if value.contains('\0') {
                return Err(Error::invalid(format!(
                    "the value of `{name}` holds a NUL byte"
                )));
            }
            env.vars.insert(name.to_owned(), value.clone());
        }
        Ok(env)
    }

    /// The `envp` to spawn with: `NAME=value` per variable, plus `TERM` on a terminal only.
    pub(crate) fn to_envp(&self, terminal: bool) -> Result<Vec<CString>> {
        let term = terminal.then(|| self.term.as_deref().unwrap_or(DEFAULT_TERM));
        let mut out = Vec::with_capacity(self.vars.len() + 1);
        let mut push = |name: &str, value: &str| {
            CString::new(format!("{name}={value}"))
                .map(|c| out.push(c))
                .map_err(|_| Error::invalid(format!("the value of `{name}` holds a NUL byte")))
        };
        for (name, value) in &self.vars {
            push(name, value)?;
        }
        if let Some(term) = term {
            push("TERM", term)?;
        }
        Ok(out)
    }

    fn check_values(&self) -> Result<()> {
        for (name, value) in self
            .vars
            .iter()
            .map(|(n, v)| (n.as_str(), v.as_str()))
            .chain(self.term.as_deref().map(|t| ("TERM", t)))
        {
            if value.contains('\0') {
                return Err(Error::invalid(format!(
                    "the value of `{name}` holds a NUL byte"
                )));
            }
        }
        Ok(())
    }
}

impl fmt::Debug for Env {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Env")
            .field("names", &self.vars.keys().collect::<Vec<_>>())
            .field("term", &self.term.is_some())
            .finish_non_exhaustive()
    }
}

fn require_absolute(what: &str, path: &Path) -> Result<()> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(Error::invalid(format!("{what} isn't an absolute path")))
    }
}

fn path_str<'a>(what: &str, path: &'a Path) -> Result<&'a str> {
    path.to_str()
        .ok_or_else(|| Error::invalid(format!("{what} isn't valid UTF-8")))
}

/// A mark is matched as `ORG_SESSION_MARK=<mark>` in other processes' environments, so it
/// must be one printable, `=`-free token.
fn check_mark(mark: &str) -> Result<()> {
    if mark.is_empty()
        || mark.len() > 256
        || !mark.bytes().all(|b| b.is_ascii_graphic() && b != b'=')
    {
        return Err(Error::invalid(
            "the session mark must be 1-256 printable ASCII characters without `=`",
        ));
    }
    Ok(())
}

/// `<office home>/bin`, then the login `PATH`'s entries that pass the filter (see
/// [`EnvBuilder::build`]).
fn compose_path(
    office_home: &Path,
    login_path: &str,
    excludes: &[PathBuf],
    git_bin_dir: Option<&Path>,
) -> String {
    let office_bin = office_home.join("bin");
    let mut roots: Vec<PathBuf> = excludes.to_vec();
    roots.extend(excludes.iter().map(|p| real_prefix(p)));
    let mut seen: Vec<PathBuf> = vec![office_bin.clone()];
    let mut parts = vec![office_bin.to_string_lossy().into_owned()];
    for raw in login_path.split(':') {
        if !is_plain_absolute(raw) {
            continue;
        }
        let entry = Path::new(raw);
        // As written, and by the real path of its longest existing prefix (so an entry that
        // doesn't exist yet can't reach a root through a symlink such as `/var`).
        if inside_any(entry, &roots) || inside_any(&real_prefix(entry), &roots) {
            continue;
        }
        if seen.iter().any(|s| s == entry) {
            continue;
        }
        seen.push(entry.to_path_buf());
        // `/` stays `/`: trimmed to nothing it would be an empty entry, which means the cwd.
        let trimmed = raw.trim_end_matches('/');
        parts.push(if trimmed.is_empty() { "/" } else { trimmed }.to_owned());
    }
    // Codex's real git goes before `/usr/bin` (its launcher there stalls in Codex's sandbox),
    // or right after the office's folder when `/usr/bin` isn't listed.
    if let Some(dir) = git_bin_dir {
        let dir = dir.to_string_lossy().trim_end_matches('/').to_owned();
        parts.retain(|p| *p != dir);
        let at = parts.iter().position(|p| p == "/usr/bin").unwrap_or(1);
        parts.insert(at, dir);
    }
    parts.join(":")
}

/// `path` with its longest existing prefix replaced by that prefix's real path.
fn real_prefix(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut cur = path;
    loop {
        if let Ok(real) = std::fs::canonicalize(cur) {
            return rest.iter().rev().fold(real, |acc, c| acc.join(c));
        }
        match (cur.parent(), cur.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                cur = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Whether `entry` is one of `roots` or inside one, component by component, ignoring ASCII
/// case (macOS file systems usually do, and dropping an extra entry is the safe side).
fn inside_any(entry: &Path, roots: &[PathBuf]) -> bool {
    let lower = |p: &Path| PathBuf::from(p.to_string_lossy().to_ascii_lowercase());
    let entry = lower(entry);
    roots.iter().any(|r| entry.starts_with(lower(r)))
}

/// Absolute, with no `.`, `..` or empty component (a trailing `/` is fine).
fn is_plain_absolute(raw: &str) -> bool {
    if !raw.starts_with('/') || raw.contains('\0') {
        return false;
    }
    let trimmed = raw.trim_end_matches('/');
    if trimmed.is_empty() {
        return true; // "/" itself
    }
    if trimmed[1..]
        .split('/')
        .any(|c| c.is_empty() || c == "." || c == "..")
    {
        return false;
    }
    Path::new(trimmed)
        .components()
        .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compose_path_t(office_home: &Path, login_path: &str, excludes: &[PathBuf]) -> String {
        compose_path(office_home, login_path, excludes, None)
    }

    #[test]
    fn codex_real_git_goes_before_usr_bin() {
        let git = Some(Path::new(
            "/Applications/Xcode.app/Contents/Developer/usr/bin",
        ));
        assert_eq!(
            compose_path(Path::new("/o"), "/opt/bin:/usr/bin:/bin", &[], git),
            "/o/bin:/opt/bin:/Applications/Xcode.app/Contents/Developer/usr/bin:/usr/bin:/bin"
        );
        assert_eq!(
            compose_path(Path::new("/o"), "/bin", &[], git),
            "/o/bin:/Applications/Xcode.app/Contents/Developer/usr/bin:/bin"
        );
    }

    fn base() -> BaseEnv {
        BaseEnv {
            office_home: "/o/home".into(),
            office_id: "off1".into(),
            login_path: String::new(),
            home: "/Users/u".into(),
            lang: Some("en_US.UTF-8".into()),
            user: "u".into(),
            logname: "u".into(),
            shell: "/bin/zsh".into(),
            tmpdir: "/private/tmp/s1".into(),
        }
    }

    fn session() -> AgentSessionEnv {
        AgentSessionEnv {
            agent: "ac-dev-ada".into(),
            token: SessionToken::new("s3cr3t-token".into()),
            workspace: "/w/ws".into(),
            agent_home: "/w/home".into(),
            git_user_name: "ada (agent)".into(),
            git_user_email: "ada@agents.example".into(),
            git_bin_dir: None,
        }
    }

    #[test]
    fn path_puts_office_bin_first_and_filters_the_login_path() {
        let path = compose_path_t(
            Path::new("/o/home"),
            "/usr/bin::rel/bin:./x:/w/ws/node_modules/.bin:/w/home/bin:/w/wsx/bin:/a/../b:\
             /a//b:/usr/bin:/o/home/bin:/opt/homebrew/bin/:/w/ws",
            &["/w/ws".into(), "/w/home".into()],
        );
        assert_eq!(path, "/o/home/bin:/usr/bin:/w/wsx/bin:/opt/homebrew/bin");
        let path = compose_path_t(Path::new("/o"), "/:/usr/bin:", &[]);
        assert_eq!(path, "/o/bin:/:/usr/bin", "never an empty (cwd) entry");
        let path = compose_path_t(Path::new("/o"), "/W/WS/bin:/w/home2", &["/w/ws".into()]);
        assert_eq!(path, "/o/bin:/w/home2", "case is ignored");
    }

    #[test]
    fn path_drops_entries_that_resolve_into_an_excluded_root() {
        let root = std::env::temp_dir().join(format!("osv-env-{}", std::process::id()));
        let ws = root.join("ws");
        std::fs::create_dir_all(ws.join("bin")).unwrap();
        let link = root.join("link");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(ws.join("bin"), &link).unwrap();
        let path = compose_path_t(
            Path::new("/o"),
            // Through the link, existing or not yet; and the workspace spelled through the
            // temp folder's own symlinks (`/var` is `/private/var`).
            &format!(
                "{0}:{0}/not-yet/bin:{1}/later/bin:/usr/bin",
                link.display(),
                ws.display()
            ),
            &[std::fs::canonicalize(&ws).unwrap()],
        );
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(path, "/o/bin:/usr/bin");
    }

    #[test]
    fn build_makes_exactly_the_allowlist() {
        let env = EnvBuilder::new(base())
            .agent_session(session())
            .mark("m-123".into())
            .build()
            .unwrap();
        let names: Vec<&str> = env.names().collect();
        assert_eq!(
            names,
            [
                "GH_CONFIG_DIR",
                "GIT_CONFIG_COUNT",
                "GIT_CONFIG_GLOBAL",
                "GIT_CONFIG_KEY_0",
                "GIT_CONFIG_KEY_1",
                "GIT_CONFIG_KEY_2",
                "GIT_CONFIG_KEY_3",
                "GIT_CONFIG_KEY_4",
                "GIT_CONFIG_KEY_5",
                "GIT_CONFIG_KEY_6",
                "GIT_CONFIG_NOSYSTEM",
                "GIT_CONFIG_VALUE_0",
                "GIT_CONFIG_VALUE_1",
                "GIT_CONFIG_VALUE_2",
                "GIT_CONFIG_VALUE_3",
                "GIT_CONFIG_VALUE_4",
                "GIT_CONFIG_VALUE_5",
                "GIT_CONFIG_VALUE_6",
                "HOME",
                "LANG",
                "LOGNAME",
                "ORG_AGENT",
                "ORG_OFFICE",
                "ORG_OFFICE_HOME",
                "ORG_SESSION_MARK",
                "ORG_SESSION_TOKEN",
                "PATH",
                "SHELL",
                "TMPDIR",
                "USER",
            ]
        );
        let envp = env.to_envp(false).unwrap();
        let has = |s: &str| envp.iter().any(|c| c.to_str().unwrap() == s);
        assert!(has("GIT_CONFIG_GLOBAL=/dev/null"));
        assert!(has("GIT_CONFIG_KEY_2=credential.helper"));
        assert!(has("GIT_CONFIG_VALUE_2="));
        assert!(has("GIT_CONFIG_KEY_3=core.hooksPath"));
        assert!(has("GIT_CONFIG_VALUE_3=/dev/null"));
        assert!(has("GIT_CONFIG_COUNT=7"));
        assert!(has("GH_CONFIG_DIR=/private/tmp/s1/gh/"));
        assert!(has("ORG_SESSION_TOKEN=s3cr3t-token"));
        assert!(!envp
            .iter()
            .any(|c| c.to_str().unwrap().starts_with("TERM=")));
        assert!(env
            .to_envp(true)
            .unwrap()
            .iter()
            .any(|c| c.to_str().unwrap() == "TERM=xterm-256color"));
    }

    #[test]
    fn debug_never_shows_values() {
        let env = EnvBuilder::new(base())
            .agent_session(session())
            .build()
            .unwrap();
        let dbg = format!("{env:?} {:?}", session());
        assert!(!dbg.contains("s3cr3t-token"), "{dbg}");
        assert!(dbg.contains("ORG_SESSION_TOKEN"));
    }

    #[test]
    fn additions_never_override_the_office() {
        let env = EnvBuilder::new(base()).build().unwrap();
        let name = |s: &str| EnvName::try_from(s).unwrap();
        let env = env
            .with_additions(&[
                (name("DISABLE_AUTOUPDATER"), "1".into()),
                (name("HOME"), "/evil".into()),
                (name("TERM"), "dumb".into()),
                (name("GIT_DIR"), "/evil".into()),
                (
                    name("GIT_CONFIG_PARAMETERS"),
                    "'core.hookspath'='/x'".into(),
                ),
                (name("ORG_SESSION_TOKEN"), "forged".into()),
                (name("DYLD_INSERT_LIBRARIES"), "/x.dylib".into()),
                (name("DISABLE_AUTOUPDATER"), "0".into()),
            ])
            .unwrap();
        let envp: Vec<String> = env
            .to_envp(false)
            .unwrap()
            .into_iter()
            .map(|c| c.into_string().unwrap())
            .collect();
        assert!(envp.contains(&"DISABLE_AUTOUPDATER=1".to_owned()));
        assert!(envp.contains(&"HOME=/Users/u".to_owned()));
        for bad in [
            "TERM=",
            "GIT_",
            "ORG_SESSION_TOKEN",
            "DYLD_",
            "DISABLE_AUTOUPDATER=0",
        ] {
            assert!(!envp.iter().any(|e| e.starts_with(bad)), "{bad}");
        }
    }

    #[test]
    fn bad_values_are_refused() {
        let mut b = base();
        b.user = "u\0x".into();
        assert!(EnvBuilder::new(b).build().is_err());
        let mut b = base();
        b.tmpdir = "rel".into();
        assert!(EnvBuilder::new(b).build().is_err());
        let mut b = base();
        b.office_home = "/o:x".into();
        assert!(EnvBuilder::new(b).build().is_err());
        for mark in ["", "a=b", "a b", "a\nb"] {
            assert!(EnvBuilder::new(base()).mark(mark.into()).build().is_err());
        }
        let env = EnvBuilder::new(base()).build().unwrap();
        let add = [(EnvName::try_from("X").unwrap(), "a\0b".to_owned())];
        assert!(env.with_additions(&add).is_err());
    }
}
