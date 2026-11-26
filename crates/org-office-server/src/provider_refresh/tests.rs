use super::*;
use org_proto::providers::{
    DiscoveredModelEntitlement, OfficeConnectionBindingView, OfficeConnectionModelView,
    OfficeConnectionStateView,
};

fn sample_summary(
    status: ConnectionReadinessStatus,
    valid_until: Option<&str>,
    stale: bool,
) -> BoundConnectionSummary {
    BoundConnectionSummary {
        binding: OfficeConnectionBindingView {
            office_id: "off-1".into(),
            connection_id: "conn-1".into(),
            revision: 1,
            auth_context_ref: "local-host-oauth".into(),
            runtime_binary_path: None,
            account_identity_ref: Some("sub:test".into()),
            updated_at: "2026-11-25T12:00:00.000Z".into(),
        },
        state: Some(OfficeConnectionStateView {
            office_id: "off-1".into(),
            connection_id: "conn-1".into(),
            status,
            binding_revision: 1,
            driver_version: Some("2.1.332".into()),
            observed_at: Some("2026-11-25T12:00:00.000Z".into()),
            valid_until: valid_until.map(str::to_owned),
            issue_code: None,
            stale,
        }),
        models: vec![OfficeConnectionModelView {
            office_id: "off-1".into(),
            connection_id: "conn-1".into(),
            model_id: "sonnet".into(),
            driver_model_id: "sonnet".into(),
            entitlement: DiscoveredModelEntitlement::Confirmed,
            capabilities_json: "{}".into(),
            observed_at: "2026-11-25T12:00:00.000Z".into(),
            valid_until: valid_until.unwrap_or("2026-11-25T12:05:00.000Z").into(),
            stale,
        }],
    }
}

#[test]
fn needs_renewal_respects_status_and_expiry_window() {
    let now_ms = org_proto::parse_rfc3339_ms("2026-11-25T12:03:00.000Z").unwrap();
    // 120s remaining <= 150s renew window -> renew
    let soon = sample_summary(
        ConnectionReadinessStatus::Ready,
        Some("2026-11-25T12:05:00.000Z"),
        false,
    );
    assert!(needs_renewal(&soon, now_ms));

    // 240s remaining > 150s -> skip
    let fresh = sample_summary(
        ConnectionReadinessStatus::Ready,
        Some("2026-11-25T12:07:00.000Z"),
        false,
    );
    assert!(!needs_renewal(&fresh, now_ms));

    // Already stale -> renew
    let stale = sample_summary(
        ConnectionReadinessStatus::Ready,
        Some("2026-11-25T12:01:00.000Z"),
        true,
    );
    assert!(needs_renewal(&stale, now_ms));

    // AuthExpired / Unavailable must NEVER be auto-renewed as Ready
    let expired = sample_summary(ConnectionReadinessStatus::AuthExpired, None, true);
    assert!(!needs_renewal(&expired, now_ms));
    let unavailable = sample_summary(ConnectionReadinessStatus::Unavailable, None, true);
    assert!(!needs_renewal(&unavailable, now_ms));
}

#[test]
fn parse_version_output_extracts_first_token() {
    assert_eq!(
        parse_version_output(b"2.1.332 (Claude Code)\n", b"").as_deref(),
        Some("2.1.332")
    );
    assert_eq!(
        parse_version_output(b"", b"codex-cli 0.118.0\n").as_deref(),
        Some("codex-cli")
    );
    assert_eq!(parse_version_output(b"   \n", b""), None);
}

#[test]
fn probe_runtime_version_aborts_immediately_when_stop_is_set() {
    let stop = AtomicBool::new(true);
    let runtimes = RuntimeRegistry::production();
    assert!(probe_runtime_version(Some("/bin/sh"), "claude", &runtimes, &stop).is_none());

    let running = AtomicBool::new(false);
    assert!(probe_runtime_version(Some("./relative-bin"), "claude", &runtimes, &running).is_none());
}
