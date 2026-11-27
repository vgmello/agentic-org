//! Warm-session route digests (config, topology, worktree sandbox) and warm turn-permit
//! acquisition (`doc-88` §6, `AE-04`, `AE-11`).

use org_office::organization_family::records::{ProviderConnection, ReasoningEffort};
use org_office::providers::PermitAcquireOutcome;
use org_office::Office;
use org_proto::providers::PermitAuthority;
use org_proto::{ApiError, ErrorCode};

use super::{
    check_local_readiness, hex_short_sha256, permit_authority_for, HqPermitAcquireSpec,
    HqPermitCoordinator,
};

/// Outcome of attempting to re-acquire a turn permit on an idle warm session's existing connection.
#[derive(Debug)]
pub(crate) enum WarmPermitOutcome {
    Granted((String, u64)),
    /// Warm session no longer matches current config/readiness/route digest and must cold-start.
    StaleWarm,
    /// Connection is full or cooling down; `multi_connection` tells the caller whether another
    /// connection in the profile pool may be able to serve if the warm session is recycled.
    Unavailable {
        multi_connection: bool,
        reason: String,
    },
    /// Hard refusal (e.g. `BUDGET_HELD` or storage failure).
    Refused(ApiError),
}

pub(crate) fn compute_worktree_sandbox_digest(office: &Office, employee_id: &str) -> String {
    let office_id = &office.info().office_id;
    let Ok(git_state) = org_office::git::OfficeGitState::load(office.runtime_root(), office_id)
    else {
        return "err".to_owned();
    };
    let Some(topo_digest) = compute_org_topology_digest(office, employee_id) else {
        return "err".to_owned();
    };
    let mut self_entries = Vec::new();
    let mut targeted_repos = std::collections::BTreeSet::new();
    for ws in git_state.workspaces.values() {
        if ws.employee_id == employee_id && ws.state == "active" {
            targeted_repos.insert(ws.repository_id.as_str());
            self_entries.push(format!(
                "{}:{}:{}",
                ws.repository_id, ws.admin_dir_name, ws.worktree_path
            ));
        }
    }
    if targeted_repos.is_empty() {
        // Zero-worktree sessions emit a blanket `<office_rt>/repositories/**` deny, so untargeted
        // repos cloned later are already blocked; topology still invalidates on sibling hires,
        // new teams, team moves, or newly enrolled offices.
        return hex_short_sha256(format!("topo={topo_digest}|wt=none").as_bytes());
    }
    self_entries.sort();
    let Ok(Ok(repos)) = office.list_repositories(None) else {
        return "err".to_owned();
    };
    let repos_root = office.runtime_root().join(office_id).join("repositories");
    let known_repos = collect_digest_repo_ids(
        repos.iter().map(|r| r.record.id.as_str()),
        &git_state,
        &repos_root,
    );
    let peer_admins = collect_digest_peer_admins(&git_state, &targeted_repos, &repos_root);
    let material = format!(
        "topo={topo_digest}|self={}|repos={}|peers={}",
        self_entries.join(","),
        known_repos.join(","),
        peer_admins.join(",")
    );
    hex_short_sha256(material.as_bytes())
}

pub(super) fn compute_org_topology_digest(office: &Office, employee_id: &str) -> Option<String> {
    let emp = office.load_employee(employee_id).ok()??;
    let snapshot = office.org_snapshot().ok()?;
    let employees = office.list_employees().ok()?;
    let mut team_entries: Vec<String> = snapshot
        .teams
        .iter()
        .map(|t| format!("{}:{}", t.id, t.owner_team_id.as_deref().unwrap_or("")))
        .collect();
    team_entries.sort();
    let mut emp_ids: Vec<String> = employees
        .into_iter()
        .map(|e| e.id.as_str().to_owned())
        .collect();
    emp_ids.sort();
    let mut office_ids = office.enrolled_office_ids().ok()?;
    office_ids.sort();
    let material = format!(
        "home={}|teams={}|emps={}|offices={}",
        emp.home_team_id,
        team_entries.join(","),
        emp_ids.join(","),
        office_ids.join(",")
    );
    Some(hex_short_sha256(material.as_bytes()))
}

pub(super) fn collect_digest_repo_ids<'a>(
    repo_ids: impl IntoIterator<Item = &'a str>,
    git_state: &org_office::git::OfficeGitState,
    repos_root: &std::path::Path,
) -> Vec<String> {
    let mut ids = std::collections::BTreeSet::new();
    for rid in repo_ids {
        ids.insert(rid.to_owned());
    }
    for rid in git_state.caches.keys() {
        ids.insert(rid.clone());
    }
    for ws in git_state.workspaces.values() {
        ids.insert(ws.repository_id.clone());
    }
    if let Ok(entries) = std::fs::read_dir(repos_root) {
        for name in entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
        {
            ids.insert(name);
        }
    }
    ids.into_iter().collect()
}

pub(super) fn collect_digest_peer_admins(
    git_state: &org_office::git::OfficeGitState,
    targeted_repos: &std::collections::BTreeSet<&str>,
    repos_root: &std::path::Path,
) -> Vec<String> {
    let mut peers = std::collections::BTreeSet::new();
    for ws in git_state.workspaces.values() {
        if targeted_repos.contains(ws.repository_id.as_str()) && !ws.admin_dir_name.is_empty() {
            peers.insert(format!("{}:{}", ws.repository_id, ws.admin_dir_name));
        }
    }
    for rid in targeted_repos {
        let wt_dir = repos_root.join(format!("{rid}.git")).join("worktrees");
        if let Ok(entries) = std::fs::read_dir(wt_dir) {
            for name in entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
            {
                peers.insert(format!("{rid}:{name}"));
            }
        }
    }
    peers.into_iter().collect()
}

pub(super) fn compute_route_digest(
    office: &Office,
    employee_id: &str,
    conn: &ProviderConnection,
    binding_rev: u64,
    resolved_model: &str,
    effort: Option<ReasoningEffort>,
    exec_rev: u64,
) -> String {
    let wt_digest = compute_worktree_sandbox_digest(office, employee_id);
    compute_route_digest_with_wt(
        conn,
        binding_rev,
        resolved_model,
        effort,
        exec_rev,
        &wt_digest,
    )
}

/// The warm-session route digest. The agent's reasoning effort is in it by value (`TASK-181`),
/// so a changed effort recycles a warm session even where the execution revision alone would
/// not show it; an agent with no effort keeps the digest it had before effort existed.
pub(crate) fn compute_route_digest_with_wt(
    conn: &ProviderConnection,
    binding_rev: u64,
    resolved_model: &str,
    effort: Option<ReasoningEffort>,
    exec_rev: u64,
    wt_digest: &str,
) -> String {
    let effort_part = effort
        .map(|e| format!(":effort={}", e.as_str()))
        .unwrap_or_default();
    let digest_input = format!(
        "conn={}:rev={}:bind={}:endpoint={}:model={}{effort_part}:exec={}:wt={}",
        conn.id.as_str(),
        conn.meta.revision,
        binding_rev,
        conn.endpoint_url.as_deref().unwrap_or(""),
        resolved_model,
        exec_rev,
        wt_digest
    );
    hex_short_sha256(digest_input.as_bytes())
}

fn warm_conflict(msg: &str) -> WarmPermitOutcome {
    WarmPermitOutcome::Refused(ApiError::new(ErrorCode::Conflict, msg))
}

/// Validates warm-session config/readiness/route digest and acquires a fresh turn permit.
pub(crate) fn acquire_warm_permit_for_office(
    office: &Office,
    hq_permits: Option<&dyn HqPermitCoordinator>,
    connection_id: &str,
    employee_id: &str,
    task_id: Option<&str>,
    expected_route_digest: Option<&str>,
) -> WarmPermitOutcome {
    let Ok(Some(cfg)) = office.load_agent_execution(employee_id) else {
        return warm_conflict("EXECUTION_DISABLED: agent execution configuration missing");
    };
    if !cfg.enabled {
        return warm_conflict("EXECUTION_DISABLED: agent execution configuration is disabled");
    }
    let Ok(Some(profile)) = office.load_model_profile(cfg.model_profile_id.as_str()) else {
        return warm_conflict("PROFILE_NOT_FOUND: assigned model profile does not exist");
    };
    if profile.archived {
        return warm_conflict("PROFILE_ARCHIVED: assigned model profile is archived");
    }
    let member_of_profile = profile
        .connection_ids
        .iter()
        .chain(profile.generated_for_connection_id.iter())
        .any(|id| id.as_str() == connection_id);
    if !member_of_profile {
        return WarmPermitOutcome::StaleWarm;
    }
    let Ok(Some(conn)) = office.load_provider_connection(connection_id) else {
        return WarmPermitOutcome::StaleWarm;
    };
    let multi_connection = profile.connection_ids.len() > 1;
    if conn.archived || !conn.enabled {
        return WarmPermitOutcome::StaleWarm;
    }
    let now_ms = office.current_time_ms();
    let (binding_rev, resolved_model, _supports_vision) =
        match check_local_readiness(office, &conn, &cfg.model_id, now_ms) {
            Ok(Ok(v)) => v,
            Ok(Err(_)) | Err(_) => return WarmPermitOutcome::StaleWarm,
        };
    if let Some(expected) = expected_route_digest {
        let wt_digest = compute_worktree_sandbox_digest(office, employee_id);
        if wt_digest == "err"
            || compute_route_digest_with_wt(
                &conn,
                binding_rev,
                &resolved_model,
                cfg.effort,
                cfg.meta.revision,
                &wt_digest,
            ) != expected
        {
            return WarmPermitOutcome::StaleWarm;
        }
    }
    let authority = permit_authority_for(office, &conn);
    let permit_res = if office.info().role == org_office::OfficeRole::Satellite
        && authority == PermitAuthority::HqPool
    {
        let hq_outcome = hq_permits.and_then(|coord| {
            coord
                .acquire_permit(HqPermitAcquireSpec {
                    connection_id: connection_id.to_owned(),
                    employee_id: employee_id.to_owned(),
                    task_id: task_id.map(str::to_owned),
                    model_id: cfg.model_id.clone(),
                    is_failover_switch: false,
                })
                .ok()
        });
        match hq_outcome {
            Some(outcome) => Ok(Ok(outcome)),
            None => {
                return WarmPermitOutcome::Unavailable {
                    multi_connection,
                    reason: format!("{connection_id}:HQ_POOL_UNAVAILABLE"),
                };
            }
        }
    } else {
        office.acquire_turn_permit(
            connection_id,
            employee_id,
            task_id,
            &cfg.model_id,
            authority,
            false,
        )
    };
    match permit_res {
        Ok(Ok(PermitAcquireOutcome::Granted(p))) => {
            WarmPermitOutcome::Granted((p.id, p.coordinator_generation))
        }
        Ok(Ok(PermitAcquireOutcome::CoolingDown { cooldown_until })) => {
            WarmPermitOutcome::Unavailable {
                multi_connection,
                reason: format!("{connection_id}:COOLDOWN({cooldown_until})"),
            }
        }
        Ok(Ok(PermitAcquireOutcome::CapacityFull { occupied, max })) => {
            WarmPermitOutcome::Unavailable {
                multi_connection,
                reason: format!("{connection_id}:CAPACITY_FULL({occupied}/{max})"),
            }
        }
        Ok(Err(api_err)) => WarmPermitOutcome::Refused(api_err),
        Err(_) => WarmPermitOutcome::Refused(ApiError::new(
            ErrorCode::StorageError,
            "could not acquire turn permit",
        )),
    }
}
