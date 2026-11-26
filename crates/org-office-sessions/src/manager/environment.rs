//! Per-run directories, standing instructions, and the scrubbed session environment.

use super::*;

impl SessionManager {
    pub(super) fn load_preset_and_sandbox(
        &self,
        employee_id: &str,
    ) -> Result<(PermissionPreset, bool), ApiError> {
        let exec = self.office.load_agent_execution(employee_id).map_err(|_| {
            ApiError::new(ErrorCode::StorageError, "could not read execution config")
        })?;
        Ok(match exec {
            Some(cfg) => (cfg.permission_preset, true),
            None => (PermissionPreset::SafeAsk, true),
        })
    }

    pub(super) fn prepare_run_dirs(
        &self,
        employee_id: &str,
    ) -> Result<(PathBuf, PathBuf), ApiError> {
        let office_id = &self.office.info().office_id;
        let base = self
            .office
            .durable_root()
            .join("local")
            .join("offices")
            .join(office_id)
            .join("agents")
            .join(employee_id);
        let runtime_dir = base.join("runtime");
        let tmpdir = crate::host_auth::resolve_agent_tmpdir(
            &self.config.office_socket,
            &base.join("tmp"),
            office_id,
            employee_id,
        );
        for dir in [&runtime_dir, &tmpdir] {
            ensure_owner_only_dir(dir)?;
        }
        Ok((runtime_dir, tmpdir))
    }

    pub(super) fn build_instructions(
        &self,
        employee: &Employee,
        home: &Path,
        workspace: &Path,
        preset_cfg: &PresetConfig,
        task_ref: Option<&str>,
    ) -> Result<String, ApiError> {
        let info = self.office.info();
        let role = employee
            .role_id
            .as_ref()
            .and_then(|id| self.office.load_role(id.as_str()).ok().flatten());
        let rendered_processes = match self
            .office
            .render_agent_process_instructions(
                employee.id.as_str(),
                None,
                task_ref,
                std::collections::BTreeMap::new(),
                128 * 1024,
            )
            .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))?
        {
            Ok(rendered) => rendered,
            Err(err) if task_ref.is_some() && err.code == ErrorCode::NotFound => self
                .office
                .render_agent_process_instructions(
                    employee.id.as_str(),
                    None,
                    None,
                    std::collections::BTreeMap::new(),
                    128 * 1024,
                )
                .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))??,
            Err(err) => return Err(err),
        };
        let exec_prompt = self
            .office
            .load_agent_execution(employee.id.as_str())
            .ok()
            .flatten()
            .map(|cfg| cfg.system_prompt_markdown);
        let combined_agent_instructions =
            combine_agent_instructions(&employee.body, exec_prompt.as_deref());
        let table = crate::instructions::command_table_or_empty(self.config.command_catalogue);
        let input = InstructionInput {
            organization_name: &info.org_name,
            office_name: &info.office_name,
            employee_id: employee.id.as_str(),
            employee_name: &employee.name,
            handle: &employee.handle,
            team_name: employee.home_team_id.as_str(),
            role_name: role.as_ref().map(|r| r.name.as_str()),
            role_instructions: role.as_ref().map(|r| r.body.as_str()),
            process_instructions: (!rendered_processes.text.is_empty())
                .then_some(rendered_processes.text.as_str()),
            agent_instructions: &combined_agent_instructions,
            is_hr_lead: employee.builtin_key == Some(BuiltinKey::Hr),
            private_home: &home.display().to_string(),
            workspace: &workspace.display().to_string(),
            permission_preset: preset_cfg.preset.as_str(),
            sandbox_enabled: preset_cfg.sandbox.enabled,
        };
        Ok(assemble_standing_instructions(&input, &table))
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
        // Preserve host read-only Rust caches (`registry`, `git`, `.rustup`) inside the agent's
        // isolated home via safe symlinks (never linking host `credentials*` or `config*`) so
        // `rustc`/`cargo` work offline with a writable `<agent_home>/.cargo/.package-cache` even
        // when runtimes scrub `CARGO_HOME` (`TASK-126`, `TASK-159`). Host `~/.cargo/credentials*`
        // remain explicitly denied in policy. When sandboxed, set `CARGO_NET_OFFLINE=true` so
        // Cargo never hits closed egress (`TASK-143`).
        let rust_forbidden = vec![
            input.home.to_path_buf(),
            input.workspace.to_path_buf(),
            self.office.durable_root().to_path_buf(),
            self.office.runtime_root().to_path_buf(),
        ];
        append_rust_toolchain_env(
            &mut captured,
            Some(input.home),
            std::env::var_os("RUSTUP_HOME"),
            std::env::var_os("CARGO_HOME"),
            std::env::var_os("HOME"),
            &rust_forbidden,
            input.sandbox_enabled,
        );
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

pub(super) fn build_objective_text(employee: &Employee, requester: &str, text: &str) -> String {
    if employee.builtin_key == Some(BuiltinKey::Hr) {
        compose_turn_objective(&TurnTrigger::HrProposalRequest {
            requester: requester.to_owned(),
            request: text.to_owned(),
        })
    } else {
        compose_turn_objective(&TurnTrigger::DirectInstruction {
            text: text.to_owned(),
        })
    }
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

pub(super) struct SessionEnvInput<'a> {
    pub(super) home: &'a Path,
    pub(super) workspace: &'a Path,
    pub(super) tmpdir: &'a Path,
    pub(super) employee_id: &'a str,
    pub(super) employee_name: &'a str,
    pub(super) employee_handle: &'a str,
    pub(super) session_id: &'a str,
    pub(super) token: &'a str,
    pub(super) binding_file: &'a Path,
    pub(super) sandbox_enabled: bool,
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
        "GIT_AUTHOR_NAME",
        "GIT_AUTHOR_EMAIL",
        "GIT_COMMITTER_NAME",
        "GIT_COMMITTER_EMAIL",
        "RUSTUP_HOME",
        "CARGO_HOME",
        "CARGO_NET_OFFLINE",
    ]
    .into_iter()
    .map(|name| EnvName::try_from(name).expect("static env name"))
    .collect()
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
