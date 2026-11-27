//! Standalone unit tests for `org-subscription-usage`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use org_proto::costs::SubscriptionUsageAuthSource;
use zeroize::Zeroizing;

use crate::{
    validate_usage_endpoint_url, HttpUsageResponse, ProviderSubscriptionUsageCollector,
    SubscriptionUsageCollectOutcome, SubscriptionUsageCollectRequest, SubscriptionUsageCollector,
    SubscriptionUsageTransport, UsageTransportError,
};

struct FixedTransport(Result<HttpUsageResponse, UsageTransportError>);

impl SubscriptionUsageTransport for FixedTransport {
    fn get_json(
        &self,
        _url: &str,
        _headers: &[(String, String)],
        _deadline: Instant,
    ) -> Result<HttpUsageResponse, UsageTransportError> {
        self.0.clone()
    }
}

#[test]
fn validates_usage_endpoints_and_parses_anthropic_oauth_payload() {
    assert!(validate_usage_endpoint_url("https://api.anthropic.com/api/oauth/usage").is_ok());
    assert!(validate_usage_endpoint_url("http://api.anthropic.com/api/oauth/usage").is_err());
    assert!(validate_usage_endpoint_url("https://127.0.0.1/usage").is_err());
    assert!(validate_usage_endpoint_url("https://localhost/usage").is_err());

    let collector = ProviderSubscriptionUsageCollector::with_transport(Arc::new(FixedTransport(
        Ok(HttpUsageResponse {
            status: 200,
            body: br#"{"five_hour":{"utilization":25.0,"resets_at":"2026-11-27T20:00:00Z"}}"#
                .to_vec(),
        }),
    )));
    let outcome = collector.collect(&SubscriptionUsageCollectRequest {
        connection_id: "conn-1".into(),
        provider_type: "anthropic".into(),
        subscription_identity: None,
        endpoint_url: None,
        auth_source: SubscriptionUsageAuthSource::EnrolledSubscriptionCredential,
        credential: Zeroizing::new(br#"{"access_token":"sk-ant-oat-test-0042"}"#.to_vec()),
        credential_deadline: None,
        time_zone: "UTC".into(),
        collected_at: "2026-11-27T15:00:00.000Z".into(),
        now_ms: 1_795_705_200_000,
        deadline: Instant::now() + Duration::from_secs(5),
    });
    match outcome {
        SubscriptionUsageCollectOutcome::Collected { models, .. } => {
            assert_eq!(models.len(), 1);
            assert_eq!(models[0].model, "claude-subscription");
            assert_eq!(
                models[0]
                    .short_term
                    .as_ref()
                    .and_then(|w| w.utilization_bps),
                Some(2500)
            );
        }
        other => panic!("expected Collected, got {other:?}"),
    }
}

#[test]
fn ssrf_blocklist_rejects_special_use_v4_v6_and_mapped_ranges_while_allowing_public_ips() {
    let blocked = [
        // IPv4 0.0.0.0/8 ("this network")
        "https://0.0.0.0/usage",
        "https://0.1.2.3/usage",
        "https://0.255.255.255/usage",
        // IPv4 loopback, private, link-local, CGNAT, multicast
        "https://127.0.0.1/usage",
        "https://10.0.0.1/usage",
        "https://172.16.0.1/usage",
        "https://192.168.1.1/usage",
        "https://169.254.169.254/usage",
        "https://100.64.0.1/usage",
        "https://100.127.255.254/usage",
        "https://224.0.0.1/usage",
        // IPv4 192.0.0.0/24 (IETF protocol assignments)
        "https://192.0.0.0/usage",
        "https://192.0.0.255/usage",
        // IPv4 198.18.0.0/15 (benchmarking)
        "https://198.18.0.1/usage",
        "https://198.19.255.254/usage",
        // IPv4 240.0.0.0/4 (reserved + broadcast)
        "https://240.0.0.1/usage",
        "https://255.255.255.255/usage",
        // IPv4 TEST-NET-1, TEST-NET-2, TEST-NET-3
        "https://192.0.2.1/usage",
        "https://198.51.100.42/usage",
        "https://203.0.113.99/usage",
        // IPv6 loopback, unspecified, ULA, link-local, multicast
        "https://[::1]/usage",
        "https://[::]/usage",
        "https://[fc00::1]/usage",
        "https://[fd12:3456::1]/usage",
        "https://[fe80::1]/usage",
        "https://[ff02::1]/usage",
        // IPv6 NAT64 well-known 64:ff9b::/96 & local-use 64:ff9b:1::/48
        "https://[64:ff9b::1.1.1.1]/usage",
        "https://[64:ff9b::127.0.0.1]/usage",
        "https://[64:ff9b:1::1]/usage",
        "https://[64:ff9b:1:abcd::1.1.1.1]/usage",
        // IPv6 deprecated site-local fec0::/10
        "https://[fec0::1]/usage",
        "https://[feff::1]/usage",
        // IPv6 discard 100::/64
        "https://[100::1]/usage",
        "https://[100::ffff:1.1.1.1]/usage",
        // IPv6 IPv4-translated ::ffff:0:0:0/96 (RFC 2765)
        "https://[::ffff:0:127.0.0.1]/usage",
        "https://[::ffff:0:1.1.1.1]/usage",
        // IPv6 IPv4-mapped across all newly blocked v4 ranges
        "https://[::ffff:0.0.0.1]/usage",
        "https://[::ffff:192.0.0.1]/usage",
        "https://[::ffff:192.0.2.1]/usage",
        "https://[::ffff:198.18.0.1]/usage",
        "https://[::ffff:198.51.100.1]/usage",
        "https://[::ffff:203.0.113.1]/usage",
        "https://[::ffff:240.0.0.1]/usage",
        // IPv4-compatible across newly blocked v4 ranges
        "https://[::192.0.2.1]/usage",
        "https://[::240.0.0.1]/usage",
    ];
    for url in blocked {
        assert!(
            validate_usage_endpoint_url(url).is_err(),
            "expected SSRF validator to reject {url}"
        );
    }

    let allowed = [
        "https://1.1.1.1/usage",
        "https://93.184.216.34/usage",
        "https://100.63.255.255/usage",
        "https://100.128.0.1/usage",
        "https://192.0.1.1/usage",
        "https://198.17.255.255/usage",
        "https://198.20.0.1/usage",
        "https://223.255.255.254/usage",
        "https://[2606:4700:4700::1111]/usage",
        "https://[2001:4860:4860::8888]/usage",
        "https://[64:ff9b:2::1]/usage",
        "https://[100:0:0:1::1]/usage",
    ];
    for url in allowed {
        assert!(
            validate_usage_endpoint_url(url).is_ok(),
            "expected SSRF validator to allow public IP {url}"
        );
    }
}

struct StubVault {
    enrolled: Option<crate::UnwrappedUsageCredential>,
}

impl crate::UsageCredentialLookup for StubVault {
    fn usage_status(
        &self,
        _connection_id: &str,
    ) -> Result<org_proto::costs::SubscriptionUsageCredentialView, crate::UsageCredentialError>
    {
        unreachable!()
    }
    fn enrolled_status(
        &self,
        _connection_id: &str,
    ) -> Result<org_proto::sessions::ProviderCredentialView, crate::UsageCredentialError> {
        unreachable!()
    }
    fn store_wrapped_usage(
        &self,
        _connection_id: &str,
        _provider: &str,
        _bound_origin: &str,
        _secret: &[u8],
        _expires_at: Option<String>,
        _account_hint: Option<String>,
    ) -> Result<org_proto::costs::SubscriptionUsageCredentialView, crate::UsageCredentialError>
    {
        unreachable!()
    }
    fn revoke_usage(
        &self,
        _connection_id: &str,
    ) -> Result<org_proto::costs::SubscriptionUsageCredentialView, crate::UsageCredentialError>
    {
        unreachable!()
    }
    fn load_unwrapped_usage(
        &self,
        _connection_id: &str,
        _expected_origin: &str,
    ) -> Result<Option<crate::UnwrappedUsageCredential>, crate::UsageCredentialError> {
        Ok(None)
    }
    fn load_unwrapped_enrolled(
        &self,
        _connection_id: &str,
    ) -> Result<Option<crate::UnwrappedUsageCredential>, crate::UsageCredentialError> {
        Ok(self
            .enrolled
            .as_ref()
            .map(|c| crate::UnwrappedUsageCredential {
                secret: c.secret.clone(),
                expires_at: c.expires_at.clone(),
            }))
    }
}

#[test]
fn enrolled_oauth_expiry_enforces_min_of_outer_and_inner_deadlines() {
    let now_ms: u64 = 1_800_000_000_000;
    let past_ms = now_ms - 60_000;
    let soon_ms = now_ms + 60_000;
    let far_ms = now_ms + 3_600_000;

    // 1. Expired inner oauth.expires_at_ms fails closed EVEN when outer expires_at is far in future.
    let expired_inner = StubVault {
        enrolled: Some(crate::UnwrappedUsageCredential {
            secret: Zeroizing::new(
                format!(r#"{{"access_token":"tok-expired-inner","expiresAt":{past_ms}}}"#)
                    .into_bytes(),
            ),
            expires_at: Some(org_office::format_rfc3339_ms(far_ms)),
        }),
    };
    let err = crate::rollup::load_active_credential(
        &expired_inner,
        "conn-1",
        "https://api.anthropic.com",
        now_ms,
    )
    .err()
    .expect("expired inner OAuth timestamp must fail closed");
    assert!(matches!(
        err.1,
        SubscriptionUsageCollectOutcome::AuthRequired { ref issue_code, .. }
            if issue_code == "CREDENTIAL_EXPIRED"
    ));

    // 2. Expired outer expires_at fails closed EVEN when inner oauth.expires_at_ms is far in future.
    let expired_outer = StubVault {
        enrolled: Some(crate::UnwrappedUsageCredential {
            secret: Zeroizing::new(
                format!(r#"{{"access_token":"tok-expired-outer","expiresAt":{far_ms}}}"#)
                    .into_bytes(),
            ),
            expires_at: Some(org_office::format_rfc3339_ms(past_ms)),
        }),
    };
    let err_outer = crate::rollup::load_active_credential(
        &expired_outer,
        "conn-1",
        "https://api.anthropic.com",
        now_ms,
    )
    .err()
    .expect("expired outer timestamp must fail closed");
    assert!(matches!(
        err_outer.1,
        SubscriptionUsageCollectOutcome::AuthRequired { ref issue_code, .. }
            if issue_code == "CREDENTIAL_EXPIRED"
    ));

    // 3. Both valid: effective expires_at is min(outer, inner) in both orderings.
    let inner_sooner = StubVault {
        enrolled: Some(crate::UnwrappedUsageCredential {
            secret: Zeroizing::new(
                format!(r#"{{"access_token":"tok-inner-sooner","expiresAt":{soon_ms}}}"#)
                    .into_bytes(),
            ),
            expires_at: Some(org_office::format_rfc3339_ms(far_ms)),
        }),
    };
    let (_, cred_a) = crate::rollup::load_active_credential(
        &inner_sooner,
        "conn-1",
        "https://api.anthropic.com",
        now_ms,
    )
    .unwrap();
    assert_eq!(
        cred_a.expires_at.as_deref(),
        Some(org_office::format_rfc3339_ms(soon_ms).as_str())
    );

    let outer_sooner = StubVault {
        enrolled: Some(crate::UnwrappedUsageCredential {
            secret: Zeroizing::new(
                format!(r#"{{"access_token":"tok-outer-sooner","expiresAt":{far_ms}}}"#)
                    .into_bytes(),
            ),
            expires_at: Some(org_office::format_rfc3339_ms(soon_ms)),
        }),
    };
    let (_, cred_b) = crate::rollup::load_active_credential(
        &outer_sooner,
        "conn-1",
        "https://api.anthropic.com",
        now_ms,
    )
    .unwrap();
    assert_eq!(
        cred_b.expires_at.as_deref(),
        Some(org_office::format_rfc3339_ms(soon_ms).as_str())
    );
}

#[test]
fn zeroizing_headers_builds_exact_capacity_bearer_header() {
    let headers = crate::ZeroizingHeaders::new(
        "tok-secret-0042",
        vec![("anthropic-beta".into(), "oauth-2025-04-20".into())],
    );
    let slice = headers.as_slice();
    assert_eq!(slice.len(), 3);
    assert_eq!(slice[0].0, "Authorization");
    assert_eq!(slice[0].1, "Bearer tok-secret-0042");
    assert_eq!(slice[0].1.capacity(), slice[0].1.len());
    assert_eq!(slice[1], ("Accept".into(), "application/json".into()));
    assert_eq!(
        slice[2],
        ("anthropic-beta".into(), "oauth-2025-04-20".into())
    );
}
