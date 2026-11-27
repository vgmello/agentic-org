//! Per-session sandbox read-only dirs, office/git/team denies, worktree grants, and the employee
//! preset config they feed.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use org_agent_runtime::{AgentRuntime, PathPattern, SpecPath};
use org_office::organization_family::records::PermissionPreset;
use org_proto::{ApiError, ErrorCode};

use crate::preset::{apply_worktree_sandbox_rules, build_session_policy, PresetConfig};
use crate::runtime_registry::RuntimeRegistry;

use super::environment::ensure_owner_only_dir;
use super::office_masks::OfficeMasks;
use super::rust_toolchain::canonicalize_with_tail;
use super::sandbox_rules::{
    append_bare_cache_sandbox_rules, append_cargo_credential_denies,
    append_privileged_socket_patterns, append_runtime_credential_denies,
    collect_agy_read_only_dirs, collect_existing_worktree_dirs, collect_non_self_admin_names,
    collect_self_active_admins, push_exact_pattern, push_glob_pattern,
};
use super::SessionManager;

impl SessionManager {
    pub(super) fn collect_session_read_only_dirs(
        &self,
        adapter: &dyn AgentRuntime,
        home: &Path,
        workspace: &Path,
        sandbox_enabled: bool,
    ) -> Vec<SpecPath> {
        if !sandbox_enabled || !adapter.host_profile().needs_explicit_sandbox_ro_dirs {
            return Vec::new();
        }
        let helper_dir = RuntimeRegistry::resolve_helper_dir(self.config.helper_dir.as_deref());
        // The runtime folder holding the office socket (`host.sock`, `r/`, `t/`, every office's
        // socket folder) is forbidden by `collect_agy_read_only_dirs` itself (`TASK-164`).
        let forbidden = [
            home.to_path_buf(),
            workspace.to_path_buf(),
            self.office.folder().to_path_buf(),
            self.office.durable_root().to_path_buf(),
            self.office.runtime_root().to_path_buf(),
        ];
        collect_agy_read_only_dirs(
            &helper_dir,
            Some(&self.config.office_socket),
            std::env::var_os("RUSTUP_HOME"),
            std::env::var_os("CARGO_HOME"),
            std::env::var_os("HOME"),
            &forbidden,
        )
    }

    /// Appends the stable `<runtime_dir>/attachments` directory to `read_only_dirs` for every
    /// runtime (Claude, Codex, Agy) so staged `0400` turn attachments are readable without ever
    /// rotating `spec_hash()` across turns (`TASK-98.3` AC #2 & AC #3).
    pub(super) fn append_attachments_read_only_dir(
        read_only_dirs: &mut Vec<SpecPath>,
        attachments_root: &Path,
    ) {
        let resolved = attachments_root
            .canonicalize()
            .unwrap_or_else(|_| canonicalize_with_tail(attachments_root));
        if let Some(spec) = resolved
            .to_str()
            .and_then(|s| SpecPath::try_from(s).ok())
            .filter(|spec| !read_only_dirs.contains(spec))
        {
            read_only_dirs.push(spec);
        }
    }

    /// Appends `<attachments_root>/**` to `preset_cfg.policy.deny` (`cap:edit`) and
    /// `preset_cfg.sandbox.deny_write` as defense-in-depth write protection for Agy and
    /// unsandboxed sessions while keeping `spec_hash()` constant per agent (`TASK-98.3`).
    pub(super) fn append_attachments_write_denies(
        preset_cfg: &mut PresetConfig,
        attachments_root: &Path,
    ) {
        let resolved = attachments_root
            .canonicalize()
            .unwrap_or_else(|_| canonicalize_with_tail(attachments_root));
        let glob = format!("{}/**", resolved.display());
        let Ok(pat) = PathPattern::try_from(glob.as_str()) else {
            return;
        };
        if !preset_cfg.sandbox.deny_write.contains(&pat) {
            preset_cfg.sandbox.deny_write.push(pat.clone());
        }
        let deny_rule = org_agent_runtime::Rule {
            tool: org_agent_runtime::ToolCapability::Edit.to_rule_tool(),
            scope: org_agent_runtime::RuleScope::Path(pat),
        };
        if !preset_cfg.policy.deny.contains(&deny_rule) {
            preset_cfg.policy.deny.push(deny_rule);
        }
    }

    #[cfg(test)]
    pub(crate) fn denied_office_globs(&self) -> Vec<PathPattern> {
        self.denied_office_globs_for(None)
    }

    #[cfg(test)]
    pub(crate) fn denied_office_globs_for(
        &self,
        self_employee_id: Option<&str>,
    ) -> Vec<PathPattern> {
        self.denied_office_globs_for_runtime(self_employee_id, None)
    }

    pub(crate) fn denied_office_globs_for_runtime(
        &self,
        self_employee_id: Option<&str>,
        active_host_config_subdir: Option<&str>,
    ) -> Vec<PathPattern> {
        let mut out = Vec::new();
        push_glob_pattern(&mut out, self.office.folder());
        push_glob_pattern(&mut out, &self.office.folder().join("git-credentials"));
        let durable = self.office.durable_root();
        let office_id = &self.office.info().office_id;
        let office_local = durable.join("local").join("offices").join(office_id);
        // Durable families, companion stores, restore backups and anything unexpected beside
        // `agents/` or the data roots (`TASK-203`).
        OfficeMasks::of(&self.office).push_denies(&mut out);
        self.append_runtime_office_denies(
            &mut out,
            self.office.runtime_root(),
            office_id,
            self_employee_id,
        );
        self.append_sibling_and_socket_denies(&mut out, &office_local, office_id, self_employee_id);
        append_cargo_credential_denies(&mut out);
        append_runtime_credential_denies(
            &self.config.runtimes,
            active_host_config_subdir,
            &mut out,
        );
        // Other agents' external (BYOA) keys and fast tokens (`TASK-137`).
        for root in &self.config.agent_os_state_roots {
            push_glob_pattern(&mut out, root);
        }
        out
    }

    pub(super) fn append_runtime_office_denies(
        &self,
        out: &mut Vec<PathPattern>,
        runtime_root: &Path,
        office_id: &str,
        self_employee_id: Option<&str>,
    ) {
        push_glob_pattern(out, &runtime_root.join("snapshot-import"));
        push_glob_pattern(out, &runtime_root.join("snapshot-export"));
        if let Ok(office_ids) = self.office.enrolled_office_ids() {
            for other_office_id in office_ids {
                if other_office_id != office_id {
                    push_glob_pattern(out, &runtime_root.join(&other_office_id));
                }
            }
        }
        if let Ok(entries) = std::fs::read_dir(runtime_root) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str != office_id
                    && name_str != "snapshot-import"
                    && name_str != "snapshot-export"
                {
                    let p = entry.path();
                    push_exact_pattern(out, &p);
                    push_glob_pattern(out, &p);
                }
            }
        }
        let office_rt = runtime_root.join(office_id);
        for file in [
            "office.db",
            "office.db-wal",
            "office.db-shm",
            "office.db-journal",
            "office.db.new",
            "office.db.new-wal",
            "office.db.new-shm",
            "office.db.new-journal",
            "git-state.json",
        ] {
            push_exact_pattern(out, &office_rt.join(file));
        }
        for dir in ["protected", "git-home"] {
            push_glob_pattern(out, &office_rt.join(dir));
        }
        match self_employee_id {
            Some(emp_id) => {
                self.append_runtime_team_sibling_denies(out, &office_rt, emp_id);
                self.append_runtime_git_denies(out, runtime_root, &office_rt, emp_id);
            }
            None => {
                push_glob_pattern(out, &office_rt.join("teams"));
                push_glob_pattern(out, &office_rt.join("repositories"));
            }
        }
    }

    pub(super) fn append_runtime_git_denies(
        &self,
        out: &mut Vec<PathPattern>,
        runtime_root: &Path,
        office_rt: &Path,
        emp_id: &str,
    ) {
        let repos_root = office_rt.join("repositories");
        let Ok(git_state) =
            org_office::git::OfficeGitState::load(runtime_root, &self.office.info().office_id)
        else {
            // Fail closed on corrupt/unreadable git-state.json: deny all bare caches.
            push_glob_pattern(out, &repos_root);
            return;
        };
        let self_admins_by_repo = collect_self_active_admins(&git_state, emp_id);
        if self_admins_by_repo.is_empty() {
            // Zero active worktrees: blanket-deny `<office_rt>/repositories/**` so repositories
            // cloned after session launch can never be read by this session (`doc-35`).
            push_glob_pattern(out, &repos_root);
            return;
        }
        let Ok((cache_dirs, extra_entries)) =
            self.collect_bare_cache_entries(&repos_root, Some(&git_state))
        else {
            push_glob_pattern(out, &repos_root);
            return;
        };
        for extra in extra_entries {
            push_exact_pattern(out, &extra);
            push_glob_pattern(out, &extra);
        }
        for cache_dir in cache_dirs {
            let repo_id = cache_dir
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".git"));
            let Some(self_admins) = repo_id.and_then(|rid| self_admins_by_repo.get(rid)) else {
                // Untargeted repository cache: deny all read and write access (`doc-35`).
                push_glob_pattern(out, &cache_dir);
                continue;
            };
            let wt_dir = cache_dir.join("worktrees");
            for other in collect_non_self_admin_names(&wt_dir, repo_id, &git_state, self_admins) {
                let p = wt_dir.join(other);
                push_exact_pattern(out, &p);
                push_glob_pattern(out, &p);
            }
        }
    }

    pub(super) fn append_runtime_team_sibling_denies(
        &self,
        out: &mut Vec<PathPattern>,
        office_rt: &Path,
        emp_id: &str,
    ) {
        let Some((chain, all_team_ids)) = self.resolve_employee_team_chain_and_ids(emp_id) else {
            push_glob_pattern(out, &office_rt.join("teams"));
            return;
        };
        let mut cursor = office_rt.to_path_buf();
        for (idx, team_id) in chain.iter().enumerate() {
            let teams_dir = cursor.join("teams");
            for other_team_id in &all_team_ids {
                if other_team_id != team_id {
                    push_glob_pattern(out, &teams_dir.join(other_team_id));
                }
            }
            if let Ok(entries) = std::fs::read_dir(&teams_dir) {
                for entry in entries.flatten() {
                    if entry.file_name().to_str() != Some(team_id.as_str()) {
                        let p = entry.path();
                        push_exact_pattern(out, &p);
                        push_glob_pattern(out, &p);
                    }
                }
            }
            cursor = teams_dir.join(team_id);
            // Ancestor teams' `agents/` directories belong to other teams' agents; deny them.
            if idx + 1 < chain.len() {
                push_glob_pattern(out, &cursor.join("agents"));
            }
        }
        // Deny any existing or future child subteams nested under the employee's own leaf team.
        push_glob_pattern(out, &cursor.join("teams"));
        let agents_dir = cursor.join("agents");
        for other in self.collect_sibling_agent_ids(&agents_dir, Some(emp_id)) {
            push_glob_pattern(out, &agents_dir.join(other));
        }
    }

    pub(crate) fn resolve_employee_team_chain_and_ids(
        &self,
        employee_id: &str,
    ) -> Option<(Vec<String>, Vec<String>)> {
        resolve_employee_team_chain_and_ids_in_office(&self.office, employee_id)
    }

    pub(crate) fn resolve_employee_team_chain(&self, employee_id: &str) -> Vec<String> {
        self.resolve_employee_team_chain_and_ids(employee_id)
            .map(|(chain, _)| chain)
            .unwrap_or_default()
    }

    pub(crate) fn worktree_sandbox_grants(
        &self,
        employee_id: &str,
        create_root: bool,
    ) -> (Vec<SpecPath>, Vec<PathPattern>) {
        let office_rt = self
            .office
            .runtime_root()
            .join(&self.office.info().office_id);
        let mut writable = Vec::new();
        let mut write_denies = Vec::new();
        let git_state = org_office::git::OfficeGitState::load(
            self.office.runtime_root(),
            &self.office.info().office_id,
        )
        .ok();
        let chain = self.resolve_employee_team_chain(employee_id);
        if !chain.is_empty() {
            let mut my_root = office_rt.clone();
            for tid in &chain {
                my_root = my_root.join("teams").join(tid);
            }
            my_root = my_root.join("agents").join(employee_id);
            if create_root {
                let _ = ensure_owner_only_dir(&my_root);
            }
            let canon_root = canonicalize_with_tail(&my_root);
            if let Ok(spec) = SpecPath::try_from(canon_root.to_string_lossy().as_ref()) {
                writable.push(spec);
            }
            // Emit concrete per-worktree `.git` / `.git/hooks/**` denies (no interior `**`
            // wildcards, which would cause Codex `path_pattern_base` to downgrade `canon_root`
            // to read-only).
            let mut wt_dirs = std::collections::BTreeSet::new();
            if let Some(gs) = &git_state {
                for ws in gs.workspaces.values() {
                    let p = Path::new(&ws.worktree_path);
                    if ws.employee_id == employee_id && ws.state == "active" && p.is_absolute() {
                        wt_dirs.insert(canonicalize_with_tail(p));
                    }
                }
            }
            collect_existing_worktree_dirs(&canon_root, 0, &mut wt_dirs);
            for wt_dir in wt_dirs {
                let dot_git = wt_dir.join(".git");
                push_exact_pattern(&mut write_denies, &dot_git);
                // Worktrees have a regular `.git` file (not a directory); only deny `.git/hooks`
                // when `.git` is a directory so Linux `bwrap` never tries to mask under a file.
                if dot_git.is_dir() {
                    push_exact_pattern(&mut write_denies, &dot_git.join("hooks"));
                    push_glob_pattern(&mut write_denies, &dot_git.join("hooks"));
                }
            }
        }
        // Scope bare-cache writable grants strictly to repositories targeted by `employee_id`'s
        // concrete active worktrees (`doc-35` §Sandbox / `FC-103`). Never grant `<cache>/worktrees`
        // parent directory as writable.
        if let Some(gs) = &git_state {
            let repos_root = office_rt.join("repositories");
            let self_admins_by_repo = collect_self_active_admins(gs, employee_id);
            for (repo_id, self_admins) in &self_admins_by_repo {
                let cache_dir = repos_root.join(format!("{repo_id}.git"));
                append_bare_cache_sandbox_rules(
                    &cache_dir,
                    self_admins,
                    &mut writable,
                    &mut write_denies,
                );
            }
        }
        (writable, write_denies)
    }

    pub(super) fn collect_bare_cache_entries(
        &self,
        repos_root: &Path,
        git_state: Option<&org_office::git::OfficeGitState>,
    ) -> Result<(Vec<PathBuf>, Vec<PathBuf>), ()> {
        let repos = self
            .office
            .list_repositories(None)
            .map_err(|_| ())?
            .map_err(|_| ())?;
        let mut dirs = std::collections::BTreeSet::new();
        let mut extra = std::collections::BTreeSet::new();
        for r in repos {
            dirs.insert(repos_root.join(format!("{}.git", r.record.id.as_str())));
        }
        if let Some(gs) = git_state {
            for rid in gs.caches.keys() {
                dirs.insert(repos_root.join(format!("{rid}.git")));
            }
            for ws in gs.workspaces.values() {
                dirs.insert(repos_root.join(format!("{}.git", ws.repository_id)));
            }
        }
        if let Ok(entries) = std::fs::read_dir(repos_root) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().ends_with(".git") {
                    dirs.insert(entry.path());
                } else {
                    extra.insert(entry.path());
                }
            }
        }
        Ok((dirs.into_iter().collect(), extra.into_iter().collect()))
    }

    pub(super) fn append_sibling_and_socket_denies(
        &self,
        out: &mut Vec<PathPattern>,
        office_local: &Path,
        office_id: &str,
        self_employee_id: Option<&str>,
    ) {
        let agents_root = office_local.join("agents");
        let sibling_ids = self.collect_sibling_agent_ids(&agents_root, self_employee_id);
        for other in &sibling_ids {
            push_glob_pattern(out, &agents_root.join(other));
        }
        // `host.sock`, `r/` and `t/` sit in the runtime folder, beside (never inside) the office
        // sockets folder `o/` that holds the office socket.
        if let Some(runtime) = crate::host_auth::office_runtime_dir(&self.config.office_socket) {
            push_exact_pattern(out, &runtime.join("host.sock"));
            if runtime.file_name().and_then(|s| s.to_str()) == Some("run") {
                if let Some(instance_root) = runtime.parent() {
                    push_exact_pattern(out, &instance_root.join("host.sock"));
                }
            }
            push_glob_pattern(out, &runtime.join("r"));
            let short_root = runtime.join("t");
            let own_slug =
                self_employee_id.map(|emp| crate::host_auth::agent_tmpdir_slug(office_id, emp));
            for other in &sibling_ids {
                let slug = crate::host_auth::agent_tmpdir_slug(office_id, other);
                if own_slug.as_deref() != Some(slug.as_str()) {
                    push_glob_pattern(out, &short_root.join(slug));
                }
            }
            if let Ok(entries) = std::fs::read_dir(&short_root) {
                for entry in entries.flatten() {
                    if own_slug.as_deref() != entry.file_name().to_str() {
                        push_glob_pattern(out, &entry.path());
                    }
                }
            }
        }
        append_privileged_socket_patterns(out);
    }

    pub(crate) fn collect_sibling_agent_ids(
        &self,
        agents_root: &Path,
        self_employee_id: Option<&str>,
    ) -> std::collections::BTreeSet<String> {
        collect_sibling_agent_ids_in_office(&self.office, agents_root, self_employee_id)
    }
}

pub(super) fn resolve_employee_team_chain_and_ids_in_office(
    office: &org_office::Office,
    employee_id: &str,
) -> Option<(Vec<String>, Vec<String>)> {
    let emp = office.load_employee(employee_id).ok()??;
    let snapshot = office.org_snapshot().ok()?;
    let parent_by_id: HashMap<&str, Option<&str>> = snapshot
        .teams
        .iter()
        .map(|t| (t.id.as_str(), t.owner_team_id.as_deref()))
        .collect();
    if !parent_by_id.contains_key(emp.home_team_id.as_str()) {
        return None;
    }
    let mut chain = Vec::new();
    let mut cur = Some(emp.home_team_id.as_str());
    let mut visited = HashSet::new();
    while let Some(tid) = cur {
        if !visited.insert(tid) {
            // Fail closed on team hierarchy cycle.
            return None;
        }
        let &parent = parent_by_id.get(tid)?;
        chain.push(tid.to_owned());
        cur = parent;
    }
    chain.reverse();
    let all_team_ids = snapshot.teams.into_iter().map(|t| t.id).collect();
    Some((chain, all_team_ids))
}

pub(super) fn collect_sibling_agent_ids_in_office(
    office: &org_office::Office,
    agents_root: &Path,
    self_employee_id: Option<&str>,
) -> std::collections::BTreeSet<String> {
    let mut ids = std::collections::BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(agents_root) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                ids.insert(name.to_owned());
            }
        }
    }
    if let Ok(employees) = office.list_employees() {
        for emp in employees {
            ids.insert(emp.id.as_str().to_owned());
        }
    }
    if let Some(me) = self_employee_id {
        ids.remove(me);
    }
    ids
}

impl SessionManager {
    pub(super) fn build_employee_preset_cfg(
        &self,
        employee_id: &str,
        preset: PermissionPreset,
        sandbox_enabled: bool,
        workspace: &SpecPath,
        create_root: bool,
    ) -> Result<PresetConfig, ApiError> {
        self.build_employee_preset_cfg_for_runtime(
            employee_id,
            preset,
            sandbox_enabled,
            workspace,
            create_root,
            None,
        )
    }

    pub(super) fn build_employee_preset_cfg_for_runtime(
        &self,
        employee_id: &str,
        preset: PermissionPreset,
        sandbox_enabled: bool,
        workspace: &SpecPath,
        create_root: bool,
        active_host_config_subdir: Option<&str>,
    ) -> Result<PresetConfig, ApiError> {
        let socket_str = self.config.office_socket.display().to_string();
        let socket_spec = SpecPath::try_from(socket_str.as_str()).ok();
        let denied_globs =
            self.denied_office_globs_for_runtime(Some(employee_id), active_host_config_subdir);
        let mut cfg = build_session_policy(
            preset,
            sandbox_enabled,
            workspace,
            socket_spec.as_ref(),
            &denied_globs,
            1,
        )
        .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid permission preset"))?;
        let (wt_writable, wt_write_denies) = self.worktree_sandbox_grants(employee_id, create_root);
        apply_worktree_sandbox_rules(&mut cfg, &wt_writable, &wt_write_denies);
        Ok(cfg)
    }
}
