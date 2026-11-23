//! End-to-end automated tests for `TASK-79`: authenticated provider subscription model usage
//! collection (total, daily, weekly), wrapped private credential custody outside organization Git,
//! expiry/missing-auth/provider-unavailable/unsupported handling, SSRF guards, owner-only
//! authorization scoping, and cross-subscription aggregation.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use org_office::organization_family::records::ProviderAuthMode;
use org_office::{Office, TestOffice};
use org_proto::costs::{
    CostSubscriptionUsageCollectParams, CostSubscriptionUsageCredentialRevokeParams,
    CostSubscriptionUsageCredentialSetParams, CostSubscriptionUsageListParams,
    SubscriptionUsageAuthSource, SubscriptionUsageStatus,
};
use org_proto::hiring::{ProviderConnectionCreate, ProviderConnectionCreateParams};
use org_proto::org::{
    MemberAddPersonParams, ProjectCreateParams, TeamCreateParams, TeamSetLeadParams,
};
use org_proto::{ErrorCode, Request};

use crate::credentials::CredentialVault;
use crate::manager::{SessionManager, SessionManagerConfig};
use crate::subscription_usage::{
    validate_usage_endpoint_url, HttpUsageResponse, ProviderSubscriptionUsageCollector,
    SubscriptionUsageTransport, UsageTransportError,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(79_000);

fn next_uuid() -> String {
    let n = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let ms = org_office::organization_family::testkit::CREATION_AT.0 + 100 + (n % 10_000);
    let mut b = [0u8; 16];
    b[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    b[6] = 0x70 | ((n >> 8) as u8 & 0x0f);
    b[7] = (n & 0xff) as u8;
    b[8] = 0x80;
    b[15] = (n & 0xff) as u8;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

type MockHandler = Box<
    dyn Fn(&str, &[(String, String)]) -> Result<HttpUsageResponse, UsageTransportError>
        + Send
        + Sync,
>;

struct MockTransport {
    calls: AtomicUsize,
    handlers: Mutex<HashMap<String, MockHandler>>,
}

impl MockTransport {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            handlers: Mutex::new(HashMap::new()),
        })
    }

    fn on_url(
        &self,
        url_prefix: &str,
        f: impl Fn(&str, &[(String, String)]) -> Result<HttpUsageResponse, UsageTransportError>
            + Send
            + Sync
            + 'static,
    ) {
        self.handlers
            .lock()
            .unwrap()
            .insert(url_prefix.to_owned(), Box::new(f));
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl SubscriptionUsageTransport for MockTransport {
    fn get_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        _deadline: std::time::Instant,
    ) -> Result<HttpUsageResponse, UsageTransportError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let guard = self.handlers.lock().unwrap();
        for (prefix, handler) in guard.iter() {
            if url.starts_with(prefix) {
                return handler(url, headers);
            }
        }
        Err(UsageTransportError::Unavailable(format!(
            "no mock configured for {url}"
        )))
    }
}

struct SubUsageFixture {
    _tmp: tempfile::TempDir,
    office: Arc<Office>,
    mgr: SessionManager,
    transport: Arc<MockTransport>,
    owner: String,
    pm: String,
    member: String,
}

impl SubUsageFixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let office = Arc::new(Office::for_test(TestOffice::default()));
        let owner = office.owner_member_id().to_owned();
        let snap = office.org_snapshot().expect("snapshot");
        let root_team = snap.teams[0].id.clone();

        let team_id = next_uuid();
        office
            .submit_org(
                &owner,
                org_office::rules::OrgCommand::TeamCreate(TeamCreateParams {
                    command_id: next_uuid(),
                    team_id: team_id.clone(),
                    name: "Core".into(),
                    owner_team_id: Some(root_team),
                }),
            )
            .unwrap()
            .unwrap();
        let pm = next_uuid();
        office
            .submit_org(
                &owner,
                org_office::rules::OrgCommand::MemberAddPerson(MemberAddPersonParams {
                    command_id: next_uuid(),
                    member_id: pm.clone(),
                    name: "PM".into(),
                    handle: Some("pm".into()),
                    home_team_id: Some(team_id.clone()),
                    contact: None,
                }),
            )
            .unwrap()
            .unwrap();
        let team_ver = office
            .org_snapshot()
            .unwrap()
            .teams
            .into_iter()
            .find(|t| t.id == team_id)
            .unwrap()
            .version;
        office
            .submit_org(
                &owner,
                org_office::rules::OrgCommand::TeamSetLead(TeamSetLeadParams {
                    command_id: next_uuid(),
                    team_id: team_id.clone(),
                    expected_version: team_ver,
                    lead_member_id: Some(pm.clone()),
                }),
            )
            .unwrap()
            .unwrap();
        office
            .submit_org(
                &owner,
                org_office::rules::OrgCommand::ProjectCreate(ProjectCreateParams {
                    command_id: next_uuid(),
                    project_id: next_uuid(),
                    name: "Alpha".into(),
                    key: None,
                    description: None,
                    teams: vec![],
                    general_area_id: String::new(),
                    general_area_name: None,
                    repo: None,
                    integration_branch: None,
                    slug: Some("alpha".into()),
                    owner_team_id: Some(team_id.clone()),
                    create_team_id: None,
                    parent_team_id: None,
                    contributing_team_ids: Vec::new(),
                }),
            )
            .unwrap()
            .unwrap();
        let member = next_uuid();
        office
            .submit_org(
                &owner,
                org_office::rules::OrgCommand::MemberAddPerson(MemberAddPersonParams {
                    command_id: next_uuid(),
                    member_id: member.clone(),
                    name: "Dev".into(),
                    handle: Some("dev".into()),
                    home_team_id: Some(team_id),
                    contact: None,
                }),
            )
            .unwrap()
            .unwrap();

        let transport = MockTransport::new();
        let mut cfg = SessionManagerConfig::production(tmp.path().join("office.sock"));
        cfg.subscription_usage_collector =
            Arc::new(ProviderSubscriptionUsageCollector::with_transport(
                Arc::clone(&transport) as Arc<dyn SubscriptionUsageTransport>,
            ));
        let mgr = SessionManager::new(Arc::clone(&office), cfg);
        Self {
            _tmp: tmp,
            office,
            mgr,
            transport,
            owner,
            pm,
            member,
        }
    }

    fn create_connection(
        &self,
        key: &str,
        provider_type: &str,
        auth_mode: ProviderAuthMode,
        subscription_identity: Option<&str>,
        endpoint_url: Option<&str>,
    ) -> String {
        let id = next_uuid();
        let runtime_driver = match provider_type {
            "openai" => "codex",
            "google" => "agy",
            _ => "claude",
        };
        let req = Request::new::<ProviderConnectionCreate>(
            1,
            &ProviderConnectionCreateParams {
                command_id: next_uuid(),
                connection_id: id.clone(),
                default_profile_id: next_uuid(),
                key: key.to_owned(),
                name: format!("Conn {key}"),
                provider_type: provider_type.to_owned(),
                auth_mode: auth_mode.as_str().to_owned(),
                subscription_identity: subscription_identity.map(str::to_owned),
                runtime_driver: runtime_driver.to_owned(),
                endpoint_url: endpoint_url.map(str::to_owned),
                max_concurrent_turns: 2,
            },
        )
        .unwrap();
        let resp = self
            .office
            .dispatch_hiring_rpc(Some(&self.owner), &req)
            .expect("handled");
        assert!(
            matches!(resp.outcome, org_proto::Outcome::Result(_)),
            "create connection failed: {resp:?}"
        );
        id
    }
}

fn dir_contains_bytes(dir: &Path, needle: &[u8]) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if dir_contains_bytes(&path, needle) {
                return true;
            }
        } else if let Ok(data) = fs::read(&path) {
            if data.windows(needle.len()).any(|w| w == needle) {
                return true;
            }
        }
    }
    false
}

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

fn assert_non_owners_rejected(fx: &SubUsageFixture, conn_id: &str) {
    for unauthorized in [&fx.pm, &fx.member] {
        assert_eq!(
            fx.mgr
                .subscription_usage_list(unauthorized, CostSubscriptionUsageListParams::default())
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );
        assert_eq!(
            fx.mgr
                .subscription_usage_collect(
                    unauthorized,
                    CostSubscriptionUsageCollectParams::default()
                )
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );
        assert_eq!(
            fx.mgr
                .subscription_usage_credential_set(
                    unauthorized,
                    CostSubscriptionUsageCredentialSetParams {
                        connection_id: conn_id.to_owned(),
                        secret: "tok-nope".into(),
                        expires_at: None,
                        account_hint: None,
                        expected_origin: None,
                    }
                )
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );
        assert_eq!(
            fx.mgr
                .subscription_usage_credential_revoke(
                    unauthorized,
                    CostSubscriptionUsageCredentialRevokeParams {
                        connection_id: conn_id.to_owned(),
                    }
                )
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );
    }
}

fn configure_mock_providers(transport: &MockTransport) {
    transport.on_url("https://api.anthropic.com/api/oauth/usage", |_url, headers| {
        assert!(headers.iter().any(|(k, v)| {
            k == "Authorization" && v == "Bearer sk-ant-oat-secret-claude-a-99999"
        }));
        assert!(headers
            .iter()
            .any(|(k, v)| k == "anthropic-beta" && v == "oauth-2025-04-20"));
        Ok(HttpUsageResponse {
            status: 200,
            body: br#"{
                "models": [
                    {
                        "model": "claude-sonnet-4",
                        "total":  {"requests": 100, "input_tokens": 500000, "output_tokens": 120000, "total_tokens": 620000},
                        "daily":  {"requests": 12,  "input_tokens": 60000,  "output_tokens": 15000,  "total_tokens": 75000, "utilization": 22.5, "resets_at": "2026-11-22T00:00:00Z"},
                        "weekly": {"requests": 64,  "input_tokens": 310000, "output_tokens": 80000,  "total_tokens": 390000, "utilization": 48.0, "resets_at": "2026-11-28T00:00:00Z"}
                    },
                    {
                        "model": "claude-opus-4",
                        "total":  {"requests": 20, "total_tokens": 90000},
                        "daily":  {"requests": 3,  "total_tokens": 14000, "utilization_bps": 1500},
                        "weekly": {"requests": 15, "total_tokens": 68000, "utilization_bps": 4100}
                    }
                ]
            }"#
            .to_vec(),
        })
    });
    transport.on_url("https://proxy-b.example.com/api/oauth/usage", |_url, headers| {
        assert!(headers.iter().any(|(k, v)| {
            k == "Authorization" && v == "Bearer sk-ant-oat-secret-claude-b-88888"
        }));
        Ok(HttpUsageResponse {
            status: 200,
            body: br#"{
                "models": [
                    {
                        "model": "claude-sonnet-4",
                        "total":  {"requests": 50, "input_tokens": 200000, "output_tokens": 40000, "total_tokens": 240000},
                        "daily":  {"requests": 8,  "input_tokens": 25000,  "output_tokens": 5000,  "total_tokens": 30000, "utilization": 10.0, "resets_at": "2026-11-22T00:00:00Z"},
                        "weekly": {"requests": 36, "input_tokens": 140000, "output_tokens": 20000, "total_tokens": 160000, "utilization": 30.0, "resets_at": "2026-11-28T00:00:00Z"}
                    }
                ]
            }"#
            .to_vec(),
        })
    });
    transport.on_url(
        "https://chatgpt.com/backend-api/wham/usage",
        |_url, headers| {
            assert!(headers
                .iter()
                .any(|(k, v)| k == "Authorization" && v == "Bearer codex-oauth-secret-77777"));
            Ok(HttpUsageResponse {
                status: 200,
                body: br#"{
                    "rate_limit": {
                        "primary_window":   {"used_percent": 35.5, "limit_window_seconds": 86400, "reset_at": 1795220000},
                        "secondary_window": {"used_percent": 61.25, "limit_window_seconds": 604800, "reset_at": 1795800000}
                    },
                    "additional_rate_limits": [
                        {
                            "limit_name": "gpt-5.4-codex",
                            "rate_limit": {
                                "primary_window":   {"used_percent": 18.0, "limit_window_seconds": 18000, "requests": 9},
                                "secondary_window": {"used_percent": 42.0, "limit_window_seconds": 604800, "requests": 44}
                            }
                        }
                    ]
                }"#
                .to_vec(),
            })
        },
    );
}

fn assert_collected_and_aggregated(
    fx: &SubUsageFixture,
    claude_a: &str,
    codex: &str,
    google: &str,
) {
    let collected = fx
        .mgr
        .subscription_usage_collect(
            &fx.owner,
            CostSubscriptionUsageCollectParams {
                connection_id: None,
                time_zone: Some("UTC".into()),
            },
        )
        .unwrap();
    assert_eq!(collected.snapshots.len(), 4);

    let snap_a = collected
        .snapshots
        .iter()
        .find(|s| s.connection_id == claude_a)
        .unwrap();
    assert_eq!(snap_a.status, SubscriptionUsageStatus::Ok);
    assert_eq!(
        snap_a.auth_source,
        SubscriptionUsageAuthSource::DedicatedUsageCredential
    );
    assert!(!snap_a.stale);
    assert_eq!(snap_a.models.len(), 2);

    let snap_codex = collected
        .snapshots
        .iter()
        .find(|s| s.connection_id == codex)
        .unwrap();
    assert_eq!(snap_codex.status, SubscriptionUsageStatus::Ok);
    let codex_row = snap_codex
        .models
        .iter()
        .find(|m| m.model == "codex-subscription")
        .unwrap();
    assert_eq!(codex_row.daily.utilization_bps, Some(3_550));
    assert_eq!(codex_row.daily.window_seconds, Some(86_400));
    assert_eq!(codex_row.weekly.utilization_bps, Some(6_125));
    assert_eq!(codex_row.weekly.window_seconds, Some(604_800));
    assert!(codex_row.daily.resets_at.is_some());
    let codex_54 = snap_codex
        .models
        .iter()
        .find(|m| m.model == "gpt-5.4-codex")
        .unwrap();
    assert_eq!(
        codex_54.short_term.as_ref().and_then(|w| w.utilization_bps),
        Some(1_800)
    );
    assert_eq!(
        codex_54.short_term.as_ref().and_then(|w| w.window_seconds),
        Some(18_000)
    );
    assert_eq!(codex_54.daily.utilization_bps, None);

    let snap_google = collected
        .snapshots
        .iter()
        .find(|s| s.connection_id == google)
        .unwrap();
    assert_eq!(snap_google.status, SubscriptionUsageStatus::Unsupported);
    assert_eq!(
        snap_google.issue_code.as_deref(),
        Some("PROVIDER_USAGE_API_UNSUPPORTED")
    );
    assert!(snap_google.models.is_empty());

    let listed = fx
        .mgr
        .subscription_usage_list(&fx.owner, CostSubscriptionUsageListParams::default())
        .unwrap();
    assert!(!listed.is_api_equivalent_estimate);
    assert!(listed.collected_as_of.is_some());
    assert!(listed.partial_totals);
    assert_eq!(listed.excluded_snapshot_count, 1); // google unsupported snapshot excluded
    let sonnet_tot = listed
        .totals_by_model
        .iter()
        .find(|m| m.model == "claude-sonnet-4")
        .unwrap();
    assert_eq!(sonnet_tot.total.requests, Some(150));
    assert_eq!(sonnet_tot.total.total_tokens, Some(860_000));
    assert_eq!(sonnet_tot.daily.total_tokens, Some(105_000));
    assert_eq!(sonnet_tot.weekly.total_tokens, Some(550_000));
    assert_eq!(sonnet_tot.daily.utilization_bps, None);

    let limited = fx
        .mgr
        .subscription_usage_list(
            &fx.owner,
            CostSubscriptionUsageListParams {
                limit: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(limited.snapshots.len(), 1);
    assert!(limited.next_cursor.is_some());
    let page2 = fx
        .mgr
        .subscription_usage_list(
            &fx.owner,
            CostSubscriptionUsageListParams {
                limit: Some(2),
                cursor: limited.next_cursor,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(page2.snapshots.len(), 2);
    assert_ne!(
        page2.snapshots[0].connection_key,
        limited.snapshots[0].connection_key
    );

    let opus_tot = listed
        .totals_by_model
        .iter()
        .find(|m| m.model == "claude-opus-4")
        .unwrap();
    assert_eq!(opus_tot.total.total_tokens, Some(90_000));
    assert_eq!(opus_tot.total.input_tokens, None);
    assert_eq!(opus_tot.daily.utilization_bps, Some(1_500));
}

fn seed_usage_credentials_and_verify_vault(
    fx: &SubUsageFixture,
    claude_a: &str,
    claude_b: &str,
    codex: &str,
    google: &str,
) -> [&'static str; 4] {
    let secret_a = "sk-ant-oat-secret-claude-a-99999";
    let secret_b = "sk-ant-oat-secret-claude-b-88888";
    let secret_codex = "codex-oauth-secret-77777";
    let secret_google = "agy-oauth-secret-66666";
    let cred_a = fx
        .mgr
        .subscription_usage_credential_set(
            &fx.owner,
            CostSubscriptionUsageCredentialSetParams {
                connection_id: claude_a.to_owned(),
                secret: secret_a.into(),
                expires_at: Some("2027-01-01T00:00:00Z".into()),
                account_hint: Some("alice@acme.test".into()),
                expected_origin: Some("https://api.anthropic.com".into()),
            },
        )
        .unwrap();
    assert!(cred_a.credential.configured);
    assert_eq!(
        cred_a.credential.account_hint.as_deref(),
        Some("alice@acme.test")
    );
    for (cid, sec, hint, origin) in [
        (
            claude_b,
            secret_b,
            Some("bob@acme.test"),
            Some("https://proxy-b.example.com"),
        ),
        (codex, secret_codex, Some("codex@acme.test"), None),
        (google, secret_google, None, None),
    ] {
        fx.mgr
            .subscription_usage_credential_set(
                &fx.owner,
                CostSubscriptionUsageCredentialSetParams {
                    connection_id: cid.to_owned(),
                    secret: sec.into(),
                    expires_at: None,
                    account_hint: hint.map(str::to_owned),
                    expected_origin: origin.map(str::to_owned),
                },
            )
            .unwrap();
    }
    let info = fx.office.info();
    let vault = CredentialVault::new(
        fx.office.durable_root().to_path_buf(),
        &info.org_id,
        &info.office_id,
        Some([7; 32]),
    );
    assert!(vault.load_unwrapped(claude_a).unwrap().is_none());
    let usage_path = vault.usage_credential_path(claude_a).unwrap();
    assert!(usage_path.exists());
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&usage_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    [secret_a, secret_b, secret_codex, secret_google]
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

fn enroll_claude_fallback(fx: &SubUsageFixture, conn_id: &str, secret: &[u8]) {
    let info = fx.office.info();
    let vault = CredentialVault::new(
        fx.office.durable_root().to_path_buf(),
        &info.org_id,
        &info.office_id,
        Some([7; 32]),
    );
    vault
        .store_wrapped(
            conn_id,
            "anthropic",
            secret,
            None,
            Some("enrolled@acme.test".into()),
        )
        .unwrap();
}

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
