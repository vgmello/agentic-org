//! Provider CLI enrollment and wrapped credential vault (`ENROLL-1..6`).

use super::*;

#[test]
fn enrollment_vault_wraps_credentials_enforces_single_active_wipes_temp_and_redacts() {
    let dir = tempfile::tempdir().unwrap();
    let key = [0x42u8; 32];
    let vault = CredentialVault::new(dir.path().to_path_buf(), "org-1", "office-1", Some(key));
    let secret = "sk-ant-oat-secret-token-value-9999";
    let mgr = EnrollmentManager::with_timeout(
        vault.clone(),
        dir.path().join("enroll-tmp"),
        Arc::new(FakeEnrollmentDriver {
            secret: secret.to_owned(),
        }),
        Duration::from_secs(60),
    );

    // ENROLL-1: start creates 0700 temp dir; ENROLL-4: second concurrent start conflicts.
    let started = mgr.start("owner-1", "conn-1", "anthropic").unwrap();
    assert_eq!(started.state, EnrollmentState::AwaitingCode);
    let tmp_dir = mgr.temp_dir_for(&started.login_id);
    assert!(tmp_dir.exists());
    assert_eq!(
        fs::metadata(&tmp_dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(matches!(
        mgr.start("owner-1", "conn-1", "anthropic"),
        Err(EnrollmentVaultError::Conflict(_))
    ));

    // Wrong caller cannot inspect, submit, or cancel another caller's login_id (High #3).
    assert!(matches!(
        mgr.status("other-member", "conn-1", &started.login_id),
        Err(EnrollmentVaultError::NotAuthorized(_))
    ));
    assert!(matches!(
        mgr.submit_code("other-member", "conn-1", &started.login_id, "code-1234"),
        Err(EnrollmentVaultError::NotAuthorized(_))
    ));
    assert!(matches!(
        mgr.cancel("other-member", "conn-1", &started.login_id),
        Err(EnrollmentVaultError::NotAuthorized(_))
    ));

    // ENROLL-2: malformed code with whitespace/quotes rejected.
    assert!(matches!(
        mgr.submit_code(
            "owner-1",
            "conn-1",
            &started.login_id,
            "bad code\nwith newline"
        ),
        Err(EnrollmentVaultError::InvalidInput(_))
    ));

    // ENROLL-2/3: valid code wraps credential under KeyClass::ProviderCredential & wipes temp dir.
    let done = mgr
        .submit_code(
            "owner-1",
            "conn-1",
            &started.login_id,
            "callback-code-abc123",
        )
        .unwrap();
    assert_eq!(done.state, EnrollmentState::Completed);
    assert!(!tmp_dir.exists(), "temp dir must be wiped on completion");
    let raw_file = fs::read_to_string(vault.credential_path("conn-1").unwrap()).unwrap();
    assert!(
        !raw_file.contains(secret),
        "plaintext secret must never hit disk"
    );
    let unwrapped = vault.load_unwrapped("conn-1").unwrap().unwrap();
    assert_eq!(unwrapped.as_slice(), secret.as_bytes());

    // ENROLL-3: redaction scrubs known tokens, prefixes, and Bearer headers.
    let dirty = format!("Auth: Bearer {secret} and token sk-ant-api1234567890");
    let clean = redact_secrets(&dirty, &[secret]);
    assert!(!clean.contains(secret));
    assert!(!clean.contains("sk-ant-api1234567890"));
    assert!(clean.contains("[REDACTED]"));

    // ENROLL-5: revoke removes credential and status reports enrolled=false.
    assert!(vault.status("conn-1").unwrap().enrolled);
    let revoked = vault.revoke("conn-1").unwrap();
    assert!(!revoked.enrolled);
    assert!(vault.load_unwrapped("conn-1").unwrap().is_none());

    // ENROLL-4: expiry wipes temp dir automatically.
    let short_mgr = EnrollmentManager::with_timeout(
        vault,
        dir.path().join("enroll-tmp-short"),
        Arc::new(FakeEnrollmentDriver {
            secret: secret.to_owned(),
        }),
        Duration::from_millis(150),
    );
    let exp = short_mgr.start("owner-1", "conn-2", "openai").unwrap();
    let exp_dir = short_mgr.temp_dir_for(&exp.login_id);
    assert!(exp_dir.exists());
    std::thread::sleep(Duration::from_millis(200));
    let polled = short_mgr
        .status("owner-1", "conn-2", &exp.login_id)
        .unwrap();
    assert_eq!(polled.state, EnrollmentState::Expired);
    assert!(!exp_dir.exists(), "temp dir must be wiped on expiry");
}

#[test]
fn enrollment_concurrent_cancel_during_submit_never_resurrects_or_stores_credential() {
    use std::sync::Barrier;

    struct BarrierDriver {
        entered_submit: Arc<Barrier>,
        allow_finish: Arc<Barrier>,
    }

    impl EnrollmentDriver for BarrierDriver {
        fn start(
            &self,
            _provider: &str,
            _temp_dir: &Path,
        ) -> Result<EnrollmentPrompt, EnrollmentVaultError> {
            Ok(EnrollmentPrompt {
                state: EnrollmentState::AwaitingCode,
                auth_url: Some("https://example.test/auth".into()),
                device_code: None,
                message: None,
            })
        }

        fn submit_code(
            &self,
            _provider: &str,
            _temp_dir: &Path,
            _code: &str,
        ) -> Result<EnrollmentOutcome, EnrollmentVaultError> {
            self.entered_submit.wait();
            self.allow_finish.wait();
            Ok(EnrollmentOutcome::Completed {
                secret: zeroize::Zeroizing::new(b"sk-ant-oat-should-never-store".to_vec()),
                expires_at: None,
                account_hint: None,
            })
        }

        fn poll(
            &self,
            _provider: &str,
            _temp_dir: &Path,
        ) -> Result<EnrollmentOutcome, EnrollmentVaultError> {
            Ok(EnrollmentOutcome::Pending {
                state: EnrollmentState::AwaitingCode,
                message: None,
            })
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let vault = CredentialVault::new(
        dir.path().to_path_buf(),
        "org-1",
        "office-1",
        Some([0x55u8; 32]),
    );
    let entered = Arc::new(Barrier::new(2));
    let allow = Arc::new(Barrier::new(2));
    let mgr = Arc::new(EnrollmentManager::with_timeout(
        vault.clone(),
        dir.path().join("enroll-race"),
        Arc::new(BarrierDriver {
            entered_submit: Arc::clone(&entered),
            allow_finish: Arc::clone(&allow),
        }),
        Duration::from_secs(60),
    ));

    let started = mgr.start("owner-1", "conn-race", "anthropic").unwrap();
    let mgr_submit = Arc::clone(&mgr);
    let login_id = started.login_id.clone();
    let handle = std::thread::spawn(move || {
        mgr_submit.submit_code("owner-1", "conn-race", &login_id, "valid-code-123")
    });

    entered.wait();
    let cancelled = mgr
        .cancel("owner-1", "conn-race", &started.login_id)
        .unwrap();
    assert_eq!(cancelled.state, EnrollmentState::Cancelled);
    allow.wait();

    let submit_res = handle.join().unwrap();
    assert!(
        matches!(submit_res, Err(EnrollmentVaultError::Conflict(_))),
        "cancelled submit_code must fail with Conflict, got {submit_res:?}"
    );
    assert!(
        vault.load_unwrapped("conn-race").unwrap().is_none(),
        "credential must NOT be stored after cancel"
    );
    let polled = mgr
        .status("owner-1", "conn-race", &started.login_id)
        .unwrap();
    assert_eq!(polled.state, EnrollmentState::Cancelled);
    assert!(!mgr.temp_dir_for(&started.login_id).exists());
}

#[test]
fn enrollment_submit_command_cache_hashes_params_without_retaining_plaintext_code() {
    let office: Arc<Office> = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let sock_dir = tempfile::tempdir().unwrap();
    let mut cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    cfg.enrollment_driver = Arc::new(FakeEnrollmentDriver {
        secret: "sk-ant-oat-secret-vault-value".into(),
    });
    let manager = SessionManager::new(Arc::clone(&office), cfg);

    let conn_id = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c9".to_owned();
    let start_req = Request::new::<ProviderEnrollmentStart>(
        1,
        &ProviderEnrollmentStartParams {
            command_id: new_uuid_v7(),
            connection_id: conn_id.clone(),
            provider: Some("anthropic".into()),
        },
    )
    .unwrap();
    let start_resp = manager.dispatch_rpc(Some(&owner_id), &start_req).unwrap();
    let login_id = match start_resp.outcome {
        org_proto::Outcome::Result(v) => v["login_id"].as_str().unwrap().to_owned(),
        org_proto::Outcome::Error(e) => panic!("start failed: {e:?}"),
    };

    let raw_code = "sk-ant-oat-raw-callback-code-must-not-be-cached-9988";
    let submit_cmd_id = new_uuid_v7();
    let submit_req = Request::new::<org_proto::sessions::ProviderEnrollmentSubmit>(
        2,
        &org_proto::sessions::ProviderEnrollmentSubmitParams {
            command_id: submit_cmd_id.clone(),
            connection_id: conn_id.clone(),
            login_id: login_id.clone(),
            code: raw_code.to_owned(),
        },
    )
    .unwrap();
    let first = manager.dispatch_rpc(Some(&owner_id), &submit_req).unwrap();
    assert!(matches!(first.outcome, org_proto::Outcome::Result(_)));

    // Identical replay succeeds.
    let replay = manager.dispatch_rpc(Some(&owner_id), &submit_req).unwrap();
    assert!(matches!(replay.outcome, org_proto::Outcome::Result(_)));

    // Reusing same command_id with different code returns Conflict.
    let mismatch_req = Request::new::<org_proto::sessions::ProviderEnrollmentSubmit>(
        3,
        &org_proto::sessions::ProviderEnrollmentSubmitParams {
            command_id: submit_cmd_id,
            connection_id: conn_id,
            login_id,
            code: "different-code-123".into(),
        },
    )
    .unwrap();
    let mismatch = manager
        .dispatch_rpc(Some(&owner_id), &mismatch_req)
        .unwrap();
    assert!(matches!(
        mismatch.outcome,
        org_proto::Outcome::Error(e) if e.code == org_proto::ErrorCode::Conflict
    ));

    // Fingerprints in command_cache are SHA-256 hex digests and NEVER retain raw code.
    let fps = manager.command_cache_fingerprints();
    assert!(!fps.is_empty());
    for fp in &fps {
        assert_eq!(fp.len(), 64, "fingerprint must be 64-char SHA-256 hex");
        assert!(
            !fp.contains(raw_code),
            "raw enrollment code must never be stored in command_cache"
        );
    }
}
