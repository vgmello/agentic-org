//! The kind of record a `state/` path holds.

use org_store::{DurablePath, Id};

use super::stable_id;
use super::EmployeeKind;

// ---- which record a path holds ----

/// The record a path of this family holds, with what the path itself says.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    Organization,
    Repositories,
    Office(Id),
    OfficeCodes,
    Role(Id),
    /// The teams from the top-level one down to this one.
    Team(Vec<Id>),
    /// The home team's chain, the folder's kind and the employee.
    Employee(Vec<Id>, EmployeeKind, Id),
    /// An agent's `execution.md` under its home team chain.
    AgentExecution(Vec<Id>, Id),
    /// A hire request under its target team chain (`hires/<id>.yaml`).
    HireRequest(Vec<Id>, Id),
    /// A provider connection (`provider-connections/<id>.yaml`).
    ProviderConnection(Id),
    /// A model profile (`model-profiles/<id>.yaml`).
    ModelProfile(Id),
    /// Organization-wide channel watching model profile setting (`execution/watching-model.yaml`).
    WatchingModel,
    /// The scope the path names.
    TeamCounter(Id),
    TeamAllocation(Id, u64),
    /// The public CA escrow package.
    CaEscrow,
}

/// A number as a path writes it: decimal, no sign, no leading zero, at least 1.
pub(crate) fn path_number(text: &str) -> Option<u64> {
    let canonical = !text.starts_with('0') && text.bytes().all(|b| b.is_ascii_digit());
    text.parse().ok().filter(|_| canonical)
}

/// Which record `path` holds; `None` when the path is not this family's. Decided by the
/// shape of the path alone, so every path has at most one owner.
pub fn kind_of(path: &DurablePath) -> Option<Kind> {
    let parts: Vec<&str> = path.components().collect();
    let id = |text: &str| stable_id(text);
    let file = |name: &str| name.strip_suffix(".yaml").and_then(stable_id);
    match parts.as_slice() {
        ["state", "organization.yaml"] => Some(Kind::Organization),
        ["state", "repositories.yaml"] => Some(Kind::Repositories),
        ["state", "offices", name] => file(name).map(Kind::Office),
        ["state", "identity", "office-codes.yaml"] => Some(Kind::OfficeCodes),
        ["state", "roles", name] => name.strip_suffix(".md").and_then(stable_id).map(Kind::Role),
        ["state", "provider-connections", name] => file(name).map(Kind::ProviderConnection),
        ["state", "model-profiles", name] => file(name).map(Kind::ModelProfile),
        ["state", "execution", "watching-model.yaml"] => Some(Kind::WatchingModel),
        ["state", "security", "ca-escrow.v1.bin"] => Some(Kind::CaEscrow),
        ["state", "identity", "counters", "organization", scope, "team.yaml"] => {
            id(scope).map(Kind::TeamCounter)
        }
        ["state", "identity", "allocations", "organization", scope, "team", name] => {
            let number = name.strip_suffix(".yaml").and_then(path_number)?;
            Some(Kind::TeamAllocation(id(scope)?, number))
        }
        ["state", teams @ ..] => {
            let mut chain = Vec::new();
            let mut rest = teams;
            while let ["teams", team, below @ ..] = rest {
                chain.push(id(team)?);
                rest = below;
            }
            let kind = |folder: &str| {
                let kinds = [EmployeeKind::Person, EmployeeKind::Agent];
                kinds.into_iter().find(|kind| kind.folder() == folder)
            };
            match rest {
                _ if chain.is_empty() => None,
                ["team.yaml"] => Some(Kind::Team(chain)),
                [folder, employee, "profile.md"] => {
                    Some(Kind::Employee(chain, kind(folder)?, id(employee)?))
                }
                ["agents", employee, "execution.md"] => {
                    Some(Kind::AgentExecution(chain, id(employee)?))
                }
                ["hires", name] => Some(Kind::HireRequest(chain, file(name)?)),
                _ => None,
            }
        }
        _ => None,
    }
}
