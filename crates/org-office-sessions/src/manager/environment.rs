//! Per-run directories, standing instructions, and the scrubbed session environment (including
//! vault-native launch auth and endpoint variables).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use org_agent_runtime::EnvName;
use org_office::organization_family::records::{BuiltinKey, Employee, PermissionPreset};
use org_office::Office;
use org_proto::{ApiError, ErrorCode};
use org_supervise::{Environment, SearchPath};

use crate::instructions::{
    assemble_standing_instructions, compose_turn_objective, resolve_placement, InstructionInput,
    Placement, StandingInstructions, TurnSender, TurnTrigger,
};
use crate::preset::PresetConfig;
use crate::runtime_registry::RuntimeRegistry;

use super::rust_toolchain::{append_rust_toolchain_env, RustToolchainEnvInput};
use super::SessionManager;

/// Bytes the organization's processes and standards may take in one prompt (`doc-34` §3.2).
const PROCESS_BUDGET_BYTES: usize = 128 * 1024;

impl SessionManager {
    pub(super) fn load_preset_and_sandbox(
        &self,
        employee_id: &str,
    ) -> Result<(PermissionPreset, bool), ApiError> {
        let exec = self.office.load_agent_execution(employee_id).map_err(|_| {
            ApiError::new(ErrorCode::StorageError, "could not read execution config")
        })?;
        let stored = exec.map_or(PermissionPreset::SafeAsk, |cfg| cfg.permission_preset);
        let builtin_key = self
            .office
            .load_employee(employee_id)
            .ok()
            .flatten()
            .and_then(|employee| employee.builtin_key);
        Ok((PermissionPreset::effective_for(builtin_key, stored), true))
    }

    pub(super) fn agent_base_dir(&self, employee_id: &str) -> PathBuf {
        self.office
            .durable_root()
            .join("local")
            .join("offices")
            .join(&self.office.info().office_id)
            .join("agents")
            .join(employee_id)
    }

    pub(crate) fn prepare_run_dirs(
        &self,
        employee_id: &str,
    ) -> Result<(PathBuf, PathBuf), ApiError> {
        let office_id = &self.office.info().office_id;
        let base = self.agent_base_dir(employee_id);
        let runtime_dir = base.join("runtime");
        let tmpdir = crate::host_auth::resolve_agent_tmpdir(
            &self.config.office_socket,
            &base.join("tmp"),
            office_id,
            employee_id,
        );
        let attachments_dir = crate::turn_attachments::attachments_root(&runtime_dir);
        for dir in [&base, &runtime_dir, &attachments_dir, &tmpdir] {
            ensure_owner_only_dir(dir)?;
        }
        Ok((runtime_dir, tmpdir))
    }

    /// The agent's standing instructions (`doc-27` §1). They depend on no task: the processes
    /// are those of the agent's home team chain, and a turn about tasks carries what their
    /// chains change ([`task_process_context`]), so the standing prefix stays cacheable across
    /// turns and tasks.
    pub(super) fn build_instructions(
        &self,
        employee: &Employee,
        home: &Path,
        workspace: &Path,
        preset_cfg: &PresetConfig,
    ) -> Result<StandingInstructions, ApiError> {
        let info = self.office.info();
        let role = employee
            .role_id
            .as_ref()
            .and_then(|id| self.office.load_role(id.as_str()).ok().flatten());
        let rendered_processes = self
            .office
            .render_agent_process_instructions(
                employee.id.as_str(),
                None,
                None,
                std::collections::BTreeMap::new(),
                PROCESS_BUDGET_BYTES,
            )
            .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))??;
        let exec_prompt = self
            .office
            .load_agent_execution(employee.id.as_str())
            .ok()
            .flatten()
            .map(|cfg| cfg.system_prompt_markdown);
        // The Chief of Staff's profile body is her charter for people to read; her duties are
        // her guidance (Part 5), so the charter never reaches her prompt (`TASK-80.1`).
        let profile_body = match employee.builtin_key {
            Some(BuiltinKey::ChiefOfStaff) => "",
            _ => employee.body.as_str(),
        };
        let combined_agent_instructions =
            combine_agent_instructions(profile_body, exec_prompt.as_deref());
        let placement = self.placement_of(employee);
        let table = crate::instructions::command_table_or_empty(self.config.command_catalogue);
        let input = InstructionInput {
            organization_name: &info.org_name,
            office_name: &info.office_name,
            employee_id: employee.id.as_str(),
            employee_name: &employee.name,
            handle: &employee.handle,
            placement: &placement,
            role_name: role.as_ref().map(|r| r.name.as_str()),
            role_instructions: role.as_ref().map(|r| r.body.as_str()),
            process_instructions: (!rendered_processes.text.is_empty())
                .then_some(rendered_processes.text.as_str()),
            agent_instructions: &combined_agent_instructions,
            repository_instructions: None,
            is_hr_lead: employee.builtin_key == Some(BuiltinKey::Hr),
            is_chief_of_staff: employee.builtin_key == Some(BuiltinKey::ChiefOfStaff),
            private_home: &home.display().to_string(),
            workspace: &workspace.display().to_string(),
            permission_preset: preset_cfg.preset.as_str(),
            sandbox_enabled: preset_cfg.sandbox.enabled,
        };
        Ok(assemble_standing_instructions(&input, &table))
    }

    /// The agent's home team and leads by name. Without a readable snapshot the home team's ID
    /// stands in for its name and no lead is listed.
    fn placement_of(&self, employee: &Employee) -> Placement {
        let reports_to = employee
            .reports_to_employee_id
            .as_ref()
            .map(|id| id.as_str());
        match self.office.org_snapshot() {
            Ok(snapshot) => resolve_placement(
                &snapshot,
                employee.id.as_str(),
                employee.home_team_id.as_str(),
                reports_to,
            ),
            Err(_) => Placement {
                home_team_name: employee.home_team_id.as_str().to_owned(),
                ..Placement::default()
            },
        }
    }

    /// The sender of a request `member_id` made to `target`, without an HQ attestation.
    #[cfg(test)]
    pub(crate) fn turn_sender(&self, target: &Employee, member_id: &str) -> TurnSender {
        self.turn_sender_attested(target, member_id, false)
    }

    /// The office-verified sender of a request `member_id` made to `target`: the same relation
    /// a queued wake from them carries ([`crate::provenance::verified_sender_relation`]). A sender
    /// the office cannot load keeps its raw ID as name and is badged `unknown`. With
    /// `hq_attested_owner` the HQ vouched on its verified channel that `member_id` is the
    /// organization's owner (an owner instruction it proxied to this satellite), so the sender
    /// is badged `owner` even before this office has replicated who the owner is.
    pub(crate) fn turn_sender_attested(
        &self,
        target: &Employee,
        member_id: &str,
        hq_attested_owner: bool,
    ) -> TurnSender {
        let record = self.office.load_employee(member_id).ok().flatten();
        let relation = if hq_attested_owner {
            "owner"
        } else {
            crate::provenance::verified_sender_relation(
                &self.office,
                target,
                member_id,
                record.as_ref(),
            )
        }
        .to_owned();
        match record {
            Some(employee) => TurnSender {
                name: employee.name,
                handle: Some(employee.handle).filter(|h| !h.is_empty()),
                relation,
            },
            None => TurnSender {
                name: member_id.to_owned(),
                handle: None,
                relation,
            },
        }
    }

    /// The turn input for an instruction `requester` sent to `employee`, without an HQ
    /// attestation.
    #[cfg(test)]
    pub(crate) fn objective_text(
        &self,
        employee: &Employee,
        requester: &str,
        text: &str,
    ) -> String {
        self.objective_text_attested(employee, requester, false, text)
    }

    /// The turn input for an instruction `requester` sent to `employee`: a hiring request for
    /// the built-in HR lead, a direct instruction otherwise. Both are fenced as data; the
    /// sender is badged as [`Self::turn_sender_attested`] says.
    pub(crate) fn objective_text_attested(
        &self,
        employee: &Employee,
        requester: &str,
        hq_attested_owner: bool,
        text: &str,
    ) -> String {
        let sender = self.turn_sender_attested(employee, requester, hq_attested_owner);
        if employee.builtin_key == Some(BuiltinKey::Hr) {
            compose_turn_objective(&TurnTrigger::HrProposalRequest {
                requester: sender,
                request: text.to_owned(),
            })
        } else {
            compose_turn_objective(&TurnTrigger::DirectInstruction {
                from: sender,
                text: text.to_owned(),
            })
        }
    }

    /// The variables a session starts from: its home, `TMPDIR`, locale and terminal, and the
    /// host's read-only Rust toolchain (none for an operator session, `TASK-80.1`).
    pub(crate) fn captured_env(&self, input: &SessionEnvInput<'_>) -> Vec<(OsString, OsString)> {
        let mut captured = vec![
            (
                OsString::from("HOME"),
                input.home.as_os_str().to_os_string(),
            ),
            (
                OsString::from("TMPDIR"),
                input.tmpdir.as_os_str().to_os_string(),
            ),
            (OsString::from("LANG"), OsString::from("en_US.UTF-8")),
            (OsString::from("TERM"), OsString::from("dumb")),
        ];
        // Preserve host read-only Rust toolchain (`RUSTUP_HOME`) and split read-only Cargo caches
        // (`registry/{index,cache}`, `git/db`) inside each agent's isolated `CARGO_HOME` while
        // giving each agent its own writable `registry/src`, `git/checkouts`, `.package-cache*`,
        // and `CARGO_TARGET_DIR` (`TASK-126`, `TASK-159`, `TASK-135`). Host `~/.cargo/credentials*`
        // remain explicitly denied in policy. When sandboxed, set `CARGO_NET_OFFLINE=true` so
        // Cargo never triggers closed-egress network approvals (`TASK-143`, `TASK-135`).
        let rust_forbidden = vec![
            input.home.to_path_buf(),
            input.workspace.to_path_buf(),
            self.office.durable_root().to_path_buf(),
            self.office.runtime_root().to_path_buf(),
        ];
        // An operator session has no repository and no Rust workflow: no toolchain, no cache
        // links in her home (`TASK-80.1`).
        if !input.operator {
            append_rust_toolchain_env(
                &mut captured,
                RustToolchainEnvInput {
                    agent_home: Some(input.home),
                    agent_base: input.agent_base,
                    rustup_explicit: std::env::var_os("RUSTUP_HOME"),
                    cargo_explicit: std::env::var_os("CARGO_HOME"),
                    host_home: std::env::var_os("HOME"),
                    forbidden: &rust_forbidden,
                    sandbox_enabled: input.sandbox_enabled,
                },
            );
        }
        captured
    }

    pub(super) fn build_environment(
        &self,
        input: SessionEnvInput<'_>,
    ) -> Result<Environment, ApiError> {
        let helper_dir = RuntimeRegistry::resolve_helper_dir(self.config.helper_dir.as_deref());
        let captured_path =
            std::env::var_os("PATH").unwrap_or_else(|| OsString::from("/usr/bin:/bin"));
        let forbidden = vec![input.home.to_path_buf(), input.workspace.to_path_buf()];
        let search_path = SearchPath::new(&helper_dir, &captured_path, &forbidden)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "invalid helper search path"))?;
        let captured = self.captured_env(&input);
        let info = self.office.info();
        let (git_name, git_email) = agent_git_identity(input.employee_name, input.employee_handle);
        let context = vec![
            (
                OsString::from("AGENT_OS_SOCKET"),
                self.config.office_socket.as_os_str().to_os_string(),
            ),
            (
                OsString::from("AGENT_OS_OFFICE"),
                OsString::from(&info.office_id),
            ),
            (OsString::from("AGENT_OS_ORG"), OsString::from(&info.org_id)),
            (
                OsString::from("AGENT_OS_AGENT_ID"),
                OsString::from(input.employee_id),
            ),
            (
                OsString::from("AGENT_OS_SESSION_ID"),
                OsString::from(input.session_id),
            ),
            (
                OsString::from("AGENT_OS_SESSION_TOKEN"),
                OsString::from(input.token),
            ),
            (
                OsString::from("AGENT_OS_BINDING_FILE"),
                input.binding_file.as_os_str().to_os_string(),
            ),
            (OsString::from("AGENT_OS_FORMAT"), OsString::from("json")),
            (OsString::from("GIT_AUTHOR_NAME"), OsString::from(&git_name)),
            (
                OsString::from("GIT_AUTHOR_EMAIL"),
                OsString::from(&git_email),
            ),
            (
                OsString::from("GIT_COMMITTER_NAME"),
                OsString::from(&git_name),
            ),
            (
                OsString::from("GIT_COMMITTER_EMAIL"),
                OsString::from(&git_email),
            ),
        ];
        Environment::new(search_path, &captured)
            .and_then(|env| env.with_context(&context))
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "could not build session env"))
    }
}

pub(super) fn resolve_workspace(home: &Path, explicit: Option<&str>) -> Result<PathBuf, ApiError> {
    let path = explicit
        .map(PathBuf::from)
        .unwrap_or_else(|| home.to_path_buf());
    path.canonicalize().map_err(|_| {
        ApiError::new(
            ErrorCode::InvalidRequest,
            "workspace directory does not exist",
        )
    })
}

pub(crate) fn ensure_owner_only_dir(path: &Path) -> Result<(), ApiError> {
    org_platform::fs::create_owner_only_dir_all(path)
        .and_then(|()| org_platform::fs::set_owner_only_dir(path))
        .map_err(|_| {
            ApiError::new(
                ErrorCode::StorageError,
                "could not create private session directory",
            )
        })
}

pub(super) fn sanitize_agent_name(handle: &str) -> String {
    let mut out: String = handle
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    if out.is_empty() || !out.as_bytes()[0].is_ascii_alphanumeric() {
        out.insert_str(0, "agent-");
    }
    out.truncate(64);
    out
}

pub(crate) fn agent_git_identity(name: &str, handle: &str) -> (String, String) {
    let safe_handle = sanitize_agent_name(handle);
    let clean_name: String = name
        .chars()
        .filter(|c| !matches!(c, '<' | '>'))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = clean_name.trim();
    let has_ident_char = trimmed.chars().any(char::is_alphanumeric);
    let display = if has_ident_char {
        trimmed.to_owned()
    } else {
        safe_handle.clone()
    };
    let email = format!("{safe_handle}@agents.agentic-org.local");
    (display, email)
}

pub(super) fn write_owner_only_binding_file(path: &Path, token: &str) -> Result<(), ApiError> {
    let _ = std::fs::remove_file(path);
    org_platform::fs::write_owner_only_file(path, token.as_bytes())
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not write session binding"))
}

pub(crate) struct SessionEnvInput<'a> {
    pub(crate) home: &'a Path,
    /// Agent private root (`<durable>/local/offices/<office_id>/agents/<employee_id>`), owning
    /// isolated `cache/cargo-target` beside `home/` (`TASK-135`).
    pub(crate) agent_base: Option<&'a Path>,
    pub(crate) workspace: &'a Path,
    pub(crate) tmpdir: &'a Path,
    pub(crate) employee_id: &'a str,
    pub(crate) employee_name: &'a str,
    pub(crate) employee_handle: &'a str,
    pub(crate) session_id: &'a str,
    pub(crate) token: &'a str,
    pub(crate) binding_file: &'a Path,
    pub(crate) sandbox_enabled: bool,
    /// An operator session (`OperatorSandboxed`): no Rust toolchain environment.
    pub(crate) operator: bool,
}

pub(super) fn office_env_names() -> Vec<EnvName> {
    [
        "AGENT_OS_SOCKET",
        "AGENT_OS_OFFICE",
        "AGENT_OS_ORG",
        "AGENT_OS_AGENT_ID",
        "AGENT_OS_SESSION_ID",
        "AGENT_OS_SESSION_TOKEN",
        "AGENT_OS_BINDING_FILE",
        "AGENT_OS_FORMAT",
        "GIT_AUTHOR_NAME",
        "GIT_AUTHOR_EMAIL",
        "GIT_COMMITTER_NAME",
        "GIT_COMMITTER_EMAIL",
        "RUSTUP_HOME",
        "CARGO_HOME",
        "CARGO_TARGET_DIR",
        "CARGO_NET_OFFLINE",
        "CARGO_BUILD_JOBS",
        "RAYON_NUM_THREADS",
        "MAKEFLAGS",
        "NPM_CONFIG_JOBS",
    ]
    .into_iter()
    .map(|name| EnvName::try_from(name).expect("static env name"))
    .collect()
}

/// Computes the bounded default per-agent build concurrency from host parallelism: `(cpus/2).clamp(1,4)`.
pub fn default_build_concurrency_jobs() -> u32 {
    let cpus = std::thread::available_parallelism().map_or(2, |n| n.get() as u32);
    (cpus / 2).clamp(1, 4)
}

/// Clamps configured build concurrency jobs into `1..=16`.
pub fn clamp_build_concurrency_jobs(jobs: u32) -> u32 {
    jobs.clamp(1, 16)
}

/// Supervisor-enforced bounded build-concurrency variables injected into child sessions (`TASK-133`).
pub(crate) fn build_concurrency_env(jobs: u32) -> [(&'static str, String); 4] {
    let capped = clamp_build_concurrency_jobs(jobs);
    let n = capped.to_string();
    [
        ("CARGO_BUILD_JOBS", n.clone()),
        ("RAYON_NUM_THREADS", n.clone()),
        ("MAKEFLAGS", format!("-j{n}")),
        ("NPM_CONFIG_JOBS", n),
    ]
}

/// Injects bounded build-concurrency caps into the supervised launch environment and declares their
/// names on `plan.auth_env_names` (never `secret_env_names`, never colliding with `plan.env_add`).
pub(super) fn apply_build_concurrency_env(
    plan: &mut org_agent_runtime::LaunchPlan,
    launch_env: &mut Vec<(OsString, OsString)>,
    jobs: u32,
) {
    for (name, value) in build_concurrency_env(jobs) {
        if plan.env_add.iter().any(|(k, _)| k.as_str() == name) {
            continue;
        }
        launch_env.retain(|(k, _)| k != name);
        launch_env.push((OsString::from(name), OsString::from(value)));
        if !plan.auth_env_names.iter().any(|n| n.as_str() == name) {
            plan.auth_env_names
                .push(EnvName::try_from(name).expect("static build-cap env name"));
        }
    }
}

/// What the tasks `task_refs` change about `employee_id`'s standing processes, for the turn that
/// names them; `None` when nothing changes (or no task still exists). The standing must-follow
/// section and everything the turn adds share [`PROCESS_BUDGET_BYTES`]: over it, or when the
/// processes cannot be read, the turn is refused as a launch is (`doc-34` Decision 4).
pub(crate) fn task_process_context(
    office: &Office,
    employee_id: &str,
    task_refs: &[&str],
) -> Result<Option<String>, ApiError> {
    if task_refs.is_empty() {
        return Ok(None);
    }
    office
        .render_agent_task_process_delta(employee_id, task_refs, PROCESS_BUDGET_BYTES)
        .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))?
}

pub(super) fn combine_agent_instructions(
    profile_body: &str,
    system_prompt: Option<&str>,
) -> String {
    let body = profile_body.trim();
    let prompt = system_prompt.map(str::trim).unwrap_or("");
    match (prompt.is_empty(), body.is_empty()) {
        (true, true) => String::new(),
        (false, true) => prompt.to_owned(),
        (true, false) => body.to_owned(),
        (false, false) if prompt == body => prompt.to_owned(),
        (false, false) => format!("{prompt}\n\n{body}"),
    }
}

pub(super) fn assemble_launch_env(
    mut launch_env: Vec<(OsString, OsString)>,
    endpoint_env: Vec<(OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    launch_env.extend(endpoint_env);
    launch_env
}
