//! SSRF guards, authenticated collection and aggregation, revocation, partial rollups, and
//! duplicate subscription identities.

use super::*;

#[test]
fn ssrf_and_url_validation_blocks_non_https_local_and_private_endpoints() {
    assert!(validate_usage_endpoint_url("https://api.anthropic.com/api/oauth/usage").is_ok());
    assert!(validate_usage_endpoint_url("https://chatgpt.com/backend-api/wham/usage").is_ok());
    assert!(validate_usage_endpoint_url("http://api.anthropic.com/api/oauth/usage").is_err());
    assert!(validate_usage_endpoint_url("https://localhost/usage").is_err());
    assert!(validate_usage_endpoint_url("https://localhost./usage").is_err());
    assert!(validate_usage_endpoint_url("https://127.0.0.1:8443/usage").is_err());
    assert!(validate_usage_endpoint_url("https://127.0.0.1./usage").is_err());
    assert!(validate_usage_endpoint_url("https://2130706433/usage").is_err());
    assert!(validate_usage_endpoint_url("https://0x7f000001/usage").is_err());
    assert!(validate_usage_endpoint_url("https://0177.0.0.1/usage").is_err());
    assert!(validate_usage_endpoint_url("https://127.1/usage").is_err());
    assert!(validate_usage_endpoint_url("https://169.254.169.254/latest/meta-data").is_err());
    assert!(validate_usage_endpoint_url("https://10.0.0.5/usage").is_err());
    assert!(validate_usage_endpoint_url("https://[::1]/usage").is_err());
    assert!(validate_usage_endpoint_url("https://[::127.0.0.1]/usage").is_err());
    assert!(validate_usage_endpoint_url("https://[fe80::1%25en0]/usage").is_err());
    assert!(validate_usage_endpoint_url("https://[::1]evil/usage").is_err());
    assert!(validate_usage_endpoint_url("https://api.anthropic.com/bad space").is_err());
    assert!(validate_usage_endpoint_url("https://user:pass@api.anthropic.com/usage").is_err());
    assert!(validate_usage_endpoint_url("https://api.anthropic.com/usage\"injected").is_err());
    assert!(validate_usage_endpoint_url("https://api.anthropic.com/usage\nheader = x").is_err());
    assert!(validate_usage_endpoint_url("https://api.anthropic.com./api/oauth/usage").is_err());
    assert!(validate_usage_endpoint_url("https://.api.anthropic.com/api/oauth/usage").is_err());
    assert!(validate_usage_endpoint_url("https://api.anthropic.com/usage#frag").is_err());
    assert!(validate_usage_endpoint_url("https://[2606:4700:4700::1111]/usage").is_ok());

    // Debug formatting of credential-set params & HTTP responses never leaks raw secret/body.
    let params = CostSubscriptionUsageCredentialSetParams {
        connection_id: "c1".into(),
        secret: "sk-ant-oat-super-secret-value".into(),
        expires_at: None,
        account_hint: None,
        expected_origin: None,
    };
    let dbg = format!("{params:?}");
    assert!(dbg.contains("<redacted>"));
    assert!(!dbg.contains("super-secret-value"));
    let resp = HttpUsageResponse {
        status: 200,
        body: b"secret-token-body-xyz".to_vec(),
    };
    let resp_dbg = format!("{resp:?}");
    assert!(resp_dbg.contains("body_len"));
    assert!(!resp_dbg.contains("secret-token-body-xyz"));
}

#[test]
fn authenticated_collection_aggregation_auth_scoping_and_secret_isolation() {
    let fx = SubUsageFixture::new();
    let claude_a = fx.create_connection(
        "claude-sub-a",
        "anthropic",
        ProviderAuthMode::SubscriptionCli,
        Some("sub-anthropic-a"),
        None,
    );
    let claude_b = fx.create_connection(
        "claude-sub-b",
        "anthropic",
        ProviderAuthMode::SubscriptionCli,
        Some("sub-anthropic-b"),
        Some("https://proxy-b.example.com"),
    );
    let codex = fx.create_connection(
        "codex-sub",
        "openai",
        ProviderAuthMode::SubscriptionCli,
        Some("sub-openai-1"),
        None,
    );
    let google = fx.create_connection(
        "agy-sub",
        "google",
        ProviderAuthMode::SubscriptionCli,
        Some("sub-google-1"),
        None,
    );
    let api_key_conn = fx.create_connection(
        "api-key-only",
        "anthropic",
        ProviderAuthMode::ApiKey,
        None,
        None,
    );

    assert_non_owners_rejected(&fx, &claude_a);
    assert_eq!(
        fx.mgr
            .subscription_usage_credential_set(
                &fx.owner,
                CostSubscriptionUsageCredentialSetParams {
                    connection_id: api_key_conn,
                    secret: "sk-ant-api-nope".into(),
                    expires_at: None,
                    account_hint: None,
                    expected_origin: None,
                },
            )
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );

    let initial = fx
        .mgr
        .subscription_usage_list(&fx.owner, CostSubscriptionUsageListParams::default())
        .unwrap();
    assert!(!initial.is_api_equivalent_estimate);
    assert_eq!(initial.snapshots.len(), 4);
    assert!(initial.snapshots.iter().all(|s| {
        s.status == SubscriptionUsageStatus::AuthRequired
            && s.issue_code.as_deref() == Some("AUTH_MISSING")
            && !s.is_api_equivalent_estimate
    }));

    let secrets =
        seed_usage_credentials_and_verify_vault(&fx, &claude_a, &claude_b, &codex, &google);
    configure_mock_providers(&fx.transport);
    assert_collected_and_aggregated(&fx, &claude_a, &codex, &google);

    for secret in secrets {
        assert!(!dir_contains_bytes(
            fx.office.durable_root(),
            secret.as_bytes()
        ));
    }
}

#[test]
fn archived_connection_can_revoke_usage_credential_and_auth_missing_clears_on_set() {
    let fx = SubUsageFixture::new();
    let conn2 = fx.create_connection(
        "claude-archivable",
        "anthropic",
        ProviderAuthMode::SubscriptionCli,
        Some("sub-anthropic-archivable"),
        None,
    );
    let missing = fx
        .mgr
        .subscription_usage_collect(
            &fx.owner,
            CostSubscriptionUsageCollectParams {
                connection_id: Some(conn2.clone()),
                time_zone: None,
            },
        )
        .unwrap();
    assert_eq!(
        missing.snapshots[0].issue_code.as_deref(),
        Some("AUTH_MISSING")
    );
    fx.mgr
        .subscription_usage_credential_set(
            &fx.owner,
            CostSubscriptionUsageCredentialSetParams {
                connection_id: conn2.clone(),
                secret: "sk-ant-oat-archivable-secret".into(),
                expires_at: None,
                account_hint: None,
                expected_origin: None,
            },
        )
        .unwrap();
    let after_set = fx
        .mgr
        .subscription_usage_list(
            &fx.owner,
            CostSubscriptionUsageListParams {
                connection_id: Some(conn2.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        after_set.snapshots[0].status,
        SubscriptionUsageStatus::NeverCollected
    );
    assert_eq!(
        after_set.snapshots[0].issue_code.as_deref(),
        Some("NOT_COLLECTED_YET")
    );

    let prof = fx
        .office
        .load_model_profiles()
        .unwrap()
        .into_iter()
        .find(|p| {
            p.generated_for_connection_id.as_ref().map(|i| i.as_str()) == Some(conn2.as_str())
        })
        .unwrap();
    fx.office
        .model_profile_update(
            &fx.owner,
            org_proto::providers::ModelProfileUpdateParams {
                command_id: next_uuid(),
                profile_id: prof.id.as_str().to_owned(),
                expected_version: prof.meta.revision,
                name: None,
                description: None,
                archived: Some(true),
                connection_ids: None,
            },
        )
        .unwrap()
        .unwrap();
    let c2_rec = fx
        .office
        .load_provider_connections()
        .unwrap()
        .into_iter()
        .find(|c| c.id.as_str() == conn2.as_str())
        .unwrap();
    fx.office
        .provider_connection_update(
            &fx.owner,
            org_proto::providers::ProviderConnectionUpdateParams {
                command_id: next_uuid(),
                connection_id: conn2.clone(),
                expected_version: c2_rec.meta.revision,
                name: None,
                endpoint_url: None,
                clear_endpoint_url: false,
                enabled: None,
                archived: Some(true),
                max_concurrent_turns: None,
            },
        )
        .unwrap()
        .unwrap();
    let revoked_archived = fx
        .mgr
        .subscription_usage_credential_revoke(
            &fx.owner,
            CostSubscriptionUsageCredentialRevokeParams {
                connection_id: conn2,
            },
        )
        .unwrap();
    assert!(!revoked_archived.credential.configured);
}

#[test]
fn partial_counter_rollup_marks_partial_instead_of_understating() {
    let fx = SubUsageFixture::new();
    let sub1 = fx.create_connection(
        "claude-part-1",
        "anthropic",
        ProviderAuthMode::SubscriptionCli,
        Some("sub-p1"),
        None,
    );
    let sub2 = fx.create_connection(
        "claude-part-2",
        "anthropic",
        ProviderAuthMode::SubscriptionCli,
        Some("sub-p2"),
        Some("https://proxy-p2.example.com"),
    );
    for (cid, sec, orig) in [
        (&sub1, "tok-p1", None),
        (&sub2, "tok-p2", Some("https://proxy-p2.example.com")),
    ] {
        fx.mgr
            .subscription_usage_credential_set(
                &fx.owner,
                CostSubscriptionUsageCredentialSetParams {
                    connection_id: cid.clone(),
                    secret: sec.into(),
                    expires_at: None,
                    account_hint: None,
                    expected_origin: orig.map(str::to_owned),
                },
            )
            .unwrap();
    }
    fx.transport
        .on_url("https://api.anthropic.com/api/oauth/usage", |_, _| {
            Ok(HttpUsageResponse {
                status: 200,
                body: br#"{"models":[{"model":"claude-sonnet-4","total":{"requests":10,"input_tokens":1000},"daily":{"requests":4,"resets_at":"2026-11-22T00:00:00Z"}}]}"#
                    .to_vec(),
            })
        });
    fx.transport
        .on_url("https://proxy-p2.example.com/api/oauth/usage", |_, _| {
            Ok(HttpUsageResponse {
                status: 200,
                body: br#"{"models":[{"model":"claude-sonnet-4","total":{"requests":5},"daily":{"requests":6,"resets_at":"2026-11-22T08:00:00Z"}}]}"#
                    .to_vec(),
            })
        });
    fx.mgr
        .subscription_usage_collect(&fx.owner, CostSubscriptionUsageCollectParams::default())
        .unwrap();
    let listed = fx
        .mgr
        .subscription_usage_list(&fx.owner, CostSubscriptionUsageListParams::default())
        .unwrap();
    assert!(listed.partial_totals);
    assert_eq!(listed.excluded_snapshot_count, 0);
    let row = listed
        .totals_by_model
        .iter()
        .find(|m| m.model == "claude-sonnet-4")
        .unwrap();
    assert_eq!(row.total.requests, Some(15));
    assert_eq!(row.total.input_tokens, None);
    // Misaligned daily resets_at (00:00Z vs 08:00Z) refuses to sum different periods!
    assert_eq!(row.daily.requests, None);
}

#[test]
fn duplicate_subscription_identity_deduplicates_and_account_mismatch_rejected() {
    use org_office::accounting::aggregate_totals_by_model;
    let fx = SubUsageFixture::new();
    let c1 = fx.create_connection(
        "claude-dup-1",
        "anthropic",
        ProviderAuthMode::SubscriptionCli,
        Some("same-sub-acct"),
        None,
    );
    fx.mgr
        .subscription_usage_credential_set(
            &fx.owner,
            CostSubscriptionUsageCredentialSetParams {
                connection_id: c1.clone(),
                secret: "tok-d1".into(),
                expires_at: None,
                account_hint: None,
                expected_origin: None,
            },
        )
        .unwrap();
    fx.transport
        .on_url("https://api.anthropic.com/api/oauth/usage", |_, _| {
            Ok(HttpUsageResponse {
                status: 200,
                body: br#"{"subscription_identity":"same-sub-acct","models":[{"model":"claude-sonnet-4","total":{"requests":10}}]}"#
                    .to_vec(),
            })
        });
    let ok = fx
        .mgr
        .subscription_usage_collect(
            &fx.owner,
            CostSubscriptionUsageCollectParams {
                connection_id: Some(c1.clone()),
                time_zone: None,
            },
        )
        .unwrap();
    // Deduplication in rollup across provider aliases (anthropic vs claude) sharing subscription_identity:
    let snap = ok.snapshots[0].clone();
    let mut dup_snap = snap.clone();
    dup_snap.connection_id = next_uuid();
    dup_snap.provider_type = "claude".into();
    let deduped = aggregate_totals_by_model(&[snap.clone(), dup_snap]);
    assert!(deduped.partial_totals);
    assert_eq!(deduped.excluded_snapshot_count, 1);
    assert_eq!(deduped.totals_by_model[0].total.requests, Some(10));

    // DeadlineExceeded leaves previously fresh snapshot intact and defers connection_id:
    fx.transport
        .on_url("https://api.anthropic.com/api/oauth/usage", |_, _| {
            Err(UsageTransportError::DeadlineExceeded)
        });
    let deferred = fx
        .mgr
        .subscription_usage_collect(
            &fx.owner,
            CostSubscriptionUsageCollectParams {
                connection_id: Some(c1.clone()),
                time_zone: None,
            },
        )
        .unwrap();
    assert!(deferred.snapshots.is_empty());
    assert_eq!(deferred.deferred_connection_ids, vec![c1.clone()]);
    let still_ok = fx
        .mgr
        .subscription_usage_list(
            &fx.owner,
            CostSubscriptionUsageListParams {
                connection_id: Some(c1.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(still_ok.snapshots[0].status, SubscriptionUsageStatus::Ok);
    assert_eq!(still_ok.totals_by_model[0].total.requests, Some(10));

    // Overflow in rollup returns None + partial_totals = true instead of saturating silently:
    let mut big1 = snap;
    big1.subscription_identity = Some("sub-a".into());
    big1.models[0].total.requests = Some(u64::MAX);
    let mut big2 = big1.clone();
    big2.subscription_identity = Some("sub-b".into());
    big2.models[0].total.requests = Some(1);
    let overflowed = aggregate_totals_by_model(&[big1, big2]);
    assert!(overflowed.partial_totals);
    assert_eq!(overflowed.totals_by_model[0].total.requests, None);

    // Blank subscription_identity must NOT suppress an explicit mismatched account_id:
    fx.transport
        .on_url("https://api.anthropic.com/api/oauth/usage", |_, _| {
            Ok(HttpUsageResponse {
                status: 200,
                body: br#"{"subscription_identity":"","account_id":"other-sub-acct","models":[{"model":"claude-sonnet-4","total":{"requests":10}}]}"#
                    .to_vec(),
            })
        });
    let mis = fx
        .mgr
        .subscription_usage_collect(
            &fx.owner,
            CostSubscriptionUsageCollectParams {
                connection_id: Some(c1),
                time_zone: None,
            },
        )
        .unwrap();
    assert_eq!(
        mis.snapshots[0].status,
        SubscriptionUsageStatus::AuthRequired
    );
    assert_eq!(
        mis.snapshots[0].issue_code.as_deref(),
        Some("ACCOUNT_IDENTITY_MISMATCH")
    );
}
