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
