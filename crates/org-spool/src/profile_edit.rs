//! Editing a member's profile (`profile set`, `msg register <id> --name ...`): who may edit
//! whom, and what each change means.

use crate::envelope::validate_agent_id;
use crate::member::{canonical_member_id, is_owner};
use crate::meta::{MemberProfile, RefEntry, TeamEntry};
use crate::spool::{Spool, SpoolError};

/// A member edits its own profile; only the owner may edit someone else's (Phase A has no
/// authentication, so this guards against mistakes, not against an agent passing `--as`).
pub fn authorize_profile_edit(editor: &str, target: &str) -> Result<(), SpoolError> {
    // The owner's id in any case is the owner (a case-insensitive file system gives both the
    // same inbox), so `--as VGMELLO` is the owner editing, not a way around this check.
    if canonical_member_id(editor) == canonical_member_id(target) || is_owner(editor) {
        Ok(())
    } else {
        Err(SpoolError::InvalidInput(format!(
            "{editor} can't edit {target}'s profile (only its own; the owner can edit any)"
        )))
    }
}

impl TeamEntry {
    /// `<id[:name[:title]]>`: the name defaults to the id; an empty title is none.
    pub fn parse_spec(raw: &str) -> Result<Self, String> {
        let parts: Vec<&str> = raw.splitn(3, ':').collect();
        let id = parts[0].trim();
        if id.is_empty() {
            return Err("--team requires <id[:name[:title]]>".into());
        }
        let name = parts
            .get(1)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or(id);
        let title_in_team = parts
            .get(2)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Ok(Self {
            id: id.to_string(),
            name: name.to_string(),
            title_in_team,
        })
    }
}

impl RefEntry {
    /// `<id[:name]>`: the name defaults to the id. `what` names the flag in the error.
    pub fn parse_spec(raw: &str, what: &str) -> Result<Self, String> {
        let parts: Vec<&str> = raw.splitn(2, ':').collect();
        let id = parts[0].trim();
        if id.is_empty() {
            return Err(format!("{what} requires <id[:name]>"));
        }
        let name = parts
            .get(1)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or(id);
        Ok(Self {
            id: id.to_string(),
            name: name.to_string(),
        })
    }
}

/// Changes to a profile, applied in order onto the current one (or the default) by
/// [`Spool::edit_profile`]. Fields left alone keep their value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileUpdate {
    name: Option<String>,
    kind: Option<String>,
    owner: bool,
    role_title: Option<String>,
    teams: Option<Vec<TeamEntry>>,
    reports_to: Option<RefEntry>,
    office: Option<RefEntry>,
}

impl ProfileUpdate {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn name(&mut self, raw: &str) {
        self.name = Some(raw.trim().to_string());
    }

    /// `agent`, `person`, or `owner` (a person who is the owner).
    pub fn kind(&mut self, raw: &str) -> Result<(), String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "owner" => self.owner(),
            kind @ ("agent" | "person") => self.kind = Some(kind.to_string()),
            _ => return Err("--kind must be agent, person, or owner".into()),
        }
        Ok(())
    }

    /// The owner flag (only the owner's own id passes validation), which makes it a person.
    pub fn owner(&mut self) {
        self.owner = true;
        self.kind = Some("person".to_string());
    }

    pub fn role(&mut self, raw: &str) {
        self.role_title = Some(raw.trim().to_string());
    }

    /// The first team given replaces the profile's teams; later ones are added.
    pub fn team(&mut self, spec: &str) -> Result<(), String> {
        let team = TeamEntry::parse_spec(spec)?;
        self.teams.get_or_insert_with(Vec::new).push(team);
        Ok(())
    }

    pub fn reports_to(&mut self, spec: &str) -> Result<(), String> {
        self.reports_to = Some(RefEntry::parse_spec(spec, "--reports-to")?);
        Ok(())
    }

    pub fn office(&mut self, spec: &str) -> Result<(), String> {
        self.office = Some(RefEntry::parse_spec(spec, "--office")?);
        Ok(())
    }

    fn apply_to(&self, profile: &mut MemberProfile) {
        if let Some(name) = &self.name {
            profile.name = name.clone();
        }
        if let Some(kind) = &self.kind {
            profile.kind = kind.clone();
        }
        if self.owner {
            profile.owner = true;
        }
        if let Some(role) = &self.role_title {
            profile.role_title = Some(role.clone());
        }
        if let Some(teams) = &self.teams {
            profile.teams = teams.clone();
        }
        if let Some(reports_to) = &self.reports_to {
            profile.reports_to = Some(reports_to.clone());
        }
        if let Some(office) = &self.office {
            profile.office = Some(office.clone());
        }
    }
}

impl Spool {
    /// `editor` changes `target`'s profile (registering `target` if needed): authorized by
    /// [`authorize_profile_edit`], applied onto the current profile, validated and written.
    /// Load, apply, validate and write all happen under the member's meta lock, so two edits
    /// of different fields at once both survive. A profile that can't be read (an I/O error)
    /// fails the edit and is left untouched; a malformed one is replaced (defaults plus the
    /// edit), as a repair.
    pub fn edit_profile(
        &self,
        editor: &str,
        target: &str,
        update: &ProfileUpdate,
    ) -> Result<MemberProfile, SpoolError> {
        authorize_profile_edit(editor, target)?;
        validate_agent_id(target, "profile").map_err(SpoolError::InvalidInput)?;
        let edited = |current: MemberProfile| {
            let mut profile = current;
            update.apply_to(&mut profile);
            profile
        };
        // Refuse an invalid edit before creating anything (re-checked under the lock).
        edited(self.read_profile(target)?)
            .without_variation_selectors()
            .validate()
            .map_err(SpoolError::InvalidInput)?;
        self.register(target)?;
        self.with_agent_meta_lock(target, || {
            self.store_profile_unlocked(&edited(self.read_profile(target)?))
        })
    }
}
