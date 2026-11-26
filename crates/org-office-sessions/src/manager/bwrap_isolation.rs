//! Linux Claude `bwrap` directory pre-creation and cross-agent sibling isolation (`TASK-144`,
//! `TASK-163`, `TASK-166`).
//!
//! Kept in its own focused module so all `bwrap` deny-dir setup, team-tree pre-creation,
//! stale-handle teardown, and pre-spawn sibling verification flow through one single
//! choke-point; teardown itself goes through [`super::reap`].

use std::collections::BTreeSet;
use std::path::Path;

use org_office::Office;
use org_proto::sessions::SessionState;
use org_proto::{ApiError, ErrorCode};

use super::environment::ensure_owner_only_dir;
use super::office_masks::OfficeMasks;
use super::reap::{reap_live_handle, ReapMode};
use super::sandbox_rules::collect_self_active_admins;
use super::{LiveSessionHandle, SessionManager, StartSlotGuard};

/// Pre-creates fixed `0700` sensitive office and sibling directories before launching a Linux
/// Claude `bwrap` sandbox so `bwrap` bind-mounts atomic directory masks without leaving unmasked
/// holes or creating 0-byte `0444` files (`TASK-144`, `TASK-166`). Fails closed on any filesystem
/// error (zero silent swallows). Never creates `<durable>/private` on satellites (`ARCH-02`).
pub(crate) fn ensure_bwrap_deny_dirs(
    mgr: &SessionManager,
    employee_id: &str,
) -> Result<BTreeSet<String>, ApiError> {
    let durable = mgr.office.durable_root();
    let office_id = &mgr.office.info().office_id;
    let office_local = durable.join("local").join("offices").join(office_id);
    let runtime_root = mgr.office.runtime_root();
    let office_rt = runtime_root.join(office_id);
    // Every durable family (`private` HQ-only, `ARCH-02`), every companion store folder (a
    // one-file store such as `hq-owner.v1.json` is masked only where it exists) and the
    // restore backups beside the data roots (`TASK-203`).
    for dir in OfficeMasks::of(&mgr.office).pre_created() {
        ensure_owner_only_dir(dir)?;
    }
    ensure_owner_only_dir(&mgr.office.folder().join("git-credentials"))?;
    ensure_owner_only_dir(&office_local.join("agents"))?;
    for dir in [
        office_rt.join("protected"),
        office_rt.join("git-home"),
        runtime_root.join("snapshot-import"),
        runtime_root.join("snapshot-export"),
    ] {
        ensure_owner_only_dir(&dir)?;
    }
    // `agent-os` external sign-in state (`TASK-137`): masked even before the first
    // `agent-os login` creates it, so a key written mid-session never appears unmasked.
    for root in &mgr.config.agent_os_state_roots {
        ensure_owner_only_dir(root)?;
    }
    ensure_bwrap_repo_dirs(runtime_root, &office_rt, office_id, employee_id)?;
    ensure_bwrap_sibling_dirs(mgr, &office_local, &office_rt, office_id, employee_id)
}

fn ensure_bwrap_repo_dirs(
    runtime_root: &Path,
    office_rt: &Path,
    office_id: &str,
    employee_id: &str,
) -> Result<(), ApiError> {
    let repos_root = office_rt.join("repositories");
    let self_admins = org_office::git::OfficeGitState::load(runtime_root, office_id)
        .ok()
        .map(|gs| collect_self_active_admins(&gs, employee_id))
        .unwrap_or_default();
    if self_admins.is_empty() {
        return ensure_owner_only_dir(&repos_root);
    }
    for repo_id in self_admins.keys() {
        let objects_dir = repos_root.join(format!("{repo_id}.git")).join("objects");
        if objects_dir.is_dir() {
            ensure_owner_only_dir(&objects_dir.join("info"))?;
        }
    }
    Ok(())
}

fn ensure_bwrap_sibling_dirs(
    mgr: &SessionManager,
    office_local: &Path,
    office_rt: &Path,
    office_id: &str,
    employee_id: &str,
) -> Result<BTreeSet<String>, ApiError> {
    let agents_root = office_local.join("agents");
    let sibling_ids = mgr.collect_sibling_agent_ids(&agents_root, Some(employee_id));
    for other in &sibling_ids {
        ensure_owner_only_dir(&agents_root.join(other))?;
    }
    if let Some(sock_parent) = mgr.config.office_socket.parent().filter(|p| p.is_dir()) {
        let short_root = sock_parent.join("t");
        ensure_owner_only_dir(&short_root)?;
        let own_slug = crate::host_auth::agent_tmpdir_slug(office_id, employee_id);
        for other in &sibling_ids {
            let slug = crate::host_auth::agent_tmpdir_slug(office_id, other);
            if slug != own_slug {
                ensure_owner_only_dir(&short_root.join(slug))?;
            }
        }
    }
    ensure_bwrap_team_dirs(mgr, office_rt, employee_id, &sibling_ids)?;
    Ok(sibling_ids)
}

fn ensure_bwrap_team_dirs(
    mgr: &SessionManager,
    office_rt: &Path,
    employee_id: &str,
    sibling_ids: &BTreeSet<String>,
) -> Result<(), ApiError> {
    let Some((chain, all_team_ids)) = mgr.resolve_employee_team_chain_and_ids(employee_id) else {
        return ensure_owner_only_dir(&office_rt.join("teams"));
    };
    let mut cursor = office_rt.to_path_buf();
    for (idx, team_id) in chain.iter().enumerate() {
        let teams_dir = cursor.join("teams");
        for other_team_id in &all_team_ids {
            if other_team_id != team_id {
                ensure_owner_only_dir(&teams_dir.join(other_team_id))?;
            }
        }
        cursor = teams_dir.join(team_id);
        if idx + 1 < chain.len() {
            ensure_owner_only_dir(&cursor.join("agents"))?;
        }
    }
    ensure_owner_only_dir(&cursor.join("teams"))?;
    let rt_agents_dir = cursor.join("agents");
    ensure_owner_only_dir(&rt_agents_dir)?;
    for other in sibling_ids {
        ensure_owner_only_dir(&rt_agents_dir.join(other))?;
    }
    Ok(())
}

/// Returns `true` when any enrolled employee or on-disk agent directory other than
/// `self_employee_id` is absent from `masked`.
pub(crate) fn has_unmasked_bwrap_sibling_in_office(
    office: &Office,
    self_employee_id: &str,
    masked: &BTreeSet<String>,
) -> bool {
    let office_id = &office.info().office_id;
    let agents_root = office
        .durable_root()
        .join("local")
        .join("offices")
        .join(office_id)
        .join("agents");
    match std::fs::read_dir(&agents_root) {
        Ok(entries) => {
            for entry in entries {
                let Ok(entry) = entry else {
                    return true; // Fail closed on per-entry readdir I/O error.
                };
                if let Some(name) = entry.file_name().to_str() {
                    if name != self_employee_id && !masked.contains(name) {
                        return true;
                    }
                }
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return true, // Fail closed on unexpected I/O error reading sibling agent dir.
    }
    let Ok(employees) = office.list_employees() else {
        return true; // Fail closed on unexpected DB/store error listing enrolled employees.
    };
    for emp in employees {
        let id = emp.id.as_str();
        if id != self_employee_id && !masked.contains(id) {
            return true;
        }
    }
    false
}

/// Verifies right before spawning a Linux `bwrap` child that no new sibling appeared in the
/// registry or on disk since `ensure_bwrap_deny_dirs` ran.
pub(crate) fn verify_bwrap_masked_before_spawn(
    mgr: &SessionManager,
    emp_id: &str,
    bwrap_masked_siblings: Option<&BTreeSet<String>>,
) -> Result<(), ApiError> {
    let Some(masked) = bwrap_masked_siblings else {
        return Ok(());
    };
    let raced = {
        let reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        reg.live
            .keys()
            .chain(reg.starting.iter())
            .any(|other| other != emp_id && !masked.contains(other))
    } || has_unmasked_bwrap_sibling_in_office(&mgr.office, emp_id, masked);
    if raced {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "a newly hired or starting sibling agent is not masked in this sandbox; retry launch",
        ));
    }
    Ok(())
}

struct StaleBwrapBatch<'a> {
    handles: Vec<(String, LiveSessionHandle)>,
    guards: Vec<StartSlotGuard<'a>>,
}

fn collect_stale_bwrap_siblings<'a>(
    mgr: &'a SessionManager,
    starting_emp_id: &str,
) -> Result<StaleBwrapBatch<'a>, ApiError> {
    let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(other) = reg
        .starting
        .iter()
        .find(|id| id.as_str() != starting_emp_id)
    {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            format!(
                "cannot start `{starting_emp_id}` while sibling `{other}` is starting; retry shortly"
            ),
        ));
    }
    let mut stale_idle_ids = Vec::new();
    for (other_id, handle) in &reg.live {
        if other_id == starting_emp_id {
            continue;
        }
        let Some(masked) = &handle.bwrap_masked_siblings else {
            continue;
        };
        if masked.contains(starting_emp_id) {
            continue;
        }
        let st = handle.state.lock().unwrap_or_else(|p| p.into_inner());
        if st.recovery_required
            || matches!(
                st.state,
                SessionState::Starting
                    | SessionState::Running
                    | SessionState::WaitingApproval
                    | SessionState::RecoveryRequired
            )
        {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                format!(
                    "cannot start `{starting_emp_id}` while active or unrecovered sandboxed session `{other_id}` does not isolate it; wait for its turn to finish or resolve `{other_id}` first"
                ),
            ));
        }
        stale_idle_ids.push(other_id.clone());
    }
    let mut removed = Vec::with_capacity(stale_idle_ids.len());
    let mut guards = Vec::with_capacity(stale_idle_ids.len());
    for id in stale_idle_ids {
        if let Some(handle) = reg.live.remove(&id) {
            reg.starting.insert(id.clone());
            guards.push(StartSlotGuard::new(&mgr.registry, id.clone()));
            removed.push((id, handle));
        }
    }
    Ok(StaleBwrapBatch {
        handles: removed,
        guards,
    })
}

fn reap_stale_bwrap_handle(
    mgr: &SessionManager,
    starting_emp_id: &str,
    other_id: String,
    handle: LiveSessionHandle,
) -> Result<(), ApiError> {
    reap_live_handle(
        mgr,
        &other_id,
        handle,
        Some("bwrap_sibling_isolation_refresh"),
        ReapMode::FailClosedRestore {
            conflict_message: format!(
                "could not verify teardown of stale sandboxed session `{other_id}` before starting `{starting_emp_id}`; resolve recovery on `{other_id}` first"
            ),
        },
    )?;
    Ok(())
}

/// Ensures no live or starting Linux `bwrap` session started before `starting_emp_id` existed
/// can read `starting_emp_id`'s newly created directory or `session-binding-*.v1` token
/// (`TASK-166`). Stale non-working `bwrap` siblings are guarded in `reg.starting`, stopped,
/// revoked, reaped, and permit-cleaned before launch proceeds; any stale actively-working,
/// unrecovered (`RecoveryRequired`), failed-reap, or concurrently-starting sibling fails launch
/// closed with `Conflict`.
pub(crate) fn enforce_bwrap_sibling_isolation(
    mgr: &SessionManager,
    starting_emp_id: &str,
) -> Result<(), ApiError> {
    let batch = collect_stale_bwrap_siblings(mgr, starting_emp_id)?;
    let mut first_err: Option<ApiError> = None;
    for (other_id, handle) in batch.handles {
        if let Err(err) = reap_stale_bwrap_handle(mgr, starting_emp_id, other_id, handle) {
            first_err.get_or_insert(err);
        }
    }
    drop(batch.guards);
    if let Some(err) = first_err {
        return Err(err);
    }
    Ok(())
}
