//! Write validation rules for organization processes and standards (`doc-34` §5, Decision 8,
//! `FC-112`, `FC-184`). Pure: no storage or network.

use crate::model::{
    strip_control_and_invisible, DocumentKind, DocumentStatus, ProcessKey, Scope, StructureDocInput,
};

/// Maximum single-line title length in Unicode scalar values.
pub const MAX_TITLE_CHARS: usize = 200;

/// Maximum Markdown body length (512 KiB, matching `process.show` cap in `doc-34` §6).
pub const MAX_BODY_BYTES: usize = 512 * 1024;

/// Authenticated caller attempting to create or update a structure document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallerKind {
    /// A human employee (`is_owner` grants organization-level `overridable: false`).
    Person {
        /// Whether this person is the organization owner.
        is_owner: bool,
    },
    /// An agent employee (`doc-34` Decision 8: may write only at its own team's project scope).
    Agent {
        /// Agent's home team ID.
        home_team_id: String,
        /// Full root-to-leaf ancestry of the agent's home team (including `home_team_id`).
        home_team_chain: Vec<String>,
    },
}

/// Inputs required to validate a `process.write` create or update.
#[derive(Clone, Debug)]
pub struct WriteCheckInput<'a> {
    /// Authenticated caller identity and team chain.
    pub caller: CallerKind,
    /// Scope where the document will be written.
    pub target_scope: Scope,
    /// Existing document ID on update (`None` on create).
    pub existing_document_id: Option<&'a str>,
    /// Optional override key.
    pub key: Option<&'a str>,
    /// `process` or `standard`.
    pub kind: DocumentKind,
    /// Document title.
    pub title: &'a str,
    /// Target lifecycle state.
    pub status: DocumentStatus,
    /// Accountable owner employee ID (`Some` required for `Current`).
    pub owner_employee_id: Option<&'a str>,
    /// Whether lower scopes may replace this document.
    pub overridable: bool,
    /// Markdown body text.
    pub body_markdown: &'a str,
    /// When `target_scope` is `Scope::Project`, the project's accountable `owner_team_id`.
    pub project_owner_team_id: Option<&'a str>,
    /// Ordered team IDs strictly above `target_scope` (for a team: its ancestors; for a project:
    /// its accountable owning team's full root-to-leaf chain).
    pub ancestor_team_chain: &'a [String],
    /// Current set of structure documents in the organization.
    pub existing_docs: &'a [StructureDocInput],
}

/// Why a process/standard write was refused (`doc-34` §5 & §6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteViolation {
    /// Field shape or lifecycle constraint failed (`invalid_request`).
    InvalidField {
        /// Field name.
        field: &'static str,
        /// Human-readable reason.
        message: String,
    },
    /// Key is already taken by another document at the same level and scope (`already_exists`).
    AlreadyExists {
        /// Duplicate key.
        key: String,
    },
    /// Attempted to replace a non-overridable document from a higher level (`invalid_request`,
    /// reason `sealed`).
    Sealed {
        /// Sealed key.
        key: String,
        /// Scope that forbids overriding this key.
        sealed_by_scope: Scope,
    },
    /// Caller lacks authority for this scope, flag, or inherited key (`not_authorized`).
    NotAuthorized {
        /// Human-readable reason.
        reason: String,
    },
}

/// Validates a create or update of an organization process or standard document (`doc-34` §5).
pub fn validate_write(input: &WriteCheckInput<'_>) -> Result<Option<ProcessKey>, WriteViolation> {
    let key = validate_fields(input)?;
    check_caller_authority(input, key.as_ref())?;
    if let Some(k) = &key {
        check_scope_uniqueness(input, k)?;
        check_ancestor_seals(input, k)?;
    }
    Ok(key)
}

fn validate_fields(input: &WriteCheckInput<'_>) -> Result<Option<ProcessKey>, WriteViolation> {
    let cleaned_title = strip_control_and_invisible(input.title);
    if cleaned_title.is_empty()
        || cleaned_title != input.title.trim()
        || cleaned_title.chars().count() > MAX_TITLE_CHARS
    {
        return Err(WriteViolation::InvalidField {
            field: "title",
            message: format!(
                "title must be a single non-empty line of at most {MAX_TITLE_CHARS} characters"
            ),
        });
    }
    if input.body_markdown.len() > MAX_BODY_BYTES {
        return Err(WriteViolation::InvalidField {
            field: "body_markdown",
            message: format!("body exceeds maximum size of {MAX_BODY_BYTES} bytes"),
        });
    }
    if input.status == DocumentStatus::Current && input.owner_employee_id.is_none() {
        return Err(WriteViolation::InvalidField {
            field: "owner_employee_id",
            message: "a current document requires an active owner_employee_id".to_owned(),
        });
    }
    let parsed_key =
        match input.key {
            None => None,
            Some(raw) if raw.trim().is_empty() => None,
            Some(raw) => Some(ProcessKey::new(raw).ok_or_else(|| {
                WriteViolation::InvalidField {
                    field: "key",
                    message:
                        "key must be 1..=64 lowercase ASCII letters, digits, or single hyphens"
                            .to_owned(),
                }
            })?),
        };
    let _ = input.kind;
    Ok(parsed_key)
}

fn check_caller_authority(
    input: &WriteCheckInput<'_>,
    key: Option<&ProcessKey>,
) -> Result<(), WriteViolation> {
    let existing = input
        .existing_document_id
        .and_then(|id| input.existing_docs.iter().find(|d| d.id == id));
    match &input.caller {
        CallerKind::Person { is_owner } => {
            if input.target_scope == Scope::Organization
                && !is_owner
                && (!input.overridable || existing.is_some_and(|e| !e.overridable))
            {
                return Err(WriteViolation::NotAuthorized {
                    reason: "only the organization owner may create, modify, unseal, or deprecate a non-overridable organization document".to_owned(),
                });
            }
            Ok(())
        }
        CallerKind::Agent {
            home_team_id,
            home_team_chain,
        } => {
            if !matches!(input.target_scope, Scope::Project(_)) {
                return Err(WriteViolation::NotAuthorized {
                    reason: "agents may only write processes or standards at project scope"
                        .to_owned(),
                });
            }
            if !input.overridable {
                return Err(WriteViolation::NotAuthorized {
                    reason: "agents cannot mark a process or standard non-overridable".to_owned(),
                });
            }
            if existing.is_some_and(|e| !e.overridable) {
                return Err(WriteViolation::NotAuthorized {
                    reason: "agents cannot modify, unseal, or deprecate a non-overridable project document".to_owned(),
                });
            }
            let Some(owner_team) = input.project_owner_team_id else {
                return Err(WriteViolation::NotAuthorized {
                    reason: "project has no accountable owning team".to_owned(),
                });
            };
            let in_owning_subtree =
                home_team_id == owner_team || home_team_chain.iter().any(|t| t == owner_team);
            if !in_owning_subtree {
                return Err(WriteViolation::NotAuthorized {
                    reason: "only agents whose home team is the project's owning team or beneath it may write project processes".to_owned(),
                });
            }
            if let Some(k) = key {
                if has_inherited_key(input, k) {
                    return Err(WriteViolation::NotAuthorized {
                        reason: format!(
                            "agents cannot replace inherited key `{}` from the organization or an ancestor team",
                            k.as_str()
                        ),
                    });
                }
            }
            Ok(())
        }
    }
}

fn check_scope_uniqueness(
    input: &WriteCheckInput<'_>,
    key: &ProcessKey,
) -> Result<(), WriteViolation> {
    let duplicate = input.existing_docs.iter().any(|doc| {
        doc.scope == input.target_scope
            && doc.key.as_ref() == Some(key)
            && Some(doc.id.as_str()) != input.existing_document_id
    });
    if duplicate {
        return Err(WriteViolation::AlreadyExists {
            key: key.as_str().to_owned(),
        });
    }
    Ok(())
}

fn is_above_scope(target: &Scope, ancestor_teams: &[String], candidate: &Scope) -> bool {
    match target {
        Scope::Organization => false,
        Scope::Team(_) | Scope::Project(_) => match candidate {
            Scope::Organization => true,
            Scope::Team(tid) => ancestor_teams.iter().any(|a| a == tid),
            Scope::Project(_) => false,
        },
    }
}

fn has_inherited_key(input: &WriteCheckInput<'_>, key: &ProcessKey) -> bool {
    input.existing_docs.iter().any(|doc| {
        !doc.is_inactive()
            && doc.key.as_ref() == Some(key)
            && is_above_scope(&input.target_scope, input.ancestor_team_chain, &doc.scope)
    })
}

fn check_ancestor_seals(
    input: &WriteCheckInput<'_>,
    key: &ProcessKey,
) -> Result<(), WriteViolation> {
    // Draft and deprecated lower documents do not activate an override, but if the document
    // is Current (or even being authored to replace a sealed key), refuse sealed replacements
    // per `doc-34` §5 ("no replacement of a non-overridable document from above").
    if input.status == DocumentStatus::Deprecated {
        return Ok(());
    }
    for doc in input.existing_docs {
        if doc.key.as_ref() != Some(key) || !doc.seals_key() {
            continue;
        }
        if is_above_scope(&input.target_scope, input.ancestor_team_chain, &doc.scope) {
            return Err(WriteViolation::Sealed {
                key: key.as_str().to_owned(),
                sealed_by_scope: doc.scope.clone(),
            });
        }
    }
    Ok(())
}
