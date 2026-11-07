//! Loading a workspace from disk, and re-reading the part a changed file feeds.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::backlog::parse_backlog;
use crate::brief::parse_project_brief;
use crate::error::{Error, Result};
use crate::model::{
    Agent, AgentProfile, Backlog, Issue, Problem, ProblemKind, Project, ProjectBrief,
    ProjectProfile, Role, StaffFunction, WorkspaceModel,
};
use crate::paths::{classify, resolve_under, FileKind};
use crate::profile::{parse_agent_profile, parse_project_profile};
use crate::role::parse_role;
use crate::staff::parse_staff_function;

/// Files larger than this aren't read (agents' files are a few KB).
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// Workspace-relative locations.
pub(crate) mod layout {
    pub const PROJECTS: &str = "projects";
    pub const ROLES: &str = "hr/roles";
    pub const HR: &str = ".claude/agents/hr.md";

    pub fn project_dir(key: &str) -> String {
        format!("projects/{key}")
    }
    pub fn project_profile(key: &str) -> String {
        format!("projects/{key}/project.json")
    }
    pub fn project_brief(key: &str) -> String {
        format!("projects/{key}/CLAUDE.md")
    }
    pub fn agents_dir(key: &str) -> String {
        format!("projects/{key}/agents")
    }
    pub fn agent_dir(key: &str, id: &str) -> String {
        format!("projects/{key}/agents/{id}")
    }
    pub fn agent_profile(key: &str, id: &str) -> String {
        format!("projects/{key}/agents/{id}/agent.json")
    }
    pub fn backlog(key: &str, id: &str) -> String {
        format!("projects/{key}/agents/{id}/tasks/BACKLOG.md")
    }
    pub fn role(id: &str) -> String {
        format!("hr/roles/{id}.toml")
    }
}

/// A value read from one file, with the problems found in it.
pub type Loaded<T> = (T, Vec<Problem>);

/// A workspace root on disk. Every read goes through [`Workspace::resolve`], so
/// nothing outside the root is ever read, even through a symlink.
#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
}

/// The outcome of reading a file's text.
enum Text {
    Read(String),
    Missing,
}

impl Workspace {
    /// Opens a workspace root (canonicalized). Fails only if it isn't a directory.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let given = root.as_ref();
        let root = fs::canonicalize(given).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::RootNotFound(given.to_path_buf()),
            _ => Error::Io {
                path: given.to_path_buf(),
                source: e,
            },
        })?;
        if !root.is_dir() {
            return Err(Error::RootNotDirectory(given.to_path_buf()));
        }
        Ok(Self { root })
    }

    /// Opens and reads a whole workspace. Fails only if the root isn't a readable
    /// directory; everything else that's wrong is in [`WorkspaceModel::problems`].
    pub fn load(root: impl AsRef<Path>) -> Result<WorkspaceModel> {
        Ok(Self::open(root)?.load_all())
    }

    /// The canonical root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolves a workspace-relative path inside the root (see [`crate::resolve_in_workspace`]).
    pub fn resolve(&self, rel: impl AsRef<Path>) -> Result<PathBuf> {
        resolve_under(&self.root, rel.as_ref())
    }

    /// Reads the whole workspace.
    pub fn load_all(&self) -> WorkspaceModel {
        let mut problems = Vec::new();

        let (roles, p) = self.load_roles();
        problems.extend(p);
        let (hr, p) = self.read_hr();
        problems.extend(p);

        let mut projects = BTreeMap::new();
        let (keys, p) = self.list_dirs(layout::PROJECTS, true);
        problems.extend(p);
        for key in keys {
            let (project, p) = self.load_project(&key);
            problems.extend(p);
            if let Some(project) = project {
                projects.insert(key, project);
            }
        }

        sort_problems(&mut problems);
        WorkspaceModel {
            root: self.root.clone(),
            projects,
            roles,
            hr,
            problems,
        }
    }

    /// Reads `projects/<key>/` with its agents. `None` if the folder doesn't exist.
    pub fn load_project(&self, key: &str) -> Loaded<Option<Project>> {
        if !(is_name(key)) {
            return (None, Vec::new());
        }
        let dir = layout::project_dir(key);
        let mut problems = Vec::new();
        match self.dir_state(&dir) {
            DirState::Missing => return (None, problems),
            DirState::Bad(issue) => {
                problems.push(issue.at(&dir));
                return (Some(empty_project(key)), problems);
            }
            DirState::Dir => {}
        }
        let (profile, p) = self.read_project_profile(key);
        problems.extend(p);
        let (brief, p) = self.read_project_brief(key);
        problems.extend(p);

        let mut agents = BTreeMap::new();
        let (ids, p) = self.list_dirs(&layout::agents_dir(key), false);
        problems.extend(p);
        for id in ids {
            let (agent, p) = self.load_agent(key, &id);
            problems.extend(p);
            if let Some(agent) = agent {
                agents.insert(id, agent);
            }
        }
        (
            Some(Project {
                profile,
                brief,
                agents,
                ..empty_project(key)
            }),
            problems,
        )
    }

    /// Reads `projects/<key>/agents/<id>/`. `None` if the folder doesn't exist.
    pub fn load_agent(&self, key: &str, id: &str) -> Loaded<Option<Agent>> {
        if !(is_name(key) && is_name(id)) {
            return (None, Vec::new());
        }
        let dir = layout::agent_dir(key, id);
        let mut problems = Vec::new();
        match self.dir_state(&dir) {
            DirState::Missing => return (None, problems),
            DirState::Bad(issue) => {
                problems.push(issue.at(&dir));
                return (
                    Some(Agent {
                        id: id.into(),
                        dir,
                        profile: None,
                        backlog: None,
                    }),
                    problems,
                );
            }
            DirState::Dir => {}
        }
        let (profile, p) = self.read_agent_profile(key, id);
        problems.extend(p);
        let (backlog, p) = self.read_backlog(key, id);
        problems.extend(p);
        (
            Some(Agent {
                id: id.into(),
                dir,
                profile,
                backlog,
            }),
            problems,
        )
    }

    /// Reads `projects/<key>/project.json` and checks it against its folder.
    pub fn read_project_profile(&self, key: &str) -> Loaded<Option<ProjectProfile>> {
        if !(is_name(key)) {
            return (None, Vec::new());
        }
        let path = layout::project_profile(key);
        let mut problems = Vec::new();
        let text = match self.read_text(&path, &mut problems) {
            Some(Text::Read(text)) => text,
            Some(Text::Missing) => {
                problems.push(
                    Issue::error(ProblemKind::Missing, "the project has no project.json").at(&path),
                );
                return (None, problems);
            }
            None => return (None, problems),
        };
        let parsed = parse_project_profile(&text);
        problems.extend(parsed.issues.into_iter().map(|i| i.at(&path)));
        if let Some(profile) = &parsed.value {
            mismatch(
                &mut problems,
                &path,
                "key",
                profile.key.as_deref(),
                key,
                "folder name",
            );
            mismatch(
                &mut problems,
                &path,
                "home",
                profile.home.as_deref(),
                &layout::project_dir(key),
                "folder",
            );
        }
        (parsed.value, problems)
    }

    /// Reads `projects/<key>/CLAUDE.md`.
    pub fn read_project_brief(&self, key: &str) -> Loaded<Option<ProjectBrief>> {
        if !(is_name(key)) {
            return (None, Vec::new());
        }
        let path = layout::project_brief(key);
        let mut problems = Vec::new();
        match self.read_text(&path, &mut problems) {
            Some(Text::Read(text)) => {
                let parsed = parse_project_brief(&text);
                problems.extend(parsed.issues.into_iter().map(|i| i.at(&path)));
                (Some(parsed.value), problems)
            }
            Some(Text::Missing) => {
                problems.push(
                    Issue::warning(ProblemKind::Missing, "the project has no CLAUDE.md brief")
                        .at(&path),
                );
                (None, problems)
            }
            None => (None, problems),
        }
    }

    /// Reads an agent's `agent.json` and checks it against its folder.
    pub fn read_agent_profile(&self, key: &str, id: &str) -> Loaded<Option<AgentProfile>> {
        if !(is_name(key) && is_name(id)) {
            return (None, Vec::new());
        }
        let path = layout::agent_profile(key, id);
        let mut problems = Vec::new();
        let text = match self.read_text(&path, &mut problems) {
            Some(Text::Read(text)) => text,
            Some(Text::Missing) => {
                problems.push(
                    Issue::error(ProblemKind::Missing, "the agent has no agent.json").at(&path),
                );
                return (None, problems);
            }
            None => return (None, problems),
        };
        let parsed = parse_agent_profile(&text);
        problems.extend(parsed.issues.into_iter().map(|i| i.at(&path)));
        if let Some(profile) = &parsed.value {
            mismatch(
                &mut problems,
                &path,
                "id",
                profile.id.as_deref(),
                id,
                "folder name",
            );
            mismatch(
                &mut problems,
                &path,
                "project",
                profile.project.as_deref(),
                key,
                "project folder",
            );
            mismatch(
                &mut problems,
                &path,
                "home",
                profile.home.as_deref(),
                &layout::agent_dir(key, id),
                "folder",
            );
        }
        (parsed.value, problems)
    }

    /// Reads an agent's `tasks/BACKLOG.md`.
    pub fn read_backlog(&self, key: &str, id: &str) -> Loaded<Option<Backlog>> {
        if !(is_name(key) && is_name(id)) {
            return (None, Vec::new());
        }
        let path = layout::backlog(key, id);
        let mut problems = Vec::new();
        match self.read_text(&path, &mut problems) {
            Some(Text::Read(text)) => {
                let parsed = parse_backlog(&text);
                problems.extend(parsed.issues.into_iter().map(|i| i.at(&path)));
                (Some(parsed.value), problems)
            }
            Some(Text::Missing) => {
                problems.push(
                    Issue::warning(ProblemKind::Missing, "the agent has no tasks/BACKLOG.md")
                        .at(&path),
                );
                (None, problems)
            }
            None => (None, problems),
        }
    }

    /// Reads the role catalog (`hr/roles/*.toml`, skipping `_*.toml` templates).
    pub fn load_roles(&self) -> Loaded<BTreeMap<String, Role>> {
        let mut problems = Vec::new();
        let mut roles = BTreeMap::new();
        let (files, p) = self.list_files(layout::ROLES);
        problems.extend(p);
        for file in files {
            let Some(id) = file.strip_suffix(".toml") else {
                continue;
            };
            if id.starts_with('_') || id.is_empty() {
                continue;
            }
            let (role, p) = self.read_role(id);
            problems.extend(p);
            if let Some(role) = role {
                roles.insert(id.to_string(), role);
            }
        }
        (roles, problems)
    }

    /// Reads `hr/roles/<id>.toml`. `None` if it's missing or isn't valid TOML.
    pub fn read_role(&self, id: &str) -> Loaded<Option<Role>> {
        if !(is_name(id)) {
            return (None, Vec::new());
        }
        let path = layout::role(id);
        let mut problems = Vec::new();
        match self.read_text(&path, &mut problems) {
            Some(Text::Read(text)) => {
                let parsed = parse_role(id, &text);
                problems.extend(parsed.issues.into_iter().map(|i| i.at(&path)));
                (parsed.value, problems)
            }
            Some(Text::Missing) | None => (None, problems),
        }
    }

    /// Reads the HR agent's definition, `.claude/agents/hr.md`.
    pub fn read_hr(&self) -> Loaded<Option<StaffFunction>> {
        let mut problems = Vec::new();
        match self.read_text(layout::HR, &mut problems) {
            Some(Text::Read(text)) => {
                let parsed = parse_staff_function(layout::HR, &text);
                problems.extend(parsed.issues.into_iter().map(|i| i.at(layout::HR)));
                (Some(parsed.value), problems)
            }
            Some(Text::Missing) => {
                problems.push(
                    Issue::warning(ProblemKind::Missing, "the workspace has no HR agent")
                        .at(layout::HR),
                );
                (None, problems)
            }
            None => (None, problems),
        }
    }

    /// Reads a file's text inside the workspace. `Some(Missing)` if it doesn't
    /// exist or is a folder (the listings skip folders too, so a refresh and a
    /// full load agree); `None` (with a problem recorded) if it can't be read.
    fn read_text(&self, rel: &str, problems: &mut Vec<Problem>) -> Option<Text> {
        if self.root.join(rel).is_dir() {
            return Some(Text::Missing);
        }
        let path = match self.resolve(rel) {
            Ok(path) => path,
            Err(Error::NotFound(_)) => return Some(Text::Missing),
            Err(err) => {
                problems.push(Issue::error(ProblemKind::Unreadable, err.to_string()).at(rel));
                return None;
            }
        };
        let unreadable = |problems: &mut Vec<Problem>, message: String| {
            problems.push(Issue::error(ProblemKind::Unreadable, message).at(rel));
        };
        // Check the type before opening: opening a FIFO for reading blocks until a
        // writer appears. (A FIFO swapped in between the two calls isn't caught.)
        match fs::metadata(&path) {
            Ok(meta) if !meta.is_file() => {
                unreadable(problems, "isn't a regular file".into());
                return None;
            }
            Ok(_) => {}
            Err(err) => {
                unreadable(problems, format!("can't read: {err}"));
                return None;
            }
        }
        let file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(err) => {
                unreadable(problems, format!("can't open: {err}"));
                return None;
            }
        };
        match file.metadata() {
            Ok(meta) if !meta.is_file() => {
                unreadable(problems, "isn't a regular file".into());
                return None;
            }
            Ok(meta) if meta.len() > MAX_FILE_BYTES => {
                unreadable(problems, format!("is larger than {MAX_FILE_BYTES} bytes"));
                return None;
            }
            Ok(_) => {}
            Err(err) => {
                unreadable(problems, format!("can't read: {err}"));
                return None;
            }
        }
        let mut bytes = Vec::new();
        if let Err(err) = file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes) {
            unreadable(problems, format!("can't read: {err}"));
            return None;
        }
        if bytes.len() as u64 > MAX_FILE_BYTES {
            unreadable(problems, format!("is larger than {MAX_FILE_BYTES} bytes"));
            return None;
        }
        Some(Text::Read(match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(err) => {
                problems.push(
                    Issue::warning(
                        ProblemKind::Unreadable,
                        "isn't valid UTF-8; invalid bytes were replaced",
                    )
                    .at(rel),
                );
                String::from_utf8_lossy(err.as_bytes()).into_owned()
            }
        }))
    }

    /// Whether a project or agent folder is there. Anything that isn't a folder
    /// (following symlinks, as the listings do) counts as missing, so a refresh
    /// of a stray file agrees with a full load, which never lists it.
    fn dir_state(&self, rel: &str) -> DirState {
        if !self.root.join(rel).is_dir() {
            return DirState::Missing;
        }
        match self.resolve(rel) {
            Ok(_) => DirState::Dir,
            Err(Error::NotFound(_)) => DirState::Missing,
            Err(err) => DirState::Bad(Issue::error(ProblemKind::Unreadable, err.to_string())),
        }
    }

    /// Names of the non-hidden entries of a workspace folder, sorted, filtered by
    /// whether they are folders. A missing folder is empty (a problem if `required`).
    fn list_entries(&self, rel: &str, want_dirs: bool, required: bool) -> Loaded<Vec<String>> {
        let mut problems = Vec::new();
        let dir = match self.resolve(rel) {
            Ok(dir) => dir,
            Err(Error::NotFound(_)) => {
                if required {
                    problems.push(
                        Issue::warning(ProblemKind::Missing, format!("no {rel}/ folder")).at(rel),
                    );
                }
                return (Vec::new(), problems);
            }
            Err(err) => {
                problems.push(Issue::error(ProblemKind::Unreadable, err.to_string()).at(rel));
                return (Vec::new(), problems);
            }
        };
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) => {
                problems.push(
                    Issue::error(ProblemKind::Unreadable, format!("can't list: {err}")).at(rel),
                );
                return (Vec::new(), problems);
            }
        };
        let mut names = Vec::new();
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                problems.push(
                    Issue::warning(
                        ProblemKind::Unreadable,
                        format!("skipped {:?}: the name isn't valid UTF-8", file_name),
                    )
                    .at(rel),
                );
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            // Follows symlinks; whether the target stays inside the root is
            // checked when the entry itself is read.
            if entry.path().is_dir() == want_dirs {
                names.push(name.to_string());
            }
        }
        names.sort();
        (names, problems)
    }

    fn list_dirs(&self, rel: &str, required: bool) -> Loaded<Vec<String>> {
        self.list_entries(rel, true, required)
    }

    fn list_files(&self, rel: &str) -> Loaded<Vec<String>> {
        self.list_entries(rel, false, true)
    }

    fn is_good_dir(&self, rel: &str) -> bool {
        matches!(self.dir_state(rel), DirState::Dir)
    }
}

enum DirState {
    Dir,
    Missing,
    Bad(Issue),
}

/// A project key, agent id or role id as one plain path segment: not empty, not
/// hidden, no separators. The `load_*`/`read_*` functions return nothing for
/// anything else, so no caller-supplied name can point elsewhere.
fn is_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\', '\0'])
}

fn empty_project(key: &str) -> Project {
    Project {
        key: key.to_string(),
        dir: layout::project_dir(key),
        profile: None,
        brief: None,
        agents: BTreeMap::new(),
    }
}

/// Records a warning when a profile field disagrees with where the file lives.
fn mismatch(
    problems: &mut Vec<Problem>,
    path: &str,
    field: &str,
    found: Option<&str>,
    expected: &str,
    what: &str,
) {
    if let Some(found) = found {
        if found != expected {
            problems.push(
                Issue::warning(
                    ProblemKind::Inconsistent,
                    format!("'{field}' is '{found}' but the {what} is '{expected}'"),
                )
                .at(path),
            );
        }
    }
}

fn sort_problems(problems: &mut [Problem]) {
    problems.sort_by(|a, b| a.path.cmp(&b.path));
}

fn is_under(path: &str, dir: &str) -> bool {
    path == dir
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

impl WorkspaceModel {
    /// Re-reads what a changed workspace path feeds (see [`classify`]) and updates
    /// the model and its problems in place. Handles created, changed and deleted
    /// files and folders. Returns what was refreshed, or `None` if the path
    /// isn't part of the model (nothing changes then).
    ///
    /// `path` is relative to [`WorkspaceModel::root`], or absolute under it. A
    /// change to a folder above many model files (the root, `projects/`, `hr/`,
    /// `.claude/`) is [`FileKind::Workspace`] and re-reads everything.
    pub fn refresh(&mut self, path: impl AsRef<Path>) -> Option<FileKind> {
        let path = path.as_ref();
        let kind = if path.is_absolute() {
            classify(self.relative_path(path)?)?
        } else {
            classify(path)?
        };
        let ws = Workspace {
            root: self.root.clone(),
        };
        match &kind {
            FileKind::Workspace => *self = ws.load_all(),
            FileKind::ProjectDir { project } => self.reload_project(&ws, project),
            FileKind::ProjectProfile { project } => {
                if self.ensure_project(&ws, project) {
                    let (profile, p) = ws.read_project_profile(project);
                    self.replace_problems(&layout::project_profile(project), p);
                    if let Some(entry) = self.projects.get_mut(project) {
                        entry.profile = profile;
                    }
                }
            }
            FileKind::ProjectBrief { project } => {
                if self.ensure_project(&ws, project) {
                    let (brief, p) = ws.read_project_brief(project);
                    self.replace_problems(&layout::project_brief(project), p);
                    if let Some(entry) = self.projects.get_mut(project) {
                        entry.brief = brief;
                    }
                }
            }
            FileKind::AgentDir { project, agent } => self.reload_agent(&ws, project, agent),
            FileKind::AgentProfile { project, agent } => {
                if self.ensure_agent(&ws, project, agent) {
                    let (profile, p) = ws.read_agent_profile(project, agent);
                    self.replace_problems(&layout::agent_profile(project, agent), p);
                    if let Some(entry) = self.agent_mut(project, agent) {
                        entry.profile = profile;
                    }
                }
            }
            FileKind::Backlog { project, agent } => {
                if self.ensure_agent(&ws, project, agent) {
                    let (backlog, p) = ws.read_backlog(project, agent);
                    self.replace_problems(&layout::backlog(project, agent), p);
                    if let Some(entry) = self.agent_mut(project, agent) {
                        entry.backlog = backlog;
                    }
                }
            }
            FileKind::RoleCatalog => {
                let (roles, p) = ws.load_roles();
                self.problems.retain(|x| !is_under(&x.path, layout::ROLES));
                self.problems.extend(p);
                self.roles = roles;
            }
            // If `hr/roles` itself isn't a usable folder, re-read the catalog as a
            // full load would, instead of reading one file under it.
            FileKind::Role { .. } if !ws.is_good_dir(layout::ROLES) => {
                let (roles, p) = ws.load_roles();
                self.problems.retain(|x| !is_under(&x.path, layout::ROLES));
                self.problems.extend(p);
                self.roles = roles;
            }
            FileKind::Role { role } => {
                let (value, p) = ws.read_role(role);
                self.replace_problems(&layout::role(role), p);
                match value {
                    Some(value) => self.roles.insert(role.clone(), value),
                    None => self.roles.remove(role),
                };
            }
            FileKind::HrAgent => {
                let (hr, p) = ws.read_hr();
                self.replace_problems(layout::HR, p);
                self.hr = hr;
            }
        }
        sort_problems(&mut self.problems);
        Some(kind)
    }

    fn replace_problems(&mut self, path: &str, new: Vec<Problem>) {
        self.problems.retain(|p| p.path != path);
        self.problems.extend(new);
    }

    fn reload_project(&mut self, ws: &Workspace, key: &str) {
        let dir = layout::project_dir(key);
        self.problems.retain(|p| !is_under(&p.path, &dir));
        let (project, p) = ws.load_project(key);
        self.problems.extend(p);
        match project {
            Some(project) => self.projects.insert(key.to_string(), project),
            None => self.projects.remove(key),
        };
    }

    fn reload_agent(&mut self, ws: &Workspace, key: &str, id: &str) {
        // A new project, or one whose folder is gone or unusable (e.g. a symlink
        // out of the workspace): re-read it whole, as a full load would.
        if !self.projects.contains_key(key) || !ws.is_good_dir(&layout::project_dir(key)) {
            return self.reload_project(ws, key);
        }
        let dir = layout::agent_dir(key, id);
        self.problems.retain(|p| !is_under(&p.path, &dir));
        let (agent, p) = ws.load_agent(key, id);
        self.problems.extend(p);
        if let Some(project) = self.projects.get_mut(key) {
            match agent {
                Some(agent) => project.agents.insert(id.to_string(), agent),
                None => project.agents.remove(id),
            };
        }
    }

    /// Makes sure the project is in the model before one of its files is
    /// re-read: loads it whole if it's new, drops it if its folder is gone.
    /// Returns whether the caller should go on re-reading the single file.
    fn ensure_project(&mut self, ws: &Workspace, key: &str) -> bool {
        if self.projects.contains_key(key) && ws.is_good_dir(&layout::project_dir(key)) {
            return true;
        }
        self.reload_project(ws, key);
        false
    }

    /// Like [`Self::ensure_project`], for an agent.
    fn ensure_agent(&mut self, ws: &Workspace, key: &str, id: &str) -> bool {
        let known = self
            .projects
            .get(key)
            .is_some_and(|p| p.agents.contains_key(id));
        if known && ws.is_good_dir(&layout::agent_dir(key, id)) {
            return true;
        }
        self.reload_agent(ws, key, id);
        false
    }

    fn agent_mut(&mut self, key: &str, id: &str) -> Option<&mut Agent> {
        self.projects.get_mut(key)?.agents.get_mut(id)
    }
}
