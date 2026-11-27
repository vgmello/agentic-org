//! Vault-native OAuth credential unit tests (`TASK-132`): envelope parsing/validation,
//! zeroization/redaction, tmpdir placement, host-auth refusal for OAuth-injected runtimes,
//! and single-flight vault refresh + operator-rotation adoption.

use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use org_office::organization_family::records::ProviderAuthMode;
use zeroize::Zeroizing;

use crate::credentials::CredentialVault;

use super::{
    allows_host_subscription_auth, collect_vault_redaction_secrets, host_config_dir_for,
    is_oauth_expiring, oauth_token_fingerprint, parse_vault_oauth_credential, resolve_agent_tmpdir,
    NoopVaultTokenRefresher, VaultAuthError, VaultOAuthCredential, VaultRefreshCoordinator,
    VaultTokenRefresher, MAX_SHORT_TMPDIR_BYTES, MAX_VAULT_OAUTH_CRED_BYTES, OAUTH_REFRESH_LEAD_MS,
};

type RefreshResponseFn =
    Box<dyn Fn(usize, &str) -> Result<VaultOAuthCredential, String> + Send + Sync>;

struct CountingRefresher {
    calls: AtomicUsize,
    barrier: Option<Arc<Barrier>>,
    response: RefreshResponseFn,
}

impl VaultTokenRefresher for CountingRefresher {
    fn refresh(
        &self,
        _connection_id: &str,
        _provider: &str,
        refresh_token: &str,
    ) -> Result<VaultOAuthCredential, String> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(ref b) = self.barrier {
            b.wait();
        }
        (self.response)(n, refresh_token)
    }
}

fn test_vault(dir: &Path) -> CredentialVault {
    CredentialVault::new(dir.to_path_buf(), "org-1", "office-1", Some([0x42u8; 32]))
}

#[test]
fn parses_raw_and_structured_vault_oauth_credentials_and_rejects_malformed() {
    // 1. Raw OAuth access token string (non-expiring / externally rotated).
    let raw = parse_vault_oauth_credential(b"  sk-ant-oat01-raw-token  \n").unwrap();
    assert_eq!(raw.access_token.as_str(), "sk-ant-oat01-raw-token");
    assert!(raw.refresh_token.is_none());
    assert_eq!(raw.expires_at_ms, None);
    assert_eq!(raw.fingerprint().len(), 16);
    assert_eq!(
        raw.fingerprint(),
        oauth_token_fingerprint("sk-ant-oat01-raw-token")
    );
    assert!(!is_oauth_expiring(raw.expires_at_ms, 1_795_620_000_000));

    // 2. Standard flat JSON envelope + canonical round-trip + redacting Debug.
    let flat = br#"{"access_token":"sk-ant-oat01-flat","refresh_token":"rt-secret-1","expires_at_ms":1795620000000}"#;
    let parsed = parse_vault_oauth_credential(flat).unwrap();
    assert_eq!(parsed.access_token.as_str(), "sk-ant-oat01-flat");
    assert_eq!(
        parsed.refresh_token.as_deref().map(String::as_str),
        Some("rt-secret-1")
    );
    assert_eq!(parsed.expires_at_ms, Some(1_795_620_000_000));
    let dbg = format!("{parsed:?}");
    assert!(
        !dbg.contains("sk-ant-oat01-flat"),
        "Debug leaked access_token: {dbg}"
    );
    assert!(
        !dbg.contains("rt-secret-1"),
        "Debug leaked refresh_token: {dbg}"
    );
    assert!(dbg.contains(&parsed.fingerprint()));

    let encoded = parsed.to_vault_json().unwrap();
    let roundtrip = parse_vault_oauth_credential(&encoded).unwrap();
    assert_eq!(roundtrip.access_token.as_str(), "sk-ant-oat01-flat");
    assert_eq!(
        roundtrip.refresh_token.as_deref().map(String::as_str),
        Some("rt-secret-1")
    );
    assert_eq!(roundtrip.expires_at_ms, Some(1_795_620_000_000));

    // 3. CamelCase aliases and `claudeAiOauth` wrapper for seamless vault enrollment.
    let wrapped = br#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-wrap","refreshToken":"rt-wrap","expiresAt":1795620000000}}"#;
    let parsed_wrap = parse_vault_oauth_credential(wrapped).unwrap();
    assert_eq!(parsed_wrap.access_token.as_str(), "sk-ant-oat01-wrap");
    assert_eq!(
        parsed_wrap.refresh_token.as_deref().map(String::as_str),
        Some("rt-wrap")
    );
    assert_eq!(parsed_wrap.expires_at_ms, Some(1_795_620_000_000));

    // Expiry lead-time boundary.
    assert!(is_oauth_expiring(
        parsed_wrap.expires_at_ms,
        1_795_620_000_000 - OAUTH_REFRESH_LEAD_MS
    ));
    assert!(!is_oauth_expiring(
        parsed_wrap.expires_at_ms,
        1_795_620_000_000 - OAUTH_REFRESH_LEAD_MS - 1
    ));

    // Redaction extracts both access_token and refresh_token from JSON envelopes.
    let mut secrets = Vec::new();
    collect_vault_redaction_secrets(flat, &mut secrets);
    let secret_strs: Vec<&str> = secrets.iter().map(|s| s.as_str()).collect();
    assert!(secret_strs.contains(&"sk-ant-oat01-flat"));
    assert!(secret_strs.contains(&"rt-secret-1"));

    // 4. Fail closed on empty, malformed JSON starting with `{`, bad types, control chars,
    // invalid UTF-8, and oversized payloads — never leaking secret bytes in error text.
    let bad_cases: &[&[u8]] = &[
        b"",
        b"   \t\n ",
        b"{not-valid-json-secret-123",
        br#"{"access_token":"   "}"#,
        br#"{"claudeAiOauth":{"accessToken":""}}"#,
        br#"{"access_token":12345}"#,
        br#"{"access_token":"ok-tok","expires_at_ms":"not-a-number"}"#,
        br#"{"claudeAiOauth":"not-an-object"}"#,
        b"tok-with\nnewline",
        b"tok-with\0nul",
        &[0xff, 0xfe, 0xfd],
    ];
    for case in bad_cases {
        let err = parse_vault_oauth_credential(case).expect_err("malformed input must fail");
        assert!(matches!(err, VaultAuthError::MalformedCredential(_)));
        let msg = err.actionable_message("claude-sub");
        assert!(!msg.contains("not-valid-json-secret-123"));
        assert!(!msg.contains("tok-with"));
    }
    let oversized = vec![b'a'; MAX_VAULT_OAUTH_CRED_BYTES + 1];
    assert!(matches!(
        parse_vault_oauth_credential(&oversized),
        Err(VaultAuthError::MalformedCredential(_))
    ));
}

#[test]
fn resolve_agent_tmpdir_prefers_short_socket_parent_for_linux_sun_path() {
    let sock = Path::new("/home/agents/.cache/agentic-org/tmp/ao/2oe/f8fc7ab6ed98.sock");
    let deep = Path::new("/home/agents/.local/share/agentic-org/instances/default/org/offices/off/durable/local/offices/off/agents/emp/tmp");
    let picked = resolve_agent_tmpdir(sock, deep, "off-1", "emp-1");
    assert!(picked.starts_with("/home/agents/.cache/agentic-org/tmp/ao/2oe/t/"));
    assert!(picked.as_os_str().len() <= MAX_SHORT_TMPDIR_BYTES);
    assert_ne!(picked, resolve_agent_tmpdir(sock, deep, "off-1", "emp-2"));
}

#[test]
fn allows_host_subscription_auth_blocks_oauth_injected_runtimes_and_enforces_mode() {
    use org_agent_runtime::AgentRuntime as _;

    let claude = org_agent_runtime_claude::ClaudeRuntime::new();
    let codex = org_agent_runtime_codex::CodexRuntime::new();
    let agy = org_agent_runtime_acp::AgyRuntime::new();
    let gemini = org_agent_runtime_acp::GeminiRuntime::new();
    let kimi = org_agent_runtime_acp::KimiRuntime::new();
    let pi = org_agent_runtime_acp::PiRuntime::new();

    // OAuth-injected runtimes (Claude) NEVER fall back to ambient host credentials.
    assert!(claude.host_profile().uses_host_oauth_token_injection);
    assert!(!allows_host_subscription_auth(
        &[],
        None,
        claude.host_profile().uses_host_oauth_token_injection
    ));
    assert!(!allows_host_subscription_auth(
        &[],
        Some(ProviderAuthMode::SubscriptionCli),
        claude.host_profile().uses_host_oauth_token_injection
    ));
    assert!(host_config_dir_for(claude.host_profile().host_config_subdir, true).is_none());

    // Non-OAuth-injected runtimes (Codex/Agy) respect mode and empty-vault checks.
    assert!(allows_host_subscription_auth(&[], None, false));
    assert!(allows_host_subscription_auth(
        &[],
        Some(ProviderAuthMode::SubscriptionCli),
        false
    ));
    assert!(!allows_host_subscription_auth(
        &[],
        Some(ProviderAuthMode::ApiKey),
        false
    ));
    assert!(!allows_host_subscription_auth(
        &[],
        Some(ProviderAuthMode::CloudIam),
        false
    ));
    let non_empty = vec![(OsString::from("ANTHROPIC_API_KEY"), OsString::from("sk-1"))];
    assert!(!allows_host_subscription_auth(
        &non_empty,
        Some(ProviderAuthMode::SubscriptionCli),
        false
    ));
    assert!(host_config_dir_for(codex.host_profile().host_config_subdir, false).is_none());
    assert!(host_config_dir_for(agy.host_profile().host_config_subdir, false).is_none());
    assert!(host_config_dir_for(gemini.host_profile().host_config_subdir, false).is_none());
    assert!(host_config_dir_for(kimi.host_profile().host_config_subdir, false).is_none());
    assert!(host_config_dir_for(pi.host_profile().host_config_subdir, false).is_none());
}

#[test]
fn vault_refresh_coordinator_proactively_rotates_single_flight_and_persists_envelope() {
    let dir = tempfile::tempdir().unwrap();
    let vault = test_vault(dir.path());
    let now = 2_000_000_000_000u64;
    let expiring = VaultOAuthCredential {
        access_token: Zeroizing::new("sk-ant-oat01-expiring".into()),
        refresh_token: Some(Zeroizing::new("rt-initial-1".into())),
        expires_at_ms: Some(now + 60_000),
    };
    let encoded = expiring.to_vault_json().unwrap();
    vault
        .store_wrapped(
            "conn-sf",
            "anthropic",
            &encoded,
            None,
            Some("team@example.com".into()),
        )
        .unwrap();

    let new_exp = now + 3_600_000;
    let refresher = Arc::new(CountingRefresher {
        calls: AtomicUsize::new(0),
        barrier: None,
        response: Box::new(move |attempt, rt| {
            assert_eq!(rt, "rt-initial-1");
            // Small delay lets concurrent waiters queue on the per-connection lock.
            std::thread::sleep(std::time::Duration::from_millis(40));
            Ok(VaultOAuthCredential {
                access_token: Zeroizing::new(format!("sk-ant-oat01-fresh-{attempt}")),
                // Omit refresh_token to verify coordinator preserves the existing one.
                refresh_token: None,
                expires_at_ms: Some(new_exp),
            })
        }),
    });
    let coord = Arc::new(VaultRefreshCoordinator::new(
        Arc::clone(&refresher) as Arc<dyn VaultTokenRefresher>
    ));

    let start_gate = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for _ in 0..3 {
        let c = Arc::clone(&coord);
        let v = vault.clone();
        let g = Arc::clone(&start_gate);
        handles.push(std::thread::spawn(move || {
            g.wait();
            c.resolve_for_launch(&v, "conn-sf", "anthropic", now)
                .unwrap()
        }));
    }
    for h in handles {
        let resolved = h.join().unwrap();
        assert_eq!(resolved.access_token.as_str(), "sk-ant-oat01-fresh-1");
        assert_eq!(resolved.expires_at_ms, Some(new_exp));
        assert_eq!(
            resolved.token_fp,
            oauth_token_fingerprint("sk-ant-oat01-fresh-1")
        );
    }
    assert_eq!(
        refresher.calls.load(Ordering::SeqCst),
        1,
        "concurrent expiring sessions must execute exactly one single-flight refresh"
    );

    // Persisted vault envelope has the new token, preserved refresh_token, and preserved hint.
    let meta = vault.load_unwrapped_with_meta("conn-sf").unwrap().unwrap();
    assert_eq!(meta.account_hint.as_deref(), Some("team@example.com"));
    assert!(meta.expires_at.is_some());
    let stored = parse_vault_oauth_credential(&meta.secret).unwrap();
    assert_eq!(stored.access_token.as_str(), "sk-ant-oat01-fresh-1");
    assert_eq!(
        stored.refresh_token.as_deref().map(String::as_str),
        Some("rt-initial-1")
    );
    assert_eq!(stored.expires_at_ms, Some(new_exp));
}

#[test]
fn vault_refresh_coordinator_adopts_operator_rotation_and_fails_closed_on_errors() {
    let dir = tempfile::tempdir().unwrap();
    let vault = test_vault(dir.path());
    let now = 2_000_000_000_000u64;
    let future_exp = now + 3_600_000;

    // 1. Missing credential -> MissingCredential (`vault_credential_missing`).
    let noop = VaultRefreshCoordinator::noop();
    let missing = noop
        .resolve_for_launch(&vault, "conn-missing", "anthropic", now)
        .err()
        .expect("missing vault credential must fail closed");
    assert_eq!(missing, VaultAuthError::MissingCredential);
    assert_eq!(missing.issue_code(), "vault_credential_missing");

    // 2. Operator already rotated vault T1 -> T2 before reactive 401 refresh:
    //    coordinator adopts T2 immediately with ZERO refresher invocations.
    let fp_t1 = oauth_token_fingerprint("sk-ant-oat01-old-t1");
    let op_rotated = VaultOAuthCredential {
        access_token: Zeroizing::new("sk-ant-oat01-operator-t2".into()),
        refresh_token: Some(Zeroizing::new("rt-op-2".into())),
        expires_at_ms: Some(future_exp),
    };
    vault
        .store_wrapped(
            "conn-op",
            "anthropic",
            &op_rotated.to_vault_json().unwrap(),
            None,
            None,
        )
        .unwrap();
    let panic_refresher = Arc::new(CountingRefresher {
        calls: AtomicUsize::new(0),
        barrier: None,
        response: Box::new(|_, _| panic!("must not call refresher when vault already rotated")),
    });
    let op_coord =
        VaultRefreshCoordinator::new(Arc::clone(&panic_refresher) as Arc<dyn VaultTokenRefresher>);
    let adopted = op_coord
        .refresh_reactive(&vault, "conn-op", "anthropic", Some(&fp_t1), now)
        .unwrap();
    assert_eq!(adopted.access_token.as_str(), "sk-ant-oat01-operator-t2");
    assert_eq!(panic_refresher.calls.load(Ordering::SeqCst), 0);

    // 3. Expired token without refresh_token or with NoopVaultTokenRefresher fails closed.
    let no_rt = VaultOAuthCredential {
        access_token: Zeroizing::new("sk-ant-oat01-expired-nort".into()),
        refresh_token: None,
        expires_at_ms: Some(1),
    };
    vault
        .store_wrapped(
            "conn-nort",
            "anthropic",
            &no_rt.to_vault_json().unwrap(),
            None,
            None,
        )
        .unwrap();
    let err_nort = noop
        .resolve_for_launch(&vault, "conn-nort", "anthropic", now)
        .err()
        .expect("expired token without refresh_token must fail");
    assert_eq!(err_nort.issue_code(), "vault_oauth_expired");

    // 4. Refresher error containing secrets + multibyte UTF-8 is redacted & char-safe truncated.
    let with_rt = VaultOAuthCredential {
        access_token: Zeroizing::new("custom-secret-access-99".into()),
        refresh_token: Some(Zeroizing::new("custom-secret-refresh-88".into())),
        expires_at_ms: Some(1),
    };
    vault
        .store_wrapped(
            "conn-err",
            "anthropic",
            &with_rt.to_vault_json().unwrap(),
            None,
            None,
        )
        .unwrap();
    let leaky = Arc::new(CountingRefresher {
        calls: AtomicUsize::new(0),
        barrier: None,
        response: Box::new(|_, rt| {
            Err(format!(
                "upstream rejected custom-secret-access-99 and {rt} {}",
                "é".repeat(150)
            ))
        }),
    });
    let err_coord = VaultRefreshCoordinator::new(leaky);
    let leaky_err = err_coord
        .resolve_for_launch(&vault, "conn-err", "anthropic", now)
        .err()
        .expect("refresher failure must fail closed");
    let msg = leaky_err.actionable_message("claude-err");
    assert!(
        !msg.contains("custom-secret-access-99"),
        "leaked access: {msg}"
    );
    assert!(
        !msg.contains("custom-secret-refresh-88"),
        "leaked refresh: {msg}"
    );
    assert!(
        msg.contains("[REDACTED]"),
        "expected redaction marker: {msg}"
    );

    // 5. Reactive refresh returning the SAME rejected token (unrotated) is refused cleanly.
    let same_fp = oauth_token_fingerprint("custom-secret-access-99");
    let unrotated_refresher = Arc::new(CountingRefresher {
        calls: AtomicUsize::new(0),
        barrier: None,
        response: Box::new(move |_, _| {
            Ok(VaultOAuthCredential {
                access_token: Zeroizing::new("custom-secret-access-99".into()),
                refresh_token: None,
                expires_at_ms: Some(future_exp),
            })
        }),
    });
    let unrot_coord = VaultRefreshCoordinator::new(unrotated_refresher);
    let unrot_err = unrot_coord
        .refresh_reactive(&vault, "conn-err", "anthropic", Some(&same_fp), now)
        .err()
        .expect("unrotated token in reactive refresh must be refused");
    assert_eq!(unrot_err.issue_code(), "auth_expired");
    // Vault must NOT have been overwritten by the rejected unrotated token.
    let still_old =
        parse_vault_oauth_credential(&vault.load_unwrapped("conn-err").unwrap().unwrap()).unwrap();
    assert_eq!(still_old.expires_at_ms, Some(1));

    // Confirm NoopVaultTokenRefresher directly returns Err.
    assert!(NoopVaultTokenRefresher
        .refresh("conn-x", "anthropic", "rt")
        .is_err());
}

#[test]
fn metadata_expires_at_enforced_for_expired_future_and_unparseable() {
    let dir = tempfile::tempdir().unwrap();
    let vault = test_vault(dir.path());
    let now = 2_000_000_000_000u64;
    let noop = VaultRefreshCoordinator::noop();

    // 1. Expired CredentialMetadata.expires_at on a raw token fails closed.
    vault
        .store_wrapped(
            "conn-raw-exp",
            "anthropic",
            b"sk-ant-oat01-raw-exp",
            Some("2020-01-01T00:00:00Z".into()),
            None,
        )
        .unwrap();
    let err = noop
        .resolve_for_launch(&vault, "conn-raw-exp", "anthropic", now)
        .err()
        .expect("expired metadata expires_at must fail closed");
    assert_eq!(err.issue_code(), "vault_oauth_expired");

    // 2. Unparseable CredentialMetadata.expires_at fails closed as MalformedCredential.
    vault
        .store_wrapped(
            "conn-bad-exp",
            "anthropic",
            b"sk-ant-oat01-raw-bad-exp",
            Some("not-a-valid-rfc3339".into()),
            None,
        )
        .unwrap();
    let bad = noop
        .resolve_for_launch(&vault, "conn-bad-exp", "anthropic", now)
        .err()
        .expect("unparseable metadata expires_at must fail closed");
    assert!(matches!(bad, VaultAuthError::MalformedCredential(_)));

    // 3. Future metadata expires_at populates resolved.expires_at_ms for warm recycle.
    vault
        .store_wrapped(
            "conn-future-exp",
            "anthropic",
            b"sk-ant-oat01-raw-future",
            Some("2099-01-01T00:00:00Z".into()),
            None,
        )
        .unwrap();
    let ok = noop
        .resolve_for_launch(&vault, "conn-future-exp", "anthropic", now)
        .unwrap();
    assert_eq!(
        ok.expires_at_ms,
        org_proto::parse_rfc3339_ms("2099-01-01T00:00:00Z")
    );

    // 4. JSON envelope without expires_at_ms inherits expired metadata expires_at and refreshes.
    let no_exp_json = br#"{"access_token":"sk-ant-oat01-old","refresh_token":"rt-meta-1"}"#;
    vault
        .store_wrapped(
            "conn-json-meta-exp",
            "anthropic",
            no_exp_json,
            Some("2020-01-01T00:00:00Z".into()),
            None,
        )
        .unwrap();
    let new_exp = now + 3_600_000;
    let refresher = Arc::new(CountingRefresher {
        calls: AtomicUsize::new(0),
        barrier: None,
        response: Box::new(move |_, rt| {
            assert_eq!(rt, "rt-meta-1");
            Ok(VaultOAuthCredential {
                access_token: Zeroizing::new("sk-ant-oat01-rotated-meta".into()),
                refresh_token: None,
                expires_at_ms: Some(new_exp),
            })
        }),
    });
    let coord =
        VaultRefreshCoordinator::new(Arc::clone(&refresher) as Arc<dyn VaultTokenRefresher>);
    let rotated = coord
        .resolve_for_launch(&vault, "conn-json-meta-exp", "anthropic", now)
        .unwrap();
    assert_eq!(rotated.access_token.as_str(), "sk-ant-oat01-rotated-meta");
    assert_eq!(refresher.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn missing_secrets_key_returns_unavailable_while_corrupt_file_maps_to_vault_auth() {
    let dir = tempfile::tempdir().unwrap();
    let vault_with_key = test_vault(dir.path());
    vault_with_key
        .store_wrapped("conn-key", "anthropic", b"sk-ant-oat01-valid", None, None)
        .unwrap();

    let vault_no_key = CredentialVault::new(dir.path().to_path_buf(), "org-1", "office-1", None);
    let claude = org_agent_runtime_claude::ClaudeRuntime::new();
    let noop = VaultRefreshCoordinator::noop();
    let now = 2_000_000_000_000u64;

    // MissingSecretsKey must return Fatal(Unavailable), NOT VaultAuth (which poisons AuthExpired).
    let res = super::resolve_connection_vault_auth(
        &vault_no_key,
        &noop,
        &claude,
        "conn-key",
        "anthropic",
        ProviderAuthMode::SubscriptionCli,
        now,
    );
    match res {
        Err(super::ConnectionAuthError::Fatal(err)) => {
            assert_eq!(err.code, org_proto::ErrorCode::Unavailable);
        }
        other => panic!(
            "expected Fatal(Unavailable) on MissingSecretsKey, got {}",
            other.is_ok()
        ),
    }

    // Crypto/format corruption still maps to VaultAuth(MalformedCredential).
    let path = vault_with_key.credential_path("conn-key").unwrap();
    std::fs::write(&path, b"{corrupt-metadata").unwrap();
    let corrupt = super::resolve_connection_vault_auth(
        &vault_with_key,
        &noop,
        &claude,
        "conn-key",
        "anthropic",
        ProviderAuthMode::SubscriptionCli,
        now,
    );
    assert!(matches!(
        corrupt,
        Err(super::ConnectionAuthError::VaultAuth(
            VaultAuthError::MalformedCredential(_)
        ))
    ));
}

#[test]
fn refresher_rejects_untrimmed_and_control_char_tokens_without_overwriting_vault() {
    let dir = tempfile::tempdir().unwrap();
    let vault = test_vault(dir.path());
    let now = 2_000_000_000_000u64;
    let future_exp = now + 3_600_000;

    // JSON parsing itself also rejects untrimmed or control-char token values.
    for bad_json in [
        br#"{"access_token":" sk-ant-padded "}"#.as_slice(),
        br#"{"access_token":"sk-ant-bel\u0007"}"#.as_slice(),
        br#"{"access_token":"sk-ant-ok","refresh_token":" rt-padded "}"#.as_slice(),
        br#"{"access_token":"sk-ant-ok","refresh_token":"rt\u001besc"}"#.as_slice(),
    ] {
        assert!(matches!(
            parse_vault_oauth_credential(bad_json),
            Err(VaultAuthError::MalformedCredential(_))
        ));
    }

    let initial = VaultOAuthCredential {
        access_token: Zeroizing::new("sk-old-token".into()),
        refresh_token: Some(Zeroizing::new("rt-valid-1".into())),
        expires_at_ms: Some(1),
    };
    vault
        .store_wrapped(
            "conn-ctrl",
            "anthropic",
            &initial.to_vault_json().unwrap(),
            None,
            None,
        )
        .unwrap();

    let bad_outputs: &[(&str, Option<&str>)] = &[
        (" sk-old-token ", None),
        ("sk-new-token\x07", None),
        ("sk-new-token\t", None),
        ("sk-new-token\x1b", None),
        ("sk-new-clean", Some(" rt-padded ")),
        ("sk-new-clean", Some("rt-bel\x07")),
    ];
    for &(bad_at, bad_rt) in bad_outputs {
        let refresher = Arc::new(CountingRefresher {
            calls: AtomicUsize::new(0),
            barrier: None,
            response: Box::new(move |_, _| {
                Ok(VaultOAuthCredential {
                    access_token: Zeroizing::new(bad_at.to_owned()),
                    refresh_token: bad_rt.map(|s| Zeroizing::new(s.to_owned())),
                    expires_at_ms: Some(future_exp),
                })
            }),
        });
        let coord = VaultRefreshCoordinator::new(refresher);
        let err = coord
            .resolve_for_launch(&vault, "conn-ctrl", "anthropic", now)
            .err()
            .expect("untrimmed or control-char refresher token must fail");
        assert!(matches!(err, VaultAuthError::ReauthRequired { .. }));
        let stored =
            parse_vault_oauth_credential(&vault.load_unwrapped("conn-ctrl").unwrap().unwrap())
                .unwrap();
        assert_eq!(stored.access_token.as_str(), "sk-old-token");
        assert_eq!(stored.expires_at_ms, Some(1));
    }
}

#[test]
fn refresh_lock_map_gc_enforces_max_entries_bound_and_keeps_active_locks() {
    let coord = VaultRefreshCoordinator::noop();
    let held = coord.connection_lock("conn-active");
    for i in 0..(super::MAX_REFRESH_LOCK_ENTRIES + 64) {
        let _ephemeral = coord.connection_lock(&format!("conn-gc-{i}"));
    }
    let map = coord.locks.lock().unwrap();
    assert!(
        map.len() <= super::MAX_REFRESH_LOCK_ENTRIES,
        "lock map must stay bounded at <= {}, got {}",
        super::MAX_REFRESH_LOCK_ENTRIES,
        map.len()
    );
    assert!(
        map.contains_key("conn-active"),
        "in-flight referenced connection lock must be retained across GC"
    );
    drop(map);
    drop(held);
}
