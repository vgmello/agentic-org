//! `agentic-org org create` and `agentic-org org list` (`TASK-113`): native `host.sock`
//! organization creation and listing (`org.create`, `org.lookup`, `org.list`).
//!
//! Authentication selection:
//! - When `--login` is given, signs in via `account.verify` after verifying the host binary.
//! - When `--login` is omitted, tries `session.local` (succeeds immediately on local authless
//!   HTTP instances) and falls back to browser-approved CLI sign-in (`cli.approval.*`,
//!   printing the approval link to stderr so `--json` stdout stays clean).

use std::io::Write;
use std::time::{Duration, Instant};

use org_host_control::ops::{
    OrgCreate, OrgCreateParams, OrgList, OrgListParams, OrgOperationLookup,
    OrgOperationLookupParams,
};
use org_host_control::organization::{ListResult, OrganizationView};
use org_host_control::{HostOpResult, SessionHandle};
use org_proto::selected_bootstrap::PlanV1;
use org_proto::web::organizations::{RepositoryRegistration, SelectedCreateParams};
use org_proto::ErrorCode;
use serde::ser::{Serialize, SerializeStruct, Serializer};

use super::cli_session::{connect_cli_session, end_session, AskPassword, Target};
use super::wire::registered;
use crate::cli_error::{refused_by, wire_name, HostCliError};

/// Help text for `agentic-org org`.
pub const HELP: &str = "agentic-org org <subcommand>\n\
Use when: creating an organization and its HQ office on this host, or listing visible organizations.\n\n\
Subcommands:\n  \
create <name> --slug <slug> --repo <https://...|ssh://...> [--branch <state_branch>] [--login <login>] [--name <instance>] [--no-wait] [--json]\n      \
Create a new organization and its HQ office on this host (default branch: main)\n      \
Use when: bootstrapping a new organization on an HQ instance from a remote Git repository.\n  \
list [--login <login>] [--name <instance>] [--json]\n      \
List organizations visible to the account and any creates in flight\n      \
Use when: looking up org/HQ IDs or checking the status of an in-flight organization create.\n\n\
When --login is omitted, uses the local authless session if authentication is disabled,\n\
or prints a browser approval link to stderr when authentication is enabled.";

/// Parameters for `agentic-org org create`.
#[derive(Debug, Clone)]
pub struct OrgCreateInput<'a> {
    pub name: &'a str,
    pub slug: &'a str,
    pub repository_remote_url: &'a str,
    pub state_branch: &'a str,
    pub no_wait: bool,
}

/// What `agentic-org org create` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrgCreateOutcome {
    /// The organization and its HQ office exist. Serializes as the [`OrganizationView`].
    Created(OrganizationView),
    /// The host accepted the create and is still making it (`--no-wait`, or the wait ran
    /// out); `org list` shows it under `creating`. Serializes as
    /// `{"operation_id": .., "status": "accepted"}`.
    Accepted { operation_id: String },
}

impl Serialize for OrgCreateOutcome {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Created(view) => view.serialize(serializer),
            // The keys in the order the command's earlier `json!` map wrote them.
            Self::Accepted { operation_id } => {
                let mut accepted = serializer.serialize_struct("OrgCreateAccepted", 2)?;
                accepted.serialize_field("operation_id", operation_id)?;
                accepted.serialize_field("status", "accepted")?;
                accepted.end()
            }
        }
    }
}

const CREATE_POLL_INTERVAL: Duration = Duration::from_millis(150);
const CREATE_WAIT_TIMEOUT: Duration = Duration::from_secs(60);

/// Validates `input` against the shared selected-create rules (`PlanV1`) before contacting
/// the host. A refusal is `Usage`, naming the field and the rule in their wire spelling.
pub fn validate_create_plan(input: &OrgCreateInput<'_>) -> Result<PlanV1, HostCliError> {
    let params = SelectedCreateParams {
        name: input.name.to_owned(),
        slug: input.slug.to_owned(),
        organization_repository: RepositoryRegistration {
            kind: "remote".to_owned(),
            remote_url: Some(input.repository_remote_url.to_owned()),
            path: None,
            credential_ref: None,
        },
        state_branch: input.state_branch.to_owned(),
    };
    params.plan().map_err(|error| {
        let field = error
            .field
            .map(|f| wire_name(&f))
            .unwrap_or_else(|| "request".to_owned());
        let rule = error
            .rule
            .map(|r| wire_name(&r))
            .unwrap_or_else(|| "invalid".to_owned());
        HostCliError::usage(format!(
            "invalid organization plan ({field}: {rule}): {}",
            error.message
        ))
    })
}

/// `agentic-org org create` for registered instance `instance_name`.
pub fn org_create(
    instance_name: Option<&str>,
    login: Option<&str>,
    input: &OrgCreateInput<'_>,
) -> Result<OrgCreateOutcome, HostCliError> {
    let plan = validate_create_plan(input)?;
    let (_, id, socket) = registered(&crate::human::paths(instance_name)?, instance_name)?;
    let mut err_out = std::io::stderr().lock();
    create_with_plan(
        &Target {
            socket: &socket,
            instance_id: &id,
            name: instance_name,
        },
        login,
        plan,
        input.no_wait,
        &mut crate::human::password,
        &mut err_out,
    )
}

/// `agentic-org org list` for registered instance `instance_name`: the organizations the
/// account can open and the creates it started that have not completed.
pub fn org_list(
    instance_name: Option<&str>,
    login: Option<&str>,
) -> Result<ListResult, HostCliError> {
    let (_, id, socket) = registered(&crate::human::paths(instance_name)?, instance_name)?;
    let mut err_out = std::io::stderr().lock();
    list(
        &Target {
            socket: &socket,
            instance_id: &id,
            name: instance_name,
        },
        login,
        &mut crate::human::password,
        &mut err_out,
    )
}

pub(crate) fn create_with_plan(
    target: &Target<'_>,
    login: Option<&str>,
    plan: PlanV1,
    no_wait: bool,
    password: AskPassword<'_>,
    prompt_out: &mut dyn Write,
) -> Result<OrgCreateOutcome, HostCliError> {
    let operation_id = crate::ids::uuid_v7().map_err(|_| "no operation ID could be generated")?;
    let (mut wire, session) = connect_cli_session(target, login, password, prompt_out)?;
    let handle = || SessionHandle::new(session.expose().into()).expect("a live handle");
    let initial = wire.call_answer::<OrgCreate>(
        &OrgCreateParams {
            session: handle(),
            operation_id: operation_id.clone(),
            plan,
        },
        None,
    );
    let outcome = match initial {
        Ok(Ok(HostOpResult::Accepted)) if !no_wait => {
            poll_org_create(&mut wire, &session, &operation_id)
        }
        other => other,
    };
    end_session(&mut wire, &session);

    match outcome? {
        Ok(HostOpResult::Completed { result }) => Ok(OrgCreateOutcome::Created(result)),
        Ok(HostOpResult::Accepted) => Ok(OrgCreateOutcome::Accepted { operation_id }),
        Ok(HostOpResult::Rejected { refusal }) => Err(HostCliError::refused(
            refusal.code,
            format!(
                "the host refused organization creation: {}",
                refusal.message
            ),
        )),
        Ok(_) => Err("the service's answer is not one this command knows".into()),
        Err(code @ ErrorCode::AlreadyExists) => Err(HostCliError::refused(
            code,
            "an organization with that name or slug already exists on this host",
        )),
        Err(code @ ErrorCode::NotAuthorized) => Err(HostCliError::refused(
            code,
            "only the instance administrator may create an organization",
        )),
        Err(code @ ErrorCode::InvalidRequest) => Err(HostCliError::refused(
            code,
            "the host rejected the organization plan parameters",
        )),
        Err(code) => Err(refused_by("organization creation", code)),
    }
}

fn poll_org_create(
    wire: &mut crate::human::Wire,
    session: &SessionHandle,
    operation_id: &str,
) -> Result<Result<HostOpResult<OrganizationView>, ErrorCode>, String> {
    let deadline = Instant::now() + CREATE_WAIT_TIMEOUT;
    loop {
        let handle = SessionHandle::new(session.expose().into()).expect("a live handle");
        let answer = wire.call_answer::<OrgOperationLookup>(
            &OrgOperationLookupParams {
                session: handle,
                operation_id: operation_id.to_owned(),
            },
            None,
        )?;
        match answer {
            Ok(HostOpResult::Accepted) => {
                if Instant::now() >= deadline {
                    return Ok(Ok(HostOpResult::Accepted));
                }
                std::thread::sleep(CREATE_POLL_INTERVAL);
            }
            other => return Ok(other),
        }
    }
}

pub(crate) fn list(
    target: &Target<'_>,
    login: Option<&str>,
    password: AskPassword<'_>,
    prompt_out: &mut dyn Write,
) -> Result<ListResult, HostCliError> {
    let (mut wire, session) = connect_cli_session(target, login, password, prompt_out)?;
    let handle = SessionHandle::new(session.expose().into()).expect("a live handle");
    let answer = wire.call_answer::<OrgList>(&OrgListParams { session: handle }, None);
    end_session(&mut wire, &session);
    match answer? {
        Ok(listed) => Ok(listed),
        Err(code @ ErrorCode::NotAuthorized) => Err(HostCliError::refused(
            code,
            "not authorized to list organizations",
        )),
        Err(code) => Err(refused_by("listing organizations", code)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_create_plan_accepts_remote_url_and_rejects_local_path() {
        let valid = validate_create_plan(&OrgCreateInput {
            name: "Acme Corp",
            slug: "acme-corp",
            repository_remote_url: "https://example.test/acme/org.git",
            state_branch: "main",
            no_wait: false,
        })
        .unwrap();
        assert_eq!(valid.slug, "acme-corp");
        assert_eq!(valid.state_branch, "main");

        let bad_repo = validate_create_plan(&OrgCreateInput {
            name: "Acme Corp",
            slug: "acme-corp",
            repository_remote_url: "file:///tmp/acme",
            state_branch: "main",
            no_wait: false,
        })
        .unwrap_err();
        assert_eq!(bad_repo.kind, super::super::HostCliErrorKind::Usage);
        assert!(
            bad_repo.message.contains("invalid organization plan"),
            "{bad_repo}"
        );
        // Field and rule in their wire spelling, never `Debug` text.
        assert!(
            bad_repo.message.contains("(organization_repository: ")
                && !bad_repo.message.contains("OrganizationRepository"),
            "{bad_repo}"
        );
    }

    #[test]
    fn help_includes_use_when_guidance_for_each_subcommand() {
        assert!(HELP.contains("create ") && HELP.contains("list "));
        assert!(
            HELP.matches("Use when:").count() >= 3,
            "expected command + 2 subcommand Use when cues:\n{HELP}"
        );
    }
}
