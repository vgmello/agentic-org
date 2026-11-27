//! The JSON contract of the command DTOs (TASK-188): `serde_json::to_value(&dto)` is what the
//! commands wrote under `--json` before they answered DTOs, with the keys the command line's
//! federation tests (`apps/agentic-org/tests/cli_federation.rs`) read. The ad-hoc answers keep
//! their earlier `json!` maps byte for byte, key order included.

use org_proto::federation::{
    OfficeInvitationActionResult, OfficeInvitationApproveResult, OfficeInvitationCreateResult,
    OfficeInvitationListResult, OfficeRemoveResult, OfficeSyncStatusResult, WireInvitationSummary,
};
use org_proto::{ListOfficesResult, OfficeInfo, OfficeRole, OfficeStatus};
use serde_json::{json, Value};

use super::cli_context::CliContext;
use super::*;

const ORG: &str = "0193a5c0-0000-7000-8000-000000000001";
const HQ: &str = "0193a5c0-0000-7000-8000-000000000002";
const SATELLITE: &str = "0193a5c0-0000-7000-8000-000000000003";
const OPERATION: &str = "0193a5c0-0000-7000-8000-000000000004";
const INVITATION: &str = "0193a5c0-0000-7000-8000-000000000005";
const INSTANCE: &str = "0193a5c0-0000-7000-8000-000000000006";

fn value(dto: &impl serde::Serialize) -> Value {
    serde_json::to_value(dto).unwrap()
}

fn office(id: &str, role: OfficeRole) -> OfficeInfo {
    OfficeInfo {
        id: id.into(),
        organization_id: ORG.into(),
        name: "Office".into(),
        role,
        host: "This Computer".into(),
        endpoint: None,
        status: OfficeStatus::Active,
        status_reason: None,
        next_restart_at: None,
    }
}

fn organization() -> OrganizationView {
    OrganizationView {
        id: ORG.into(),
        name: "Acme".into(),
        slug: "acme-cli".into(),
        created_at: "2026-11-26T08:00:00.000Z".into(),
        employee_id: "0193a5c0-0000-7000-8000-000000000007".into(),
        offices: vec![office(HQ, OfficeRole::Hq)],
        recovery: RecoveryState::Pending,
    }
}

fn invitation(status: &str, check_code: Option<&str>) -> WireInvitationSummary {
    WireInvitationSummary {
        invitation_id: INVITATION.into(),
        office_id: SATELLITE.into(),
        office_name: "Lab".into(),
        status: status.into(),
        check_code: check_code.map(str::to_owned),
        wrong_code_attempts: 0,
        created_at_ms: 1,
        expires_at_ms: 2,
        issued_cert_serial_hex: None,
    }
}

#[test]
fn org_create_answers_the_view_or_the_accepted_map_byte_for_byte() {
    let created = value(&OrgCreateOutcome::Created(organization()));
    assert_eq!(created, value(&organization()));
    assert_eq!(created["id"], ORG);
    assert_eq!(created["slug"], "acme-cli");
    assert_eq!(created["offices"][0]["id"], HQ);
    assert_eq!(created["recovery"], "pending");

    let accepted = OrgCreateOutcome::Accepted {
        operation_id: OPERATION.into(),
    };
    assert_eq!(
        serde_json::to_string_pretty(&accepted).unwrap(),
        serde_json::to_string_pretty(&json!({
            "status": "accepted",
            "operation_id": OPERATION,
        }))
        .unwrap()
    );
}

#[test]
fn org_and_office_lists_keep_their_keys() {
    let listed = value(&OrganizationList {
        organizations: vec![organization()],
        creating: vec![CreatingOrganization {
            operation_id: OPERATION.into(),
            name: "Next".into(),
            slug: "next".into(),
        }],
    });
    assert_eq!(listed["organizations"][0]["id"], ORG);
    assert_eq!(listed["creating"][0]["operation_id"], OPERATION);

    let offices = value(&ListOfficesResult {
        offices: vec![
            office(HQ, OfficeRole::Hq),
            office(SATELLITE, OfficeRole::Satellite),
        ],
    });
    assert_eq!(offices["offices"][0]["id"], HQ);
    assert_eq!(offices["offices"][0]["role"], "hq");
    assert_eq!(offices["offices"][1]["organization_id"], ORG);
    assert_eq!(offices["offices"][1]["role"], "satellite");
    assert_eq!(offices["offices"][1]["status"], "active");
    assert!(offices["offices"][0].get("endpoint").is_none());
}

#[test]
fn the_satellite_enrollment_answers_keep_their_keys() {
    let invited = value(&OfficeInvitationCreateResult {
        invitation: invitation("awaiting_approval", None),
        token: "one-time-token".into(),
    });
    assert_eq!(invited["invitation"]["invitation_id"], INVITATION);
    assert_eq!(invited["invitation"]["office_id"], SATELLITE);
    assert_eq!(invited["token"], "one-time-token");

    let joined = value(&OfficeJoinResult {
        office_id: SATELLITE.into(),
        org_id: ORG.into(),
        org_name: "Acme".into(),
        office_name: "Lab".into(),
        hq_office_id: HQ.into(),
        state: OfficeJoinState::JoinPending,
        check_code: Some("1234-5678".into()),
    });
    assert_eq!(joined["office_id"], SATELLITE);
    assert_eq!(joined["org_id"], ORG);
    assert_eq!(joined["state"], "join_pending");
    assert_eq!(joined["check_code"], "1234-5678");

    let listed = value(&OfficeInvitationListResult {
        invitations: vec![invitation("awaiting_approval", None)],
    });
    assert_eq!(listed["invitations"][0]["invitation_id"], INVITATION);
    assert_eq!(listed["invitations"][0]["status"], "awaiting_approval");
    assert!(listed["invitations"][0].get("check_code").is_none());

    let approved = value(&OfficeInvitationApproveResult {
        invitation: invitation("approved", None),
        office_id: SATELLITE.into(),
        slug: "lab".into(),
        office_code: "LAB".into(),
        cert_serial_hex: "0a1b".into(),
    });
    assert_eq!(approved["office_id"], SATELLITE);
    assert_eq!(approved["cert_serial_hex"], "0a1b");

    for status in ["cancelled", "declined"] {
        let acted = value(&OfficeInvitationActionResult {
            invitation: invitation(status, None),
        });
        assert_eq!(acted["invitation"]["status"], status);
    }

    let removed = value(&OfficeRemoveResult {
        office_id: SATELLITE.into(),
        revoked_serials: vec!["0a1b".into()],
    });
    assert_eq!(removed["office_id"], SATELLITE);
    assert_eq!(removed["revoked_serials"][0], "0a1b");

    let synced = value(&OfficeSyncStatusResult {
        org_id: ORG.into(),
        local_office: SATELLITE.into(),
        hq_office: HQ.into(),
        epoch: 1,
        seq: 2,
        highest_epoch: 1,
        protocol_level: 2,
        restore_pending: false,
        initial_snapshot_required: false,
        pending_work_count: 0,
        pending_message_count: 0,
    });
    assert_eq!(synced["local_office"], SATELLITE);
    assert_eq!(synced["hq_office"], HQ);
    assert_eq!(synced["initial_snapshot_required"], false);
}

#[test]
fn the_ad_hoc_answers_keep_their_earlier_maps_byte_for_byte() {
    let completed = OfficeOperationCompleted {
        office_id: HQ.into(),
        operation_id: OPERATION.into(),
        outcome: OfficeOperationOutcome::Completed,
    };
    assert_eq!(
        serde_json::to_string(&completed).unwrap(),
        serde_json::to_string(&json!({
            "operation_id": OPERATION,
            "office_id": HQ,
            "outcome": "completed",
        }))
        .unwrap()
    );

    let provider = ProviderCreated {
        connection_id: "c".into(),
        default_profile_id: "p".into(),
        generation: 3,
        revision: 4,
        name: "Claude".into(),
        key: "claude".into(),
    };
    assert_eq!(
        serde_json::to_string_pretty(&provider).unwrap(),
        serde_json::to_string_pretty(&json!({
            "connection_id": "c",
            "default_profile_id": "p",
            "revision": 4,
            "generation": 3,
        }))
        .unwrap()
    );
}

#[test]
fn the_new_answers_serialize_in_snake_case_words() {
    let context = CliContext::new(INSTANCE.into(), None, HQ.into()).unwrap();
    assert_eq!(
        value(&SignedIn {
            account: "Owner".into(),
            saved_context: Some(context),
        }),
        json!({
            "account": "Owner",
            "saved_context": { "instance_id": INSTANCE, "office_id": HQ },
        })
    );
    assert_eq!(
        value(&TlsPairReplaced {
            instance: "default".into(),
            origin: "https://org.example:7443".into(),
            sha256_fingerprint: "AB:CD".into(),
        }),
        json!({
            "instance": "default",
            "origin": "https://org.example:7443",
            "sha256_fingerprint": "AB:CD",
        })
    );
    assert_eq!(
        value(&ServiceReport {
            instance: "lab".into(),
            action: ServiceAction::Status,
            state: ServiceState::NotRegistered,
            pid: None,
            foreground: false,
            notes: Vec::new(),
        }),
        json!({
            "instance": "lab",
            "action": "status",
            "state": "not_registered",
            "pid": null,
            "foreground": false,
            "notes": [],
        })
    );
    assert_eq!(value(&ServiceState::StoppedByRequest), "stopped_by_request");
    assert_eq!(value(&ServiceAction::Start), "start");
}
