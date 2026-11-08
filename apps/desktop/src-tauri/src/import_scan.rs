//! Reading the places an existing team lives in, for "Bring in an existing team". Read-only: nothing here writes, and
//! nothing runs. Two sources:
//!
//! - the HR workspace: `projects/*/agents/*/agent.json` (one record per agent) and `hr/roles/*.toml` (the role files);
//! - the messaging spool: the directory names under `inbox/`, one per registered name.
//!
//! The folders are the owner's choice (they are never fixed in the app); `defaults` only looks in the usual places so the
//! first screen can be prefilled. Files are read by fixed names and size-limited, directories are not followed through
//! symbolic links, and a record that can't be read is reported on its own without hiding the others.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// The most agents and names one scan returns, and the largest file it reads.
const MAX_ENTRIES: usize = 1_000;
const MAX_FILE_BYTES: u64 = 64 * 1024;

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
    /// Records that were skipped, each with why.
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MessagingScan {
    pub path: String,
    pub error: Option<String>,
    pub names: Vec<String>,
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
    let messaging = spool.join("inbox").is_dir().then(|| path_text(&spool));
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

fn is_hr_workspace(dir: &Path) -> bool {
    dir.join("hr").join("roles").is_dir() && dir.join("projects").is_dir()
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
    out.roles = names_in(&root.join("hr").join("roles"), |name| {
        name.strip_suffix(".toml")
            .filter(|stem| !stem.starts_with('_'))
            .map(str::to_string)
    });
    for project in sorted_dirs(&root.join("projects")) {
        for agent_dir in sorted_dirs(&project.join("agents")) {
            if out.agents.len() >= MAX_ENTRIES {
                out.skipped.push("More agents than this scan reads.".into());
                return out;
            }
            let record = agent_dir.join("agent.json");
            match read_agent(&record, root) {
                Ok(Some(agent)) => out.agents.push(agent),
                Ok(None) => {}
                Err(why) => out.skipped.push(format!("{}: {why}", short(root, &record))),
            }
        }
    }
    out
}

/// `None`: no record in this folder (not an agent's home).
fn read_agent(record: &Path, root: &Path) -> Result<Option<HrAgent>, String> {
    let Ok(meta) = fs::symlink_metadata(record) else {
        return Ok(None);
    };
    if !meta.is_file() {
        return Err("not a plain file".into());
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err("too large to be an agent record".into());
    }
    let text = fs::read_to_string(record).map_err(|e| format!("can't be read ({e})"))?;
    let mut agent: HrAgent =
        serde_json::from_str(&text).map_err(|_| "isn't a readable agent record".to_string())?;
    if agent.id.trim().is_empty() {
        return Err("has no id".into());
    }
    agent.home_path = agent.home.as_deref().and_then(|home| resolve(root, home));
    Ok(Some(agent))
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
    if !inbox.is_dir() {
        out.error = Some("That folder has no inbox: it isn't a messaging spool.".into());
        return out;
    }
    out.names = sorted_dirs(&inbox)
        .into_iter()
        .filter_map(|dir| dir.file_name().map(|n| n.to_string_lossy().into_owned()))
        .filter(|name| !name.starts_with('.'))
        .take(MAX_ENTRIES)
        .collect();
    out
}

/// The sub-directories of `dir` that are real directories (a symbolic link is not followed), by name.
fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .map(|entry| entry.path())
        .collect();
    dirs.sort();
    dirs
}

fn names_in(dir: &Path, pick: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| pick(&entry.file_name().to_string_lossy()))
        .collect();
    names.sort();
    names
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
}
