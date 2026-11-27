//! Pending BYOA authorization listing and server-computed workspace familiarity (`TASK-248`).

use org_office::organization_family::records::{Employee, EmployeeKind};
use org_proto::external_auth::{MemberExternalPendingResult, PendingExternalAuthView};
use org_proto::{ApiError, ErrorCode};

use crate::credentials::{format_rfc3339_ms, now_ms};
use crate::external::{
    compute_workspace_familiarity, decode_pubkey, pubkey_fingerprint, PendingSnapshot,
};
use crate::manager::SessionManager;

/// Maximum history entries inspected per agent when computing workspace familiarity (bounded
/// further by `MAX_HISTORY_TAIL_BYTES` in `SessionStore::read_external_history`).
const FAMILIARITY_HISTORY_LIMIT: usize = 500;

impl SessionManager {
    /// `member.external_pending`: lists live pending BYOA sign-in requests across the office
    /// (or filtered to one agent), including masked `code_prefix` (`XXXX-••••`) and
    /// server-computed `workspace_familiarity`. Strictly human-only (`authorize_owner_or_person`).
    pub fn external_pending(
        &self,
        actor: &str,
        employee_id: Option<&str>,
    ) -> Result<MemberExternalPendingResult, ApiError> {
        self.authorize_owner_or_person_inner(actor, false)?;
        let filter_id = match employee_id {
            Some(target) => {
                let employee = self.require_agent(target)?;
                self.authorize_external_operator(actor, &employee)?;
                Some(employee.id.as_str().to_owned())
            }
            None => None,
        };
        // Snapshot live pending logins and drop the lock before reading office state or disk.
        let snapshots = self
            .external
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .list_live(filter_id.as_deref(), now_ms());
        let mut pending = Vec::with_capacity(snapshots.len());
        for snap in snapshots {
            let Ok(Some(employee)) = self.office.load_employee(&snap.employee_id) else {
                continue;
            };
            if employee.kind != EmployeeKind::Agent {
                continue;
            }
            if self.authorize_external_operator(actor, &employee).is_err() {
                continue;
            }
            pending.push(self.build_pending_view(&employee, snap)?);
        }
        Ok(MemberExternalPendingResult { pending })
    }

    fn build_pending_view(
        &self,
        employee: &Employee,
        snap: PendingSnapshot,
    ) -> Result<PendingExternalAuthView, ApiError> {
        let fingerprint = decode_pubkey(&snap.pubkey_ed25519_b64)
            .map(|k| pubkey_fingerprint(&k))
            .unwrap_or_default();
        let history = self
            .store
            .read_external_history(employee.id.as_str(), FAMILIARITY_HISTORY_LIMIT)
            .map_err(|_| {
                ApiError::new(
                    ErrorCode::StorageError,
                    "could not read the external session history",
                )
            })?;
        let (workspace_familiarity, last_approved_by, last_approved_at) =
            compute_workspace_familiarity(
                &history,
                snap.hostname.as_deref(),
                snap.workspace_path.as_deref(),
                &snap.workspace_label,
                &fingerprint,
            );
        let handle = (!employee.handle.is_empty())
            .then(|| format!("@{}", employee.handle.trim_start_matches('@')));
        let info = self.office.info();
        Ok(PendingExternalAuthView {
            request_id: snap.request_id,
            handle,
            employee_id: employee.id.as_str().to_owned(),
            office_id: info.office_id.clone(),
            office_name: info.office_name.clone(),
            hostname: snap.hostname,
            workspace_path: snap.workspace_path,
            workspace_label: snap.workspace_label,
            pubkey_fingerprint: fingerprint,
            managed_turn_active: self.has_active_managed_turn(employee.id.as_str()),
            requested_at: format_rfc3339_ms(snap.requested_ms),
            expires_at: format_rfc3339_ms(snap.expires_ms),
            code_prefix: snap.code_prefix,
            workspace_familiarity,
            last_approved_by,
            last_approved_at,
        })
    }
}
