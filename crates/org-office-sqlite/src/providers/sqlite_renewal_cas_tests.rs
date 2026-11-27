use super::*;

use org_proto::providers::{ConnectionReadinessStatus, DiscoveredModelEntitlement};

use org_office::providers::{
    OfficeConnectionBinding, OfficeConnectionModel, OfficeConnectionState, ProviderPoolStore,
};

fn sample_binding(revision: u64) -> OfficeConnectionBinding {
    OfficeConnectionBinding {
        organization_id: "org-1".into(),
        office_id: "off-1".into(),
        connection_id: "conn-1".into(),
        revision,
        auth_context_ref: "local-host-oauth".into(),
        runtime_binary_path: None,
        account_identity_ref: Some("sub:team".into()),
        updated_at: "2026-11-25T12:00:00.000Z".into(),
        last_operation_id: None,
        last_operation_hash: None,
    }
}

fn sample_state(
    status: ConnectionReadinessStatus,
    revision: u64,
    until: Option<&str>,
) -> OfficeConnectionState {
    OfficeConnectionState {
        organization_id: "org-1".into(),
        office_id: "off-1".into(),
        connection_id: "conn-1".into(),
        status,
        binding_revision: revision,
        driver_version: Some("2.1.332".into()),
        observed_at: Some("2026-11-25T12:00:00.000Z".into()),
        valid_until: until.map(str::to_owned),
        issue_code: None,
    }
}

fn sample_model(until: &str) -> OfficeConnectionModel {
    OfficeConnectionModel {
        organization_id: "org-1".into(),
        office_id: "off-1".into(),
        connection_id: "conn-1".into(),
        model_id: "sonnet".into(),
        driver_model_id: "sonnet".into(),
        entitlement: DiscoveredModelEntitlement::Confirmed,
        capabilities_json: "{}".into(),
        observed_at: "2026-11-25T12:00:00.000Z".into(),
        valid_until: until.into(),
    }
}

#[test]
fn sqlite_renew_ready_observation_refuses_promotion_after_auth_expired_or_revision_bump() {
    let dir = tempfile::tempdir().unwrap();
    let durable = dir.path().join("durable");
    let runtime = dir.path().join("runtime");
    std::fs::create_dir_all(&durable).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    let store = SqliteProviderPoolStore::open(&durable, &runtime, "off-1", "org-1", true).unwrap();

    store
        .save_binding_and_invalidate(&sample_binding(1))
        .unwrap();
    let initial = sample_state(
        ConnectionReadinessStatus::Ready,
        1,
        Some("2026-11-25T12:05:00.000Z"),
    );
    store
        .save_observation(&initial, &[sample_model("2026-11-25T12:05:00.000Z")])
        .unwrap();

    let renewed = sample_state(
        ConnectionReadinessStatus::Ready,
        1,
        Some("2026-11-25T12:10:00.000Z"),
    );
    assert!(store
        .renew_ready_observation_if_unchanged(
            1,
            &renewed,
            &[sample_model("2026-11-25T12:10:00.000Z")],
        )
        .unwrap());
    assert_eq!(
        store
            .get_state("org-1", "off-1", "conn-1")
            .unwrap()
            .unwrap()
            .valid_until
            .as_deref(),
        Some("2026-11-25T12:10:00.000Z")
    );

    let expired = sample_state(ConnectionReadinessStatus::AuthExpired, 1, None);
    store.save_observation(&expired, &[]).unwrap();
    assert!(!store
        .renew_ready_observation_if_unchanged(
            1,
            &renewed,
            &[sample_model("2026-11-25T12:15:00.000Z")],
        )
        .unwrap());
    assert_eq!(
        store
            .get_state("org-1", "off-1", "conn-1")
            .unwrap()
            .unwrap()
            .status,
        ConnectionReadinessStatus::AuthExpired
    );

    store
        .save_binding_and_invalidate(&sample_binding(2))
        .unwrap();
    let ready_v2 = sample_state(
        ConnectionReadinessStatus::Ready,
        2,
        Some("2026-11-25T12:20:00.000Z"),
    );
    store
        .save_observation(&ready_v2, &[sample_model("2026-11-25T12:20:00.000Z")])
        .unwrap();
    assert!(!store
        .renew_ready_observation_if_unchanged(
            1,
            &renewed,
            &[sample_model("2026-11-25T12:25:00.000Z")],
        )
        .unwrap());
    assert_eq!(
        store
            .get_state("org-1", "off-1", "conn-1")
            .unwrap()
            .unwrap()
            .binding_revision,
        2
    );
}
