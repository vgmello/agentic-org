//! The checks every process write runs (processes.md §5): [`check_write`] answers a `doc.new` or
//! `doc.save` of kind process, or a `process.save`, with the file to write or the first refusal.
//! Pure: the office gathers the facts (the files at the base, the branch, the caller) and acts on
//! the answer.

use crate::file::{ProcessFile, Status};
use crate::place::{location, MemberId, Place, ProcessKey, ProjectKey, Repo};
use crate::resolve::{Input, Inputs};

/// Who is writing. The owner is never an agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Writer {
    /// The organization's owner.
    Owner(MemberId),
    /// An agent.
    Agent(MemberId),
    /// A human member who isn't the owner.
    Member(MemberId),
}

impl Writer {
    /// The caller's member id.
    pub fn member(&self) -> &MemberId {
        match self {
            Writer::Owner(id) | Writer::Agent(id) | Writer::Member(id) => id,
        }
    }
}

/// The branch a write names (`branch?`), as the office knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    /// The office made it (an office-made `docs/…` branch).
    pub office_made: bool,
    /// It is merged.
    pub merged: bool,
    /// The project whose repository it is in; none for the handbook.
    pub project: Option<ProjectKey>,
    /// The path of the file it is for, in that repository.
    pub path: String,
    /// The member who opened it.
    pub opened_by: MemberId,
}

/// One write, as plain data.
#[derive(Clone, Debug)]
pub struct WriteInput<'a> {
    /// The repository written.
    pub repo: Repo<'a>,
    /// The file's path in it (relative, `/`-separated).
    pub path: String,
    /// The new text.
    pub text: String,
    /// Who is writing.
    pub caller: Writer,
    /// The resolution inputs at the base, with every file at the written place and the
    /// organization's.
    pub base: Inputs,
    /// This file's version at the base; none for a new file. A file at this path in
    /// [`WriteInput::base`] whose version can be read must agree with it.
    pub base_version: Option<u32>,
    /// The branch named by `branch?`, if any.
    pub branch: Option<Branch>,
}

/// Why a write is refused: org-proto's `ApiError` code, reason and field, and a message. Built
/// only by [`check_write`]:
///
/// ```compile_fail,E0451
/// use org_process::{RefusalCode, WriteRefusal};
/// let made = WriteRefusal {
///     code: RefusalCode::NotAuthorized,
///     reason: None,
///     field: None,
///     message: String::new(),
/// };
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteRefusal {
    code: RefusalCode,
    reason: Option<RefusalReason>,
    field: Option<String>,
    message: String,
}

/// The `ApiError` codes a write can be refused with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusalCode {
    /// `invalid_request`.
    InvalidRequest,
    /// `already_exists`.
    AlreadyExists,
    /// `not_authorized`.
    NotAuthorized,
}

/// The `ApiError` reasons a refusal can carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusalReason {
    /// `private_only_adds`.
    PrivateOnlyAdds,
}

impl RefusalCode {
    /// The code as `ApiError` spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            RefusalCode::InvalidRequest => "invalid_request",
            RefusalCode::AlreadyExists => "already_exists",
            RefusalCode::NotAuthorized => "not_authorized",
        }
    }
}

impl RefusalReason {
    /// The reason as `ApiError.reason` spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            RefusalReason::PrivateOnlyAdds => "private_only_adds",
        }
    }
}

impl WriteRefusal {
    /// The code.
    pub fn code(&self) -> RefusalCode {
        self.code
    }

    /// The reason, if the spec gives one.
    pub fn reason(&self) -> Option<RefusalReason> {
        self.reason
    }

    /// The field, if the spec names one.
    pub fn field(&self) -> Option<&str> {
        self.field.as_deref()
    }

    /// What was wrong, in words.
    pub fn message(&self) -> &str {
        &self.message
    }
}

fn refuse(code: RefusalCode, message: String) -> WriteRefusal {
    WriteRefusal {
        code,
        reason: None,
        field: None,
        message,
    }
}

fn private_only_adds(message: String) -> WriteRefusal {
    WriteRefusal {
        reason: Some(RefusalReason::PrivateOnlyAdds),
        ..refuse(RefusalCode::InvalidRequest, message)
    }
}

/// The key `input` was read with, if it could be read.
fn key_of(input: &Input) -> Option<&ProcessKey> {
    match &input.read {
        Ok(file) => Some(&file.key),
        Err(problem) => problem.key.as_ref(),
    }
}

/// The checks of §5, in a fixed order, so the first problem found is the one reported: the
/// location, the front matter, the own private folder, the organization seal, private only
/// adds, the unique key, (sealed keys: not yet checked), the version and the branch. Returns the
/// file as read.
pub fn check_write(input: &WriteInput<'_>) -> Result<ProcessFile, WriteRefusal> {
    use RefusalCode::{AlreadyExists, InvalidRequest, NotAuthorized};
    // 1. Location. A refused file name is refused by `Location::read`, as a `file name` problem
    // that comes before any front-matter problem.
    let at = location(input.repo, &input.path).ok_or_else(|| {
        let path = &input.path;
        refuse(
            InvalidRequest,
            format!("{path} isn't a process file's place"),
        )
    })?;
    // 2. Front matter, matching the place.
    let file = at.read(&input.text).map_err(|problem| WriteRefusal {
        field: Some(problem.field.clone()).filter(|f| !f.is_empty()),
        ..refuse(
            InvalidRequest,
            format!("details not recognized ({})", problem.field),
        )
    })?;
    // 3. Own private folder: only its agent writes it, so neither the owner nor a human member
    // does (read fail-closed).
    if matches!(&file.place, Place::Agent(folder) if input.caller != Writer::Agent(folder.clone()))
    {
        let message = "only its agent writes a private folder".to_owned();
        return Err(refuse(NotAuthorized, message));
    }
    // 4. Organization seal (§5, ruling (a)): the new text has `overridable: false` in any
    // status, or the file at this path seals at the base, or can't be shown not to (rule 2).
    let owner = matches!(input.caller, Writer::Owner(_));
    let sealed_at_base = input
        .base
        .files
        .iter()
        .any(|i| i.path == input.path && i.seals());
    if file.place == Place::Organization && (!file.overridable || sealed_at_base) && !owner {
        let message = "only the owner seals or edits a sealed organization process".to_owned();
        return Err(refuse(NotAuthorized, message));
    }
    // 5. Private only adds. Organization keys only (team and project keys are resolution's), in
    // any status or validity; an organization file whose key can't be read might have this key.
    if matches!(file.place, Place::Agent(_)) {
        if file.overrides.is_some() {
            return Err(private_only_adds(
                "a private file sets no `overrides`".into(),
            ));
        }
        if file.status == Status::Off {
            return Err(private_only_adds("a private file isn't `off`".into()));
        }
        let org = input
            .base
            .files
            .iter()
            .filter(|i| i.place == Place::Organization);
        for other in org {
            let path = &other.path;
            match key_of(other) {
                Some(key) if *key == file.key => {
                    return Err(private_only_adds(format!("{path} uses the key")));
                }
                None => {
                    let message = format!("can't tell: {path} can't be read");
                    return Err(private_only_adds(message));
                }
                Some(_) => {}
            }
        }
    }
    // 6. Unique key at the place, among the other files there in any status or validity; one
    // whose key can't be read doesn't block.
    let twin = |other: &&Input| {
        other.place == file.place && other.path != input.path && key_of(other) == Some(&file.key)
    };
    if let Some(other) = input.base.files.iter().find(twin) {
        let message = format!("{} already has the key", other.path);
        return Err(refuse(AlreadyExists, message));
    }
    // 7. Sealed keys: not checked yet.
    // 8. Version: `base_version` agrees with a valid file at this path in the base inputs (one
    // whose version can't be read isn't compared), and the new one is the base's + 1, or 1 for a
    // new file.
    let at_base = input.base.files.iter().find_map(|i| match &i.read {
        Ok(base) if i.path == input.path => Some(base.version),
        _ => None,
    });
    if at_base.is_some_and(|version| input.base_version != Some(version)) {
        return Err(WriteRefusal {
            field: Some("version".into()),
            ..refuse(
                InvalidRequest,
                "the base version isn't the base file's".into(),
            )
        });
    }
    let next = input
        .base_version
        .map_or(Some(1), |base| base.checked_add(1));
    if next != Some(file.version) {
        return Err(WriteRefusal {
            field: Some("version".into()),
            ..refuse(
                InvalidRequest,
                match next {
                    Some(next) => format!("the version must be {next}"),
                    None => "no version follows the base's".to_owned(),
                },
            )
        });
    }
    // 9. Branch: office-made, unmerged and for this file in this repository, else
    // `invalid_request`; then opened by the caller, else `not_authorized`.
    if let Some(branch) = &input.branch {
        let repo = match input.repo {
            Repo::Handbook => None,
            Repo::Project(key) => Some(key),
        };
        let elsewhere = branch.project.as_ref() != repo || branch.path != input.path;
        if !branch.office_made || branch.merged || elsewhere {
            let message = "the branch isn't an open office-made branch for this file".to_owned();
            return Err(refuse(InvalidRequest, message));
        }
        if branch.opened_by != *input.caller.member() {
            let message = "another member opened the branch".to_owned();
            return Err(refuse(NotAuthorized, message));
        }
    }
    Ok(file)
}

#[cfg(test)]
mod tests;
