//! Session permits: the HQ permit-sync handler and the satellite's HQ permit coordinator.

use std::net::TcpStream;
use std::sync::Arc;

use org_office::Office;
use org_sync_proto::SyncFrame;
use rustls::{ServerConnection, StreamOwned};

use super::{now_ms, HqPermitRequest, SatelliteSyncSession};
use crate::link::hq_peer::{write_sync_frame_if_peer_active, HqSessionPeer};

pub(super) struct PermitSyncInput {
    pub(super) request_id: String,
    pub(super) connection_id: String,
    pub(super) employee_id: String,
    pub(super) task_id: Option<String>,
    pub(super) model_id: String,
    pub(super) is_failover_switch: bool,
}

pub(super) fn handle_hq_permit_sync(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    input: PermitSyncInput,
) -> bool {
    let Ok(Some(outcome)) = org_office::run_if_peer_active(
        peer.office,
        peer.sat_id.as_str(),
        Some(peer.cert_serial_hex),
        || {
            peer.office
                .acquire_turn_permit_for_office(org_office::OfficePermitSpec {
                    target_office_id: peer.sat_id.as_str(),
                    connection_id: &input.connection_id,
                    employee_id: &input.employee_id,
                    task_id: input.task_id.as_deref(),
                    model_id: &input.model_id,
                    authority: org_proto::providers::PermitAuthority::HqPool,
                    is_failover_switch: input.is_failover_switch,
                })
        },
    ) else {
        return false;
    };
    let resp = match outcome {
        Ok(Ok(org_office::providers::PermitAcquireOutcome::Granted(permit))) => {
            SyncFrame::PermitSyncResponse {
                request_id: input.request_id,
                granted: true,
                permit_id: Some(permit.id),
                coordinator_generation: Some(permit.coordinator_generation),
                reason: None,
                cooldown_until: None,
                occupied: None,
                max_capacity: None,
            }
        }
        Ok(Ok(org_office::providers::PermitAcquireOutcome::CoolingDown { cooldown_until })) => {
            SyncFrame::PermitSyncResponse {
                request_id: input.request_id,
                granted: false,
                permit_id: None,
                coordinator_generation: None,
                reason: Some(format!("cooling down until {cooldown_until}")),
                cooldown_until: Some(cooldown_until),
                occupied: None,
                max_capacity: None,
            }
        }
        Ok(Ok(org_office::providers::PermitAcquireOutcome::CapacityFull { occupied, max })) => {
            SyncFrame::PermitSyncResponse {
                request_id: input.request_id,
                granted: false,
                permit_id: None,
                coordinator_generation: None,
                reason: Some(format!("capacity full ({occupied}/{max})")),
                cooldown_until: None,
                occupied: Some(occupied),
                max_capacity: Some(max),
            }
        }
        Ok(Err(e)) => SyncFrame::PermitSyncResponse {
            request_id: input.request_id,
            granted: false,
            permit_id: None,
            coordinator_generation: None,
            reason: Some(e.message),
            cooldown_until: None,
            occupied: None,
            max_capacity: None,
        },
        Err(e) => SyncFrame::PermitSyncResponse {
            request_id: input.request_id,
            granted: false,
            permit_id: None,
            coordinator_generation: None,
            reason: Some(e.to_string()),
            cooldown_until: None,
            occupied: None,
            max_capacity: None,
        },
    };
    write_sync_frame_if_peer_active(tls, peer, &resp)
}

/// Production satellite adapter connecting `org-office-sessions`'s router to HQ's shared `HqPool`
/// permit coordinator over Org-CA mTLS (`TASK-52` M-21).
pub struct SatelliteHqPermitCoordinator {
    office: Arc<Office>,
}

impl SatelliteHqPermitCoordinator {
    pub fn new(office: Arc<Office>) -> Self {
        Self { office }
    }
}

impl org_office_sessions::HqPermitCoordinator for SatelliteHqPermitCoordinator {
    fn acquire_permit(
        &self,
        req: org_office_sessions::HqPermitAcquireSpec,
    ) -> Result<org_office::providers::PermitAcquireOutcome, String> {
        let mut session = SatelliteSyncSession::connect(Arc::clone(&self.office))?;
        let resp = session.request_hq_permit(HqPermitRequest {
            request_id: self.office.next_uuid_v7(),
            connection_id: req.connection_id.clone(),
            employee_id: req.employee_id.clone(),
            task_id: req.task_id.clone(),
            model_id: req.model_id.clone(),
            is_failover_switch: req.is_failover_switch,
        })?;
        if resp.granted {
            let permit_id = resp
                .permit_id
                .ok_or_else(|| "HQ granted permit without permit_id".to_owned())?;
            let now_rfc = org_office::format_rfc3339_ms(now_ms());
            return Ok(org_office::providers::PermitAcquireOutcome::Granted(
                org_office::providers::TurnPermitRecord {
                    organization_id: self.office.info().org_id.clone(),
                    id: permit_id,
                    connection_id: req.connection_id,
                    office_id: self.office.info().office_id.clone(),
                    employee_id: req.employee_id,
                    placement_generation: 1,
                    model_id: req.model_id,
                    authority: org_proto::providers::PermitAuthority::HqPool,
                    state: org_proto::providers::TurnPermitState::Active,
                    issued_at: now_rfc.clone(),
                    expires_at: now_rfc,
                    coordinator_generation: resp.coordinator_generation.unwrap_or(1),
                },
            ));
        }
        if let Some(until) = resp.cooldown_until {
            return Ok(org_office::providers::PermitAcquireOutcome::CoolingDown {
                cooldown_until: until,
            });
        }
        Ok(org_office::providers::PermitAcquireOutcome::CapacityFull {
            occupied: resp.occupied.unwrap_or(1),
            max: resp.max_capacity.unwrap_or(1),
        })
    }

    fn renew_permit(&self, permit_id: &str, coordinator_generation: u64) -> Result<bool, String> {
        let mut session = SatelliteSyncSession::connect(Arc::clone(&self.office))?;
        session.renew_hq_permit(
            self.office.next_uuid_v7(),
            permit_id.to_owned(),
            coordinator_generation,
        )
    }

    fn release_permit(&self, permit_id: &str) -> Result<(), String> {
        let mut session = SatelliteSyncSession::connect(Arc::clone(&self.office))?;
        session.release_hq_permit(permit_id.to_owned())
    }

    fn report_cooldown(
        &self,
        connection_id: &str,
        resets_at: Option<&str>,
        reason: Option<&str>,
    ) -> Result<(), String> {
        let mut session = SatelliteSyncSession::connect(Arc::clone(&self.office))?;
        session.report_hq_cooldown(
            connection_id.to_owned(),
            resets_at.map(str::to_owned),
            reason.map(str::to_owned),
        )
    }
}
