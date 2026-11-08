//! Reading the places an existing team lives in, for "Bring in an existing team". Read-only: nothing here writes, and
//! nothing runs. Two sources:
//!
//! - the HR workspace: `projects/*/agents/*/agent.json` (one record per agent) and `hr/roles/*.toml` (the role files);
//! - the messaging spool: the directory names under `inbox/`, one per registered name.
//!
//! The folders are the owner's choice (they are never fixed in the app); `defaults` only looks in the usual places so the
//! first screen can be prefilled. Files are read by fixed names and size-limited, folders inside the chosen one are not
//! followed through symbolic links (the chosen folder itself may be one), and a record or folder that can't be read is
//! reported on its own without hiding the others.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// The most agents and names one scan returns, and the largest file it reads.
const MAX_ENTRIES: usize = 1_000;
/// The most notes about what was skipped one scan carries.
const MAX_NOTES: usize = 50;
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// The folders the owner can have scanned: the usual places the app found and the ones the owner picked. The webview
/// names a folder to scan only by one of these, so it can't use the scan to look anywhere else on the disk.
#[derive(Default)]
pub struct Granted(std::sync::Mutex<std::collections::HashSet<String>>);

impl Granted {
    pub fn grant(&self, path: &str) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(path.to_string());
    }

    pub fn allows(&self, path: &str) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(path)
    }
}

/// The usual places, found or not.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Defaults {
    pub hr: Option<String>,
    pub messaging: Option<String>,
    /// The name this Mac's user goes by: the owner's own inbox is not an agent.
    pub user: Option<String>,
}

/// One HR record. Missing fields stay missing: the review asks the owner about them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HrAgent {
    pub id: String,
    pub name: Option<String>,
    pub project: Option<String>,
    pub role: Option<String>,
    pub title: Option<String>,
    pub department: Option<String>,
    pub focus: Option<String>,
    pub reports_to: Option<String>,
    pub model: Option<String>,
    pub status: Option<String>,
    pub hired_on: Option<String>,
    /// As HR wrote it (relative to the HR workspace). `home_path` is where it is on this Mac.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    #[serde(skip_deserializing)]
    pub home_path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct HrScan {
    pub path: String,
    /// Why the workspace could not be read at all (the other source still works).
    pub error: Option<String>,
    pub agents: Vec<HrAgent>,
    /// The role files' names (`software-engineer`), for the role picker.
    pub roles: Vec<String>,
    /// Records that were skipped, each with why (the first few: `skipped_more` counts the rest).
    pub skipped: Vec<String>,
    pub skipped_more: usize,
    /// There were more agents than this scan reads: the first ones are listed.
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MessagingScan {
    pub path: String,
    pub error: Option<String>,
    pub names: Vec<String>,
    /// There were more names than this scan reads: the first ones are listed.
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Scan {
    pub hr: Option<HrScan>,
    pub messaging: Option<MessagingScan>,
}

/// Look in the usual places under `home`: an HR workspace is a folder holding `hr/roles` and `projects`, found as
/// `projects/*/agents` (or `agents`) under the user's projects folder; the spool is `.agency/spool`.
pub fn defaults(home: &Path, user: Option<String>) -> Defaults {
    let spool = home.join(".agency").join("spool");
    let messaging = plain_dir(&spool.join("inbox")).then(|| path_text(&spool));
    let mut hr = None;
    if let Ok(projects) = fs::read_dir(home.join("projects")) {
        let mut found: Vec<PathBuf> = projects
            .flatten()
            .flat_map(|entry| [entry.path().join("agents"), entry.path()])
            .filter(|dir| is_hr_workspace(dir))
            .collect();
        found.sort();
        hr = found.first().map(|p| path_text(p));
    }
    Defaults {
        hr,
        messaging,
        user,
    }
}

/// One team of a hints file, as the owner wrote it: `[teams.<key>]` with `name`, `parent`, `lead` and `members` (messaging
/// names or HR ids). Nothing is checked here against the agents found: the review does that and lists what isn't found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HintTeam {
    #[serde(skip_deserializing)]
    pub key: String,
    pub name: Option<String>,
    pub parent: Option<String>,
    pub lead: Option<String>,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Hints {
    pub path: String,
    /// Why the file could not be used (the import goes on without it).
    pub error: Option<String>,
    pub teams: Vec<HintTeam>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct HintsFile {
    teams: std::collections::BTreeMap<String, HintTeam>,
}

const MAX_HINT_TEAMS: usize = 200;

/// Read the team hints file the owner chose. A file that can't be read, or isn't what it should be, says why.
pub fn read_hints(path: &Path) -> Hints {
    let mut out = Hints {
        path: path_text(path),
        ..Hints::default()
    };
    match read_small_file(path, true, "a teams file").and_then(|text| {
        toml::from_str::<HintsFile>(&text).map_err(|_| "isn't a readable teams file".to_string())
    }) {
        Ok(file) => {
            if file.teams.len() > MAX_HINT_TEAMS {
                out.error = Some("More teams than this import reads.".into());
                return out;
            }
            out.teams = file
                .teams
                .into_iter()
                .map(|(key, mut team)| {
                    team.key = key;
                    team.members.truncate(MAX_ENTRIES);
                    team
                })
                .collect();
        }
        Err(why) => out.error = Some(format!("The file {why}.")),
    }
    out
}

/// `read_hints` for a file named by the webview: it must be one the app handed out (the picker's).
pub fn read_hints_granted(granted: &Granted, path: &str) -> Result<Hints, NotGranted> {
    granted
        .allows(path)
        .then(|| read_hints(Path::new(path)))
        .ok_or(NotGranted)
}

/// A small text file, opened without blocking and checked on the open handle (so what was checked is what is read): a
/// plain file, not larger than `MAX_FILE_BYTES`. `follow_links` is false inside a workspace (nothing leads out of it) and true
/// for a file the owner chose themselves. `what` names it in the errors ("an agent record", "a teams file").
fn read_small_file(path: &Path, follow_links: bool, what: &str) -> Result<String, String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let flags = libc::O_NONBLOCK | if follow_links { 0 } else { libc::O_NOFOLLOW };
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(path)
        .map_err(|e| format!("can't be read ({e})"))?;
    let meta = file
        .metadata()
        .map_err(|e| format!("can't be read ({e})"))?;
    if !meta.is_file() {
        return Err("isn't a plain file".into());
    }
    let too_large = || format!("is too large to be {what}");
    if meta.len() > MAX_FILE_BYTES {
        return Err(too_large());
    }
    let mut text = String::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|e| format!("can't be read ({e})"))?;
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err(too_large());
    }
    Ok(text)
}

/// A real directory: a symbolic link to one is not (inside the chosen folder, nothing is followed out of it).
fn plain_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

fn is_hr_workspace(dir: &Path) -> bool {
    plain_dir(&dir.join("hr"))
        && plain_dir(&dir.join("hr").join("roles"))
        && plain_dir(&dir.join("projects"))
}

/// A folder the webview named was not one the app handed out.
#[derive(Debug, PartialEq, Eq)]
pub struct NotGranted;

/// `scan` for folders named by the webview: every one must be a folder the app handed out, or nothing is read at all.
pub fn scan_granted(
    granted: &Granted,
    hr: Option<&str>,
    spool: Option<&str>,
) -> Result<Scan, NotGranted> {
    if hr.into_iter().chain(spool).any(|dir| !granted.allows(dir)) {
        return Err(NotGranted);
    }
    Ok(scan(hr.map(Path::new), spool.map(Path::new)))
}

/// Read what is there. A source the owner did not give is `None`; one that can't be read says why.
pub fn scan(hr: Option<&Path>, spool: Option<&Path>) -> Scan {
    Scan {
        hr: hr.map(scan_hr),
        messaging: spool.map(scan_messaging),
    }
}

fn scan_hr(root: &Path) -> HrScan {
    let mut out = HrScan {
        path: path_text(root),
        ..HrScan::default()
    };
    if !root.is_dir() {
        out.error = Some("That folder isn't there.".into());
        return out;
    }
    if !is_hr_workspace(root) {
        out.error =
            Some("That folder has no hr/roles and projects: it isn't an HR workspace.".into());
        return out;
    }
    let mut notes = Notes::default();
    let roles = root.join("hr").join("roles");
    match fs::read_dir(&roles) {
        Ok(entries) => {
            out.roles = entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    name.strip_suffix(".toml")
                        .filter(|stem| !stem.starts_with('_'))
                        .map(str::to_string)
                })
                .collect();
            out.roles.sort();
        }
        Err(e) => note(&mut notes, format!("hr/roles can't be read ({e})")),
    }
    'projects: for project in dirs_in(&root.join("projects"), root, &mut notes) {
        for agent_dir in dirs_in(&project.join("agents"), root, &mut notes) {
            let record = agent_dir.join("agent.json");
            if matches!(fs::symlink_metadata(&record), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
            {
                continue; // not an agent's home
            }
            if out.agents.len() >= MAX_ENTRIES {
                out.truncated = true;
                break 'projects;
            }
            match read_agent(&record, root) {
                Ok(agent) => out.agents.push(agent),
                Err(why) => note(&mut notes, format!("{}: {why}", short(root, &record))),
            }
        }
    }
    out.skipped = notes.list;
    out.skipped_more = notes.more;
    out
}

/// Remember something that was skipped, up to a limit (the rest are only counted, so a flood of them can't grow the answer).
fn note(notes: &mut Notes, text: String) {
    if notes.list.len() < MAX_NOTES {
        notes.list.push(text);
    } else {
        notes.more += 1;
    }
}

#[derive(Default)]
struct Notes {
    list: Vec<String>,
    more: usize,
}

/// The agent record at `record`, or why it can't be read.
fn read_agent(record: &Path, root: &Path) -> Result<HrAgent, String> {
    let text = read_small_file(record, false, "an agent record")?;
    let mut agent: HrAgent =
        serde_json::from_str(&text).map_err(|_| "isn't a readable agent record".to_string())?;
    if agent.id.trim().is_empty() {
        return Err("has no id".into());
    }
    agent.home_path = agent.home.as_deref().and_then(|home| resolve(root, home));
    Ok(agent)
}

/// Where a relative home is on this Mac, or `None` for anything that could leave the workspace.
fn resolve(root: &Path, home: &str) -> Option<String> {
    let relative = Path::new(home);
    let plain = !home.is_empty()
        && relative
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
    plain.then(|| path_text(&root.join(relative)))
}

fn scan_messaging(spool: &Path) -> MessagingScan {
    let mut out = MessagingScan {
        path: path_text(spool),
        ..MessagingScan::default()
    };
    let inbox = spool.join("inbox");
    if !plain_dir(&inbox) {
        out.error = Some("That folder has no inbox: it isn't a messaging spool.".into());
        return out;
    }
    let mut unread = Notes::default();
    let mut names: Vec<String> = dirs_in(&inbox, spool, &mut unread)
        .into_iter()
        .filter_map(|dir| dir.file_name().map(|n| n.to_string_lossy().into_owned()))
        .filter(|name| !name.starts_with('.'))
        .collect();
    if let Some(why) = unread.list.first() {
        out.error = Some(format!("The inbox can't be read ({why})."));
        return out;
    }
    if names.len() > MAX_ENTRIES {
        names.truncate(MAX_ENTRIES);
        out.truncated = true;
    }
    out.names = names;
    out
}

/// The sub-directories of `dir` that are real directories (a symbolic link is not followed), by name. A folder that
/// can't be read is noted in `skipped` (a missing one is just empty).
fn dirs_in(dir: &Path, root: &Path, skipped: &mut Notes) -> Vec<PathBuf> {
    if fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink()) {
        note(
            skipped,
            format!("{} is a link, which is not followed", short(root, dir)),
        );
        return Vec::new();
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            note(skipped, format!("{} can't be read ({e})", short(root, dir)));
            return Vec::new();
        }
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .map(|entry| entry.path())
        .collect();
    dirs.sort();
    dirs
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn short(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(path_text)
        .unwrap_or_else(|_| path_text(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// An HR workspace with two agents (one offboarded), one broken record and a role template.
    fn workspace(root: &Path) {
        write(&root.join("hr/roles/software-engineer.toml"), "");
        write(&root.join("hr/roles/_template.toml"), "");
        write(
            &root.join("projects/ac/agents/ac-staff-engineer-ada/agent.json"),
            r#"{"id":"ac-staff-engineer-ada","name":"ada","project":"ac","role":"staff-engineer","title":"Staff Engineer",
                "department":"Engineering","focus":null,"reports_to":"user","model":"inherit","status":"active",
                "hired_on":"2026-11-07","home":"projects/ac/agents/ac-staff-engineer-ada"}"#,
        );
        write(
            &root.join("projects/ac/agents/ac-old/agent.json"),
            r#"{"id":"ac-old","status":"offboarded","home":"../outside"}"#,
        );
        write(&root.join("projects/ac/agents/broken/agent.json"), "{ nope");
        write(&root.join("projects/ac/agents/no-record/readme.md"), "x");
    }

    #[test]
    fn it_reads_hr_records_and_role_files_and_reports_the_broken_one_on_its_own() {
        let dir = tempfile::tempdir().unwrap();
        workspace(dir.path());
        let scan = scan(Some(dir.path()), None);
        let hr = scan.hr.unwrap();
        assert_eq!(hr.error, None);
        assert_eq!(
            hr.roles,
            ["software-engineer"],
            "the template is not a role"
        );
        let ids: Vec<_> = hr.agents.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["ac-old", "ac-staff-engineer-ada"]);
        let ada = &hr.agents[1];
        assert_eq!(ada.title.as_deref(), Some("Staff Engineer"));
        assert_eq!(ada.reports_to.as_deref(), Some("user"));
        assert_eq!(ada.focus, None);
        assert_eq!(
            ada.home_path.as_deref(),
            Some(&*path_text(
                &dir.path().join("projects/ac/agents/ac-staff-engineer-ada")
            ))
        );
        assert_eq!(
            hr.agents[0].home_path, None,
            "a home that leaves the workspace is not resolved"
        );
        assert_eq!(hr.skipped.len(), 1);
        assert!(hr.skipped[0].contains("broken"), "{:?}", hr.skipped);
        assert!(scan.messaging.is_none());
    }

    #[test]
    fn it_reads_the_registered_names_from_the_inbox_directories() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["server-dev", "codex-dev", ".hidden"] {
            fs::create_dir_all(dir.path().join("inbox").join(name)).unwrap();
        }
        write(&dir.path().join("inbox/stray-file"), "x");
        fs::create_dir_all(dir.path().join("outbox")).unwrap();
        let spool = scan(None, Some(dir.path())).messaging.unwrap();
        assert_eq!(spool.names, ["codex-dev", "server-dev"]);
        assert_eq!(spool.error, None);
    }

    #[test]
    fn a_source_that_is_not_what_it_should_be_says_so_and_the_other_still_works() {
        let hr_dir = tempfile::tempdir().unwrap();
        let spool_dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(spool_dir.path().join("inbox/x")).unwrap();
        let scan = scan(Some(hr_dir.path()), Some(spool_dir.path()));
        assert!(scan
            .hr
            .unwrap()
            .error
            .unwrap()
            .contains("isn't an HR workspace"));
        assert_eq!(scan.messaging.unwrap().names, ["x"]);
        let gone = super::scan(Some(&hr_dir.path().join("missing")), None);
        assert!(gone.hr.unwrap().error.unwrap().contains("isn't there"));
        let no_inbox = super::scan(None, Some(hr_dir.path()));
        assert!(no_inbox
            .messaging
            .unwrap()
            .error
            .unwrap()
            .contains("no inbox"));
    }

    #[test]
    fn it_does_not_read_through_a_symbolic_link_or_a_huge_file() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        workspace(dir.path());
        write(&outside.path().join("agent.json"), r#"{"id":"stolen"}"#);
        fs::create_dir_all(dir.path().join("projects/ac/agents/linked")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("agent.json"),
            dir.path().join("projects/ac/agents/linked/agent.json"),
        )
        .unwrap();
        std::os::unix::fs::symlink(
            outside.path(),
            dir.path().join("projects/ac/agents/dirlink"),
        )
        .unwrap();
        write(
            &dir.path().join("projects/ac/agents/huge/agent.json"),
            &format!(r#"{{"id":"huge","focus":"{}"}}"#, "x".repeat(70_000)),
        );
        let hr = scan(Some(dir.path()), None).hr.unwrap();
        let ids: Vec<_> = hr.agents.iter().map(|a| a.id.as_str()).collect();
        assert!(
            !ids.contains(&"stolen") && !ids.contains(&"huge"),
            "{ids:?}"
        );
        assert_eq!(
            hr.skipped.len(),
            3,
            "broken, linked and huge: {:?}",
            hr.skipped
        );
    }

    #[test]
    fn the_usual_places_are_found_when_they_are_there_and_not_invented() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(defaults(home.path(), Some("vgmello".into())).hr, None);
        workspace(&home.path().join("projects/agentic-consulting/agents"));
        fs::create_dir_all(home.path().join(".agency/spool/inbox")).unwrap();
        let found = defaults(home.path(), Some("vgmello".into()));
        assert_eq!(
            found.hr.as_deref(),
            Some(&*path_text(
                &home.path().join("projects/agentic-consulting/agents")
            ))
        );
        assert_eq!(
            found.messaging.as_deref(),
            Some(&*path_text(&home.path().join(".agency/spool")))
        );
        assert_eq!(found.user.as_deref(), Some("vgmello"));
    }

    #[test]
    fn a_folder_symlinked_inside_the_workspace_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        workspace(dir.path());
        write(
            &outside.path().join("p/agents/x/agent.json"),
            r#"{"id":"stolen"}"#,
        );
        // A whole projects/<p>/agents folder, and the inbox, are links to somewhere else.
        fs::create_dir_all(dir.path().join("projects/linked")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("p/agents"),
            dir.path().join("projects/linked/agents"),
        )
        .unwrap();
        let hr = scan(Some(dir.path()), None).hr.unwrap();
        assert!(!hr.agents.iter().any(|a| a.id == "stolen"));
        let spool = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), spool.path().join("inbox")).unwrap();
        assert!(scan(None, Some(spool.path()))
            .messaging
            .unwrap()
            .error
            .unwrap()
            .contains("no inbox"));
    }

    #[test]
    fn a_folder_that_cannot_be_read_is_said_not_shown_empty() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("inbox/ada")).unwrap();
        fs::set_permissions(dir.path().join("inbox"), fs::Permissions::from_mode(0o000)).unwrap();
        let spool = scan(None, Some(dir.path())).messaging.unwrap();
        fs::set_permissions(dir.path().join("inbox"), fs::Permissions::from_mode(0o755)).unwrap();
        if spool.error.is_some() || !spool.names.is_empty() {
            // (Skipped when the tests run as a user that ignores permissions.)
            assert!(spool.error.unwrap().contains("can't be read"));
            assert!(spool.names.is_empty());
        }
    }

    #[test]
    fn more_than_the_limit_is_said_and_the_notes_are_limited_too() {
        let spool = tempfile::tempdir().unwrap();
        for n in 0..(MAX_ENTRIES + 5) {
            fs::create_dir_all(spool.path().join(format!("inbox/a{n:05}"))).unwrap();
        }
        let messaging = scan(None, Some(spool.path())).messaging.unwrap();
        assert_eq!(messaging.names.len(), MAX_ENTRIES);
        assert!(messaging.truncated && messaging.error.is_none());
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("hr/roles/r.toml"), "");
        for n in 0..(MAX_NOTES + 20) {
            write(
                &dir.path()
                    .join(format!("projects/p/agents/a{n}/agent.json")),
                "{ nope",
            );
        }
        let hr = scan(Some(dir.path()), None).hr.unwrap();
        assert_eq!(hr.skipped.len(), MAX_NOTES);
        assert_eq!(hr.skipped_more, 20, "and the rest are counted");
    }

    #[test]
    fn a_home_that_is_absolute_or_empty_is_not_resolved_and_a_role_directory_is_not_a_role() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("hr/roles/real.toml"), "");
        fs::create_dir_all(dir.path().join("hr/roles/fake.toml")).unwrap();
        write(
            &dir.path().join("projects/p/agents/a/agent.json"),
            r#"{"id":"a","home":"/etc"}"#,
        );
        write(
            &dir.path().join("projects/p/agents/b/agent.json"),
            r#"{"id":"b","home":""}"#,
        );
        let hr = scan(Some(dir.path()), None).hr.unwrap();
        assert_eq!(hr.roles, ["real"]);
        assert!(hr.agents.iter().all(|a| a.home_path.is_none()));
    }

    #[test]
    fn a_link_in_hr_is_not_followed_and_a_pipe_does_not_hang_the_scan() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("roles/stolen.toml"), "");
        fs::create_dir_all(dir.path().join("projects/p")).unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("hr")).unwrap();
        assert!(scan(Some(dir.path()), None)
            .hr
            .unwrap()
            .error
            .unwrap()
            .contains("isn't an HR workspace"));
        let ws = tempfile::tempdir().unwrap();
        workspace(ws.path());
        let pipe = ws.path().join("projects/ac/agents/pipe/agent.json");
        fs::create_dir_all(pipe.parent().unwrap()).unwrap();
        let made = std::process::Command::new("mkfifo")
            .arg(&pipe)
            .status()
            .unwrap();
        assert!(made.success());
        let hr = scan(Some(ws.path()), None).hr.unwrap();
        assert!(hr.agents.iter().all(|a| a.id != "pipe"));
        assert!(
            hr.skipped.iter().any(|s| s.contains("pipe")),
            "{:?}",
            hr.skipped
        );
    }

    #[test]
    fn the_webview_may_scan_only_what_was_handed_out_and_nothing_is_read_otherwise() {
        let dir = tempfile::tempdir().unwrap();
        workspace(dir.path());
        let spool = tempfile::tempdir().unwrap();
        fs::create_dir_all(spool.path().join("inbox/x")).unwrap();
        let (hr, sp) = (path_text(dir.path()), path_text(spool.path()));
        let granted = Granted::default();
        granted.grant(&hr);
        // Never granted, or a granted path with something appended: refused.
        assert_eq!(scan_granted(&granted, Some(&sp), None), Err(NotGranted));
        assert_eq!(
            scan_granted(&granted, Some(&format!("{hr}/../x")), None),
            Err(NotGranted)
        );
        // One granted and one not: refused as a whole, with no partial scan.
        assert_eq!(
            scan_granted(&granted, Some(&hr), Some(&sp)),
            Err(NotGranted)
        );
        granted.grant(&sp);
        let both = scan_granted(&granted, Some(&hr), Some(&sp)).unwrap();
        assert!(both.hr.is_some() && both.messaging.is_some());
        assert_eq!(scan_granted(&granted, None, None).unwrap(), Scan::default());
    }

    #[test]
    fn only_folders_the_app_handed_out_may_be_scanned() {
        let granted = Granted::default();
        granted.grant("/Users/me/agents");
        assert!(granted.allows("/Users/me/agents"));
        assert!(!granted.allows("/Users/me"));
        assert!(!granted.allows("."));
        assert!(!granted.allows("/Users/me/agents/../.ssh"));
    }

    #[test]
    fn it_reads_a_teams_file_and_says_why_when_it_cannot() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("teams.toml");
        write(
            &file,
            r#"
            # hints
            [teams.engineering]
            name = "Engineering"
            lead = "ada"
            members = ["ada"]
            [teams.app]
            name = "App"
            parent = "engineering"
            lead = "app-frontend-dev"
            members = ["app-frontend-dev", "riley"]
            "#,
        );
        let hints = read_hints(&file);
        assert_eq!(hints.error, None);
        let keys: Vec<_> = hints.teams.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys, ["app", "engineering"]);
        assert_eq!(hints.teams[0].parent.as_deref(), Some("engineering"));
        assert_eq!(hints.teams[0].members, ["app-frontend-dev", "riley"]);
        write(
            &dir.path().join("bad.toml"),
            "[teams.x
name=",
        );
        assert!(read_hints(&dir.path().join("bad.toml"))
            .error
            .unwrap()
            .contains("isn't a readable teams file"));
        assert!(read_hints(&dir.path().join("missing.toml"))
            .error
            .unwrap()
            .contains("can't be read"));
        assert!(read_hints(dir.path())
            .error
            .unwrap()
            .contains("isn't a plain file"));
        write(
            &dir.path().join("huge.toml"),
            &format!("# {}", "x".repeat(70_000)),
        );
        assert!(read_hints(&dir.path().join("huge.toml"))
            .error
            .unwrap()
            .contains("too large"));
        // A named pipe is not a teams file, and does not hang the read.
        let pipe = dir.path().join("pipe.toml");
        assert!(std::process::Command::new("mkfifo")
            .arg(&pipe)
            .status()
            .unwrap()
            .success());
        assert!(read_hints(&pipe)
            .error
            .unwrap()
            .contains("isn't a plain file"));
        // A link to a real file is fine: the owner chose it.
        std::os::unix::fs::symlink(&file, dir.path().join("link.toml")).unwrap();
        assert_eq!(read_hints(&dir.path().join("link.toml")).teams.len(), 2);
        // A file with no teams at all is a file with no hints, not an error.
        write(&dir.path().join("empty.toml"), "# nothing\n");
        assert_eq!(
            read_hints(&dir.path().join("empty.toml")),
            Hints {
                path: path_text(&dir.path().join("empty.toml")),
                error: None,
                teams: vec![]
            }
        );
    }

    #[test]
    fn a_hints_file_is_read_only_when_the_app_handed_it_out() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("teams.toml");
        write(&file, "[teams.a]\nname = \"A\"\n");
        let granted = Granted::default();
        assert_eq!(
            read_hints_granted(&granted, &path_text(&file)),
            Err(NotGranted)
        );
        granted.grant(&path_text(&file));
        assert_eq!(
            read_hints_granted(&granted, &path_text(&file))
                .unwrap()
                .teams
                .len(),
            1
        );
        assert_eq!(
            read_hints_granted(&granted, &path_text(&dir.path().join("x/../teams.toml"))),
            Err(NotGranted)
        );
    }
}
