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

/// The place of the file at `path` (relative, `/`-separated) in `repo`, if it is a process file:
/// a `*.md` (not hidden) directly in one of §2.2's folders, never nested.
pub fn location(repo: Repo<'_>, path: &str) -> Option<Place> {
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
    match (repo, folders) {
        (Repo::Handbook, ["processes"]) => Some(Place::Organization),
        (Repo::Handbook, ["teams", id, "processes"]) => TeamId::new(id).map(Place::Team),
        (Repo::Handbook, ["agents", id, "processes"]) => MemberId::new(id).map(Place::Agent),
        (Repo::Project(key), ["docs", "processes"]) => Some(Place::Project(key.clone())),
        _ => None,
    }
}
