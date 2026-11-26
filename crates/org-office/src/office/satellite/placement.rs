//! Employee placement across offices and the local task write authority it implies.

use crate::office::{storage, Office};
use crate::{error::OfficeError, model::OfficeRole};

use super::work_ingest::ReadOfficeCodesOp;

pub(super) struct PlacementContext {
    pub(super) hq_office_id: String,
    pub(super) enrolled: std::collections::BTreeSet<String>,
    pub(super) removed_offices: std::collections::BTreeSet<String>,
    pub(super) employee_offices: std::collections::BTreeMap<String, String>,
}

struct ReadPlacementContextOp;

impl crate::store::open::StoreOp for ReadPlacementContextOp {
    type Out = Result<PlacementContext, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        let (ctx, _) = store.read(|snap| {
            let offices = crate::organization::ports::OrganizationRead::offices(snap)
                .map_err(org_store::StoreError::Engine)?;
            let hq_office_id = offices
                .iter()
                .find(|o| o.role == crate::organization::records::OfficeRole::Hq)
                .map(|o| o.id.as_str().to_owned())
                .unwrap_or_default();
            let enrolled: std::collections::BTreeSet<String> =
                offices.iter().map(|o| o.id.as_str().to_owned()).collect();
            let hires = crate::organization::ports::OrganizationRead::hire_requests(snap)
                .map_err(org_store::StoreError::Engine)?;
            let mut employee_offices = std::collections::BTreeMap::new();
            for req in hires {
                if req.state == crate::organization::records::HireState::Hired {
                    if let (Some(emp_id), Some(oid)) = (req.employee_id, req.proposal.office_id) {
                        if enrolled.contains(&oid) {
                            employee_offices.insert(emp_id.as_str().to_owned(), oid);
                        }
                    }
                }
            }
            Ok(PlacementContext {
                hq_office_id,
                enrolled,
                removed_offices: std::collections::BTreeSet::new(),
                employee_offices,
            })
        })?;
        Ok(ctx)
    }
}

pub(super) fn load_placement_context_locked(
    office: &Office,
) -> Result<PlacementContext, OfficeError> {
    let mut ctx = office.store.run(ReadPlacementContextOp).map_err(storage)?;
    if ctx.hq_office_id.is_empty() {
        ctx.hq_office_id = office
            .replication
            .org_checkpoint()
            .map(|cp| cp.hq_office.as_str().to_owned())
            .unwrap_or_else(|_| office.info.office_id.clone());
    }
    let removed = crate::office::invitations::removed_satellite_offices_locked(office)?;
    for oid in &removed {
        ctx.enrolled.remove(oid);
    }
    ctx.removed_offices = removed;
    Ok(ctx)
}

pub(super) fn load_placement_context(office: &Office) -> Result<PlacementContext, OfficeError> {
    let _guard = office
        .federation_io_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    load_placement_context_locked(office)
}

/// Resolves the enrolled office where `employee_id` is placed (explicit hired agent placement,
/// falling back to HQ for persons and unplaced agents, `TASK-52` P3-#1 / P3-#11).
pub fn resolve_employee_office(office: &Office, employee_id: &str) -> Result<String, OfficeError> {
    let ctx = load_placement_context(office)?;
    Ok(ctx
        .employee_offices
        .get(employee_id)
        .cloned()
        .unwrap_or(ctx.hq_office_id))
}

/// Returns whether `office` has local write authority for a task numbered with `task_office_code`
/// (`TASK-52` P3-#9 / P4-#6 / P5-#7): satellites may only modify their own assigned codes, while HQ
/// may modify its own codes AND retired/orphaned codes (such as R1/D25 recovered tasks from a lost
/// HQ), refusing only tasks owned by an active satellite's current code.
pub(crate) fn has_local_task_write_authority(
    office: &Office,
    task_office_code: &str,
) -> Result<bool, OfficeError> {
    let (codes, _) = office.store.read(ReadOfficeCodesOp).map_err(storage)?;
    if office.info.role == OfficeRole::Satellite {
        return Ok(codes
            .iter()
            .any(|c| c.office_id.as_str() == office.info.office_id && c.code == task_office_code));
    }
    let enrolled = office.enrolled_office_ids().unwrap_or_default();
    let removed = crate::office::invitations::removed_satellite_offices(office)?;
    let owned_by_active_satellite = codes.iter().any(|c| {
        c.code == task_office_code
            && c.retired_at.is_none()
            && c.office_id.as_str() != office.info.office_id
            && enrolled.iter().any(|id| id == c.office_id.as_str())
            && !removed.contains(c.office_id.as_str())
    });
    Ok(!owned_by_active_satellite)
}
