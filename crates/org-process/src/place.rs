//! Where a process file lives decides its level and scope (`docs/design/processes.md` §2.2).

/// The four levels, in inheritance order: an organization's processes first, an agent's last.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Level {
    /// The handbook's `processes/`.
    Organization,
    /// The handbook's `teams/<team id>/processes/`.
    Team,
    /// The project repository's `docs/processes/`.
    Project,
    /// The handbook's `agents/<member id>/processes/` (private processes).
    Agent,
}

/// A process's stable key, the same at every level: 1-64 of `[a-z0-9-]`, starting with a
/// letter or digit.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProcessKey(String);

/// A team id: 1-64 ASCII letters, digits, `-` or `_` (org-proto's client-made ids).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TeamId(String);

/// A member id: exactly 26 ASCII letters and digits (org-proto's member ids).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MemberId(String);

/// A project key: 2-10 lowercase ASCII letters and digits, starting with a letter (org-proto's).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectKey(String);

fn lower_or_digit(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit()
}

fn checked(value: &str, valid: bool) -> Option<String> {
    valid.then(|| value.to_owned())
}

impl ProcessKey {
    /// The key, if it has the required shape.
    pub fn new(value: &str) -> Option<Self> {
        let first = value.bytes().next().is_some_and(lower_or_digit);
        let rest = value.bytes().all(|b| lower_or_digit(b) || b == b'-');
        checked(value, value.len() <= 64 && first && rest).map(Self)
    }

    /// The key as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TeamId {
    /// The id, if it has the required shape.
    pub fn new(value: &str) -> Option<Self> {
        let chars = value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        checked(value, (1..=64).contains(&value.len()) && chars).map(Self)
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl MemberId {
    /// The id, if it has the required shape.
    pub fn new(value: &str) -> Option<Self> {
        let chars = value.bytes().all(|b| b.is_ascii_alphanumeric());
        checked(value, value.len() == 26 && chars).map(Self)
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl ProjectKey {
    /// The key, if it has the required shape.
    pub fn new(value: &str) -> Option<Self> {
        let first = value.bytes().next().is_some_and(|b| b.is_ascii_lowercase());
        let chars = value.bytes().all(lower_or_digit);
        checked(value, (2..=10).contains(&value.len()) && first && chars).map(Self)
    }

    /// The key as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A level with its scope.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Place {
    /// The organization's processes.
    Organization,
    /// A team's.
    Team(TeamId),
    /// A project's.
    Project(ProjectKey),
    /// An agent's private ones.
    Agent(MemberId),
}

impl Level {
    /// The level as written in front matter.
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Organization => "organization",
            Level::Team => "team",
            Level::Project => "project",
            Level::Agent => "agent",
        }
    }

    /// The level named `text`, if any.
    pub fn parse(text: &str) -> Option<Level> {
        [
            Level::Organization,
            Level::Team,
            Level::Project,
            Level::Agent,
        ]
        .into_iter()
        .find(|level| level.as_str() == text)
    }
}

impl Place {
    /// The level, without its scope.
    pub fn level(&self) -> Level {
        match self {
            Place::Organization => Level::Organization,
            Place::Team(_) => Level::Team,
            Place::Project(_) => Level::Project,
            Place::Agent(_) => Level::Agent,
        }
    }

    /// The scope as written in front matter (`None` for the organization).
    pub fn scope(&self) -> Option<&str> {
        match self {
            Place::Organization => None,
            Place::Team(id) => Some(id.as_str()),
            Place::Project(key) => Some(key.as_str()),
            Place::Agent(id) => Some(id.as_str()),
        }
    }
}

/// The repository a path is in: the organization's handbook, or a project's repository.
#[derive(Clone, Copy, Debug)]
pub enum Repo<'a> {
    /// The handbook (organization, team and agent processes).
    Handbook,
    /// The repository of this project (its processes).
    Project(&'a ProjectKey),
}

/// Where a process file is: its place, and whether its file name is one a process may not have.
/// Built only by [`location`], from the file's path, so only a project file's name can be
/// refused:
///
/// ```compile_fail,E0451
/// use org_process::{Location, Place};
/// let made = Location { place: Place::Organization, name_refused: true };
/// ```
///
/// Read it with [`Location::read`], which turns a refused name into a problem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    place: Place,
    name_refused: bool,
}

impl Location {
    /// The level and scope the folder gives.
    pub fn place(&self) -> &Place {
        &self.place
    }

    /// The name is refused (`plain_name`): the file is listed with a "file name" problem and left
    /// out of every set (processes.md §2.2, §3.2 rule 1; PROC-16). Only a project repository's
    /// file can be one.
    pub fn name_refused(&self) -> bool {
        self.name_refused
    }
}

/// Where the file at `path` (relative, `/`-separated) in `repo` is, if it is a process file: a
/// `*.md` (not hidden) directly in one of §2.2's folders, never nested. In a project repository
/// a file whose name is refused still is one, so it can't vanish from the view; in the handbook,
/// which only the tool writes, it isn't.
pub fn location(repo: Repo<'_>, path: &str) -> Option<Location> {
    let parts: Vec<&str> = path.split('/').collect();
    let (file, folders) = parts.split_last()?;
    let markdown = file
        .strip_suffix(".md")
        .is_some_and(|stem| !stem.is_empty())
        && !file.starts_with('.');
    // The folders must match exactly and ids have strict shapes, so `..`, `.` and empty
    // segments can never select a place.
    if !markdown {
        return None;
    }
    let place = match (repo, folders) {
        (Repo::Handbook, ["processes"]) => Some(Place::Organization),
        (Repo::Handbook, ["teams", id, "processes"]) => TeamId::new(id).map(Place::Team),
        (Repo::Handbook, ["agents", id, "processes"]) => MemberId::new(id).map(Place::Agent),
        (Repo::Project(key), ["docs", "processes"]) => Some(Place::Project(key.clone())),
        _ => None,
    }?;
    let name_refused = !plain_name(file);
    match repo {
        Repo::Handbook if name_refused => None,
        _ => Some(Location {
            place,
            name_refused,
        }),
    }
}

/// The same set as org-office's status lines (organization/status.rs): line breaks, the bidi
/// and invisible format characters, the fillers that render as nothing, and the tag block and
/// variation selectors supplement, which can hide text from a reader but not from an agent.
/// The zero-width (non-)joiners and the direction marks stay: emoji and many scripts need them.
pub(crate) fn is_invisible_or_break(c: char) -> bool {
    matches!(
        c,
        '\u{2028}'
            | '\u{2029}'
            | '\u{200b}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}'
            | '\u{180e}'
            | '\u{fff9}'..='\u{fffb}'
            | '\u{115f}'
            | '\u{1160}'
            | '\u{3164}'
            | '\u{ffa0}'
            | '\u{e0000}'..='\u{e007f}'
            | '\u{e0100}'..='\u{e01ef}'
    )
}

/// A file name that can name a process: none that starts with `-`, and none with whitespace,
/// control or invisible characters, or a backslash. Unlike W7's ASCII-only rule for paths the
/// tool writes (tasks-projects-docs.md §4.9), non-ASCII letters are allowed here, since the
/// front-matter key, not the file name, matches a process (processes.md §2.2).
fn plain_name(name: &str) -> bool {
    let odd =
        |c: char| c.is_whitespace() || c.is_control() || c == '\\' || is_invisible_or_break(c);
    !name.starts_with('-') && !name.contains(odd)
}
