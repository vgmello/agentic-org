//! Expired, rejected, and unavailable paths, strict parsing, destination binding, and endpoint
//! path joining.

use super::*;

#[test]
fn expired_rejected_and_unavailable_error_paths() {
    let fx = SubUsageFixture::new();
    let conn_id = fx.create_connection(
        "claude-main",
        "anthropic",
        ProviderAuthMode::SubscriptionCli,
        Some("sub-anthropic-main"),
        None,
    );

    fx.mgr
        .subscription_usage_credential_set(
            &fx.owner,
            CostSubscriptionUsageCredentialSetParams {
                connection_id: conn_id.clone(),
                secret: "sk-ant-oat-expired-token".into(),
                expires_at: Some("2020-01-01T00:00:00Z".into()),
                account_hint: None,
                expected_origin: None,
            },
        )
        .unwrap();
    let expired_res = fx
        .mgr
        .subscription_usage_collect(
            &fx.owner,
            CostSubscriptionUsageCollectParams {
                connection_id: Some(conn_id.clone()),
                time_zone: None,
            },
        )
        .unwrap();
    assert_eq!(fx.transport.call_count(), 0);
    assert_eq!(
        expired_res.snapshots[0].status,
        SubscriptionUsageStatus::AuthRequired
    );
    assert_eq!(
        expired_res.snapshots[0].issue_code.as_deref(),
        Some("CREDENTIAL_EXPIRED")
    );

    let revoked = fx
        .mgr
        .subscription_usage_credential_revoke(
            &fx.owner,
            CostSubscriptionUsageCredentialRevokeParams {
                connection_id: conn_id.clone(),
            },
        )
        .unwrap();
    assert!(!revoked.credential.configured);
    enroll_claude_fallback(&fx, &conn_id, b"sk-ant-oat-enrolled-fallback-12345");

    fx.transport
        .on_url("https://api.anthropic.com/api/oauth/usage", |_, _| {
            Ok(HttpUsageResponse {
                status: 401,
                body: b"{\"error\":\"unauthorized sk-ant-oat-enrolled-fallback-12345\"}".to_vec(),
            })
        });
    let rejected = fx
        .mgr
        .subscription_usage_collect(
            &fx.owner,
            CostSubscriptionUsageCollectParams {
                connection_id: Some(conn_id.clone()),
                time_zone: None,
            },
        )
        .unwrap();
    assert_eq!(
        rejected.snapshots[0].status,
        SubscriptionUsageStatus::AuthRequired
    );
    assert_eq!(
        rejected.snapshots[0].auth_source,
        SubscriptionUsageAuthSource::EnrolledSubscriptionCredential
    );
    assert_eq!(
        rejected.snapshots[0].issue_code.as_deref(),
        Some("AUTH_REJECTED")
    );
    assert!(!rejected.snapshots[0]
        .message
        .as_deref()
        .unwrap_or("")
        .contains("12345"));

    fx.transport
        .on_url("https://api.anthropic.com/api/oauth/usage", |_, _| {
            Ok(HttpUsageResponse {
                status: 503,
                body: b"service unavailable".to_vec(),
            })
        });
    let unavail = fx
        .mgr
        .subscription_usage_collect(
            &fx.owner,
            CostSubscriptionUsageCollectParams {
                connection_id: Some(conn_id),
                time_zone: None,
            },
        )
        .unwrap();
    assert_eq!(
        unavail.snapshots[0].status,
        SubscriptionUsageStatus::Unavailable
    );
    assert_eq!(
        unavail.snapshots[0].issue_code.as_deref(),
        Some("PROVIDER_UNAVAILABLE")
    );
}

#[test]
fn anthropic_short_term_window_and_last_ok_preservation_on_unavailable() {
    let fx = SubUsageFixture::new();
    let conn_id = fx.create_connection(
        "claude-windows",
        "anthropic",
        ProviderAuthMode::SubscriptionCli,
        Some("sub-anthropic-win"),
        None,
    );
    enroll_claude_fallback(&fx, &conn_id, b"sk-ant-oat-enrolled-win-54321");

    fx.transport
        .on_url("https://api.anthropic.com/api/oauth/usage", |_, _| {
            Ok(HttpUsageResponse {
                status: 200,
                body: br#"{
                    "five_hour": {"utilization": 14.5, "resets_at": "2026-11-21T23:00:00Z"},
                    "seven_day": {"utilization": 52.0, "resets_at": "2026-11-28T00:00:00Z"},
                    "seven_day_sonnet": {"utilization": 33.25, "resets_at": "2026-11-28T00:00:00Z"}
                }"#
                .to_vec(),
            })
        });
    let ok = fx
        .mgr
        .subscription_usage_collect(
            &fx.owner,
            CostSubscriptionUsageCollectParams {
                connection_id: Some(conn_id.clone()),
                time_zone: None,
            },
        )
        .unwrap();
    let snap = &ok.snapshots[0];
    assert_eq!(snap.status, SubscriptionUsageStatus::Ok);
    assert_eq!(snap.last_ok_collected_at, snap.collected_at);
    let sub_row = snap
        .models
        .iter()
        .find(|m| m.model == "claude-subscription")
        .unwrap();
    assert_eq!(
        sub_row.short_term.as_ref().and_then(|w| w.utilization_bps),
        Some(1_450)
    );
    assert_eq!(
        sub_row.short_term.as_ref().and_then(|w| w.window_seconds),
        Some(18_000)
    );
    assert_eq!(sub_row.daily.utilization_bps, None);
    let sonnet = snap
        .models
        .iter()
        .find(|m| m.model == "claude-sonnet")
        .unwrap();
    assert_eq!(sonnet.weekly.utilization_bps, Some(3_325));
    assert_eq!(sonnet.weekly.window_seconds, Some(604_800));

    // Transient failure preserves last_ok_collected_at & models, excluded from Ok rollups.
    fx.transport
        .on_url("https://api.anthropic.com/api/oauth/usage", |_, _| {
            Ok(HttpUsageResponse {
                status: 200,
                body: br#"{
                    "models": [
                        {"model": "dup-m", "total": {"requests": 1}},
                        {"model": "dup-m", "total": {"requests": 2}}
                    ]
                }"#
                .to_vec(),
            })
        });
    let dup = fx
        .mgr
        .subscription_usage_collect(
            &fx.owner,
            CostSubscriptionUsageCollectParams {
                connection_id: Some(conn_id),
                time_zone: None,
            },
        )
        .unwrap();
    assert_eq!(
        dup.snapshots[0].status,
        SubscriptionUsageStatus::Unavailable
    );
    assert_eq!(
        dup.snapshots[0].issue_code.as_deref(),
        Some("INVALID_PROVIDER_RESPONSE")
    );
    assert_eq!(dup.snapshots[0].models.len(), 2);
    assert_eq!(dup.snapshots[0].last_ok_collected_at, snap.collected_at);
}

#[test]
fn strict_parser_rejects_dup_keys_wrong_types_out_of_range_and_secret_reflection() {
    let fx = SubUsageFixture::new();
    let conn_id = fx.create_connection(
        "claude-strict",
        "anthropic",
        ProviderAuthMode::SubscriptionCli,
        Some("sub-strict"),
        None,
    );
    let secret = "sk-ant-oat-secret-reflection-99999";
    fx.mgr
        .subscription_usage_credential_set(
            &fx.owner,
            CostSubscriptionUsageCredentialSetParams {
                connection_id: conn_id.clone(),
                secret: secret.into(),
                expires_at: None,
                account_hint: None,
                expected_origin: None,
            },
        )
        .unwrap();
    let bad_payloads: &[&[u8]] = &[
        br#"{"models":[{"model":"a","total":{"requests":1}},{"model":"a","total":{"requests":2}}]}"#,
        br#"{"five_hour":{"utilization":10.0},"five_hour":{"utilization":20.0}}"#,
        br#"{"models":[{"model":"a","total":{"requests":"10"}}]}"#,
        br#"{"models":[{"model":"a","daily":{"requests":1,"window_seconds":18000}}]}"#,
        br#"{"models":[{"model":"a","weekly":{"requests":1,"window_seconds":86400}}]}"#,
        br#"{"models":[["m",{"requests":7},null,null,null]]}"#,
        br#"{"models":[{"model":"a","daily":{"requests":1,"starts_at":"2026-11-22T12:00:00Z","ends_at":"2026-11-22T00:00:00Z"}}]}"#,
        br#"{"seven_day":{"utilization":20.0,"window_seconds":86400}}"#,
        br#"{"five_hour":{"utilization":20.0,"window_seconds":86400}}"#,
        br#"{"five_hour":{"utilization":105.0}}"#,
        br#"{"five_hour":{"utilization":-1.0}}"#,
        br#"{"five_hour":{}}"#,
        br#"{"models":[{"model":"sk-ant-oat-secret-reflection-99999","total":{"requests":1}}]}"#,
    ];
    for payload in bad_payloads {
        let body = payload.to_vec();
        fx.transport
            .on_url("https://api.anthropic.com/api/oauth/usage", move |_, _| {
                Ok(HttpUsageResponse {
                    status: 200,
                    body: body.clone(),
                })
            });
        let res = fx
            .mgr
            .subscription_usage_collect(
                &fx.owner,
                CostSubscriptionUsageCollectParams {
                    connection_id: Some(conn_id.clone()),
                    time_zone: None,
                },
            )
            .unwrap();
        assert_eq!(
            res.snapshots[0].status,
            SubscriptionUsageStatus::Unavailable
        );
        assert_eq!(
            res.snapshots[0].issue_code.as_deref(),
            Some("INVALID_PROVIDER_RESPONSE")
        );
        assert!(res.snapshots[0].models.is_empty());
    }
}

#[test]
fn credential_destination_binding_blocks_custom_and_changed_origins() {
    let fx = SubUsageFixture::new();
    let custom_conn = fx.create_connection(
        "claude-custom-origin",
        "anthropic",
        ProviderAuthMode::SubscriptionCli,
        Some("sub-custom"),
        Some("https://proxy-a.example.com"),
    );
    // Enrolled credential is never sent to a custom non-official origin!
    enroll_claude_fallback(&fx, &custom_conn, b"sk-ant-oat-enrolled-custom");
    let mismatch = fx
        .mgr
        .subscription_usage_collect(
            &fx.owner,
            CostSubscriptionUsageCollectParams {
                connection_id: Some(custom_conn.clone()),
                time_zone: None,
            },
        )
        .unwrap();
    assert_eq!(fx.transport.call_count(), 0);
    assert_eq!(
        mismatch.snapshots[0].status,
        SubscriptionUsageStatus::AuthRequired
    );
    assert_eq!(
        mismatch.snapshots[0].issue_code.as_deref(),
        Some("ENDPOINT_CREDENTIAL_MISMATCH")
    );

    // Custom endpoint requires matching expected_origin (stale form or omitted origin -> Conflict).
    assert_eq!(
        fx.mgr
            .subscription_usage_credential_set(
                &fx.owner,
                CostSubscriptionUsageCredentialSetParams {
                    connection_id: custom_conn.clone(),
                    secret: "sk-ant-oat-bound-origin".into(),
                    expires_at: None,
                    account_hint: None,
                    expected_origin: None,
                },
            )
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        fx.mgr
            .subscription_usage_credential_set(
                &fx.owner,
                CostSubscriptionUsageCredentialSetParams {
                    connection_id: custom_conn.clone(),
                    secret: "sk-ant-oat-bound-origin".into(),
                    expires_at: None,
                    account_hint: None,
                    expected_origin: Some("https://wrong.example.com".into()),
                },
            )
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    // Dedicated usage credential bound to proxy-a.example.com fails closed if endpoint changes!
    fx.mgr
        .subscription_usage_credential_set(
            &fx.owner,
            CostSubscriptionUsageCredentialSetParams {
                connection_id: custom_conn.clone(),
                secret: "sk-ant-oat-bound-origin".into(),
                expires_at: None,
                account_hint: None,
                expected_origin: Some("https://proxy-a.example.com".into()),
            },
        )
        .unwrap();
    let rec = fx
        .office
        .load_provider_connections()
        .unwrap()
        .into_iter()
        .find(|c| c.id.as_str() == custom_conn.as_str())
        .unwrap();
    fx.office
        .provider_connection_update(
            &fx.owner,
            org_proto::providers::ProviderConnectionUpdateParams {
                command_id: next_uuid(),
                connection_id: custom_conn.clone(),
                expected_version: rec.meta.revision,
                name: None,
                endpoint_url: Some("https://proxy-b.example.com".into()),
                clear_endpoint_url: false,
                enabled: None,
                archived: None,
                max_concurrent_turns: None,
            },
        )
        .unwrap()
        .unwrap();
    let changed = fx
        .mgr
        .subscription_usage_collect(
            &fx.owner,
            CostSubscriptionUsageCollectParams {
                connection_id: Some(custom_conn),
                time_zone: None,
            },
        )
        .unwrap();
    assert_eq!(fx.transport.call_count(), 0);
    assert_eq!(
        changed.snapshots[0].status,
        SubscriptionUsageStatus::AuthRequired
    );
    assert_eq!(
        changed.snapshots[0].issue_code.as_deref(),
        Some("ENDPOINT_ORIGIN_CHANGED")
    );
}

#[test]
fn provider_alias_and_custom_query_endpoint_path_joining() {
    let fx = SubUsageFixture::new();
    let cc = fx.create_connection(
        "claude-code-alias",
        "claude-code",
        ProviderAuthMode::SubscriptionCli,
        Some("sub-cc-alias"),
        Some("https://proxy-q.example.com/gateway?region=eu"),
    );
    fx.mgr
        .subscription_usage_credential_set(
            &fx.owner,
            CostSubscriptionUsageCredentialSetParams {
                connection_id: cc.clone(),
                secret: "tok-cc-alias".into(),
                expires_at: None,
                account_hint: None,
                expected_origin: Some("https://proxy-q.example.com".into()),
            },
        )
        .unwrap();
    fx.transport.on_url(
        "https://proxy-q.example.com/gateway/api/oauth/usage?region=eu",
        |_, _| {
            Ok(HttpUsageResponse {
                status: 200,
                body: br#"{"five_hour":{"utilization":12.5}}"#.to_vec(),
            })
        },
    );
    let res = fx
        .mgr
        .subscription_usage_collect(
            &fx.owner,
            CostSubscriptionUsageCollectParams {
                connection_id: Some(cc),
                time_zone: None,
            },
        )
        .unwrap();
    assert_eq!(res.snapshots[0].status, SubscriptionUsageStatus::Ok);
    assert_eq!(
        res.snapshots[0].models[0]
            .short_term
            .as_ref()
            .and_then(|w| w.utilization_bps),
        Some(1_250)
    );
}
