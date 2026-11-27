use super::*;

#[test]
fn challenge_message_binds_purpose_office_member_session_and_nonce() {
    let a = challenge_message(ChallengePurpose::Exchange, "o", "m", "s", "n");
    assert!(a.starts_with(b"agentic-org/external-auth/v1\0exchange\0"));
    assert_eq!(
        a,
        challenge_message(ChallengePurpose::Exchange, "o", "m", "s", "n")
    );
    for other in [
        challenge_message(ChallengePurpose::Logout, "o", "m", "s", "n"),
        challenge_message(ChallengePurpose::Exchange, "o2", "m", "s", "n"),
        challenge_message(ChallengePurpose::Exchange, "o", "m2", "s", "n"),
        challenge_message(ChallengePurpose::Exchange, "o", "m", "s2", "n"),
        challenge_message(ChallengePurpose::Exchange, "o", "m", "s", "n2"),
        // Field boundaries are not ambiguous: "om" + "" differs from "o" + "m".
        challenge_message(ChallengePurpose::Exchange, "om", "", "s", "n"),
    ] {
        assert_ne!(a, other);
    }
}

#[test]
fn secrets_never_appear_in_debug_output() {
    let exchange = AuthExchangeResult {
        member_id: "m".into(),
        office_id: "o".into(),
        session_id: "s".into(),
        token: String::from("tok-very-secret").into(),
        expires_at: "2026-11-26T00:00:00Z".into(),
        lease_expires_at: "2026-11-27T00:00:00Z".into(),
    };
    assert!(!format!("{exchange:?}").contains("tok-very-secret"));
    // The token is a plain JSON string on the wire and reads back into wiped memory.
    let wire = serde_json::to_value(&exchange).unwrap();
    assert_eq!(wire["token"], "tok-very-secret");
    let back: AuthExchangeResult = serde_json::from_value(wire).unwrap();
    assert_eq!(back, exchange);
    let signed = SignedChallengeParams {
        member: "m".into(),
        session_id: "s".into(),
        nonce: "nonce-secret".into(),
        signature_ed25519_b64: "sig-secret".into(),
    };
    let rendered = format!("{signed:?}");
    assert!(!rendered.contains("nonce-secret") && !rendered.contains("sig-secret"));
}

#[test]
fn params_refuse_unknown_fields_and_unknown_enums_degrade() {
    let bad = serde_json::json!({"code": "ABCD-EFGH", "extra": 1});
    assert!(serde_json::from_value::<MemberExternalApproveParams>(bad).is_err());
    let ok: MemberExternalApproveParams =
        serde_json::from_value(serde_json::json!({"code": "ABCD-EFGH"})).unwrap();
    assert!(!ok.takeover);
    let state: ExternalLoginState = serde_json::from_str("\"from_the_future\"").unwrap();
    assert_eq!(state, ExternalLoginState::Unknown);
    let mode: SessionMode = serde_json::from_str("\"external\"").unwrap();
    assert_eq!(mode, SessionMode::External);
}

#[test]
fn external_bind_params_reject_unknown_fields_and_result_round_trips() {
    let bad = serde_json::json!({
        "employee_id": "@ada",
        "pubkey_ed25519_b64": "AAAA",
        "workspace_label": "~/src/app",
        "unexpected": true
    });
    assert!(serde_json::from_value::<MemberExternalBindParams>(bad).is_err());

    let ok: MemberExternalBindParams = serde_json::from_value(serde_json::json!({
        "employee_id": "@ada",
        "pubkey_ed25519_b64": "AAAA",
        "workspace_label": "~/src/app"
    }))
    .unwrap();
    assert!(!ok.takeover);
    assert_eq!(ok.ttl_secs, None);
    assert_eq!(ok.command_id, None);

    let result = MemberExternalBindResult {
        office_id: "0193a5c0-0000-7000-8000-000000000001".into(),
        office_socket: Some("/run/o/000000000001/s.sock".into()),
        session: ExternalSessionView {
            session_id: "ext-0193a5c0-0000-7000-8000-000000000070".into(),
            employee_id: "0193a5c0-0000-7000-8000-000000000051".into(),
            handle: Some("ada".into()),
            workspace_label: "~/src/app".into(),
            pubkey_fingerprint: "3f9c2a7d10b4e8f1".into(),
            approved_by: "0193a5c0-0000-7000-8000-000000000002".into(),
            approved_at: "2026-11-27T00:00:00.000Z".into(),
            expires_at: "2026-11-28T00:00:00.000Z".into(),
            revoked_at: None,
            active: true,
            superseded_session_id: None,
            took_over_managed: false,
        },
    };
    let wire = serde_json::to_value(&result).unwrap();
    assert_eq!(wire["office_id"], "0193a5c0-0000-7000-8000-000000000001");
    assert_eq!(wire["office_socket"], "/run/o/000000000001/s.sock");
    assert_eq!(
        wire["session_id"],
        "ext-0193a5c0-0000-7000-8000-000000000070"
    );
    let back: MemberExternalBindResult = serde_json::from_value(wire).unwrap();
    assert_eq!(back, result);
}

#[test]
fn external_renew_and_pending_params_reject_unknown_fields_and_round_trip() {
    assert!(
        serde_json::from_value::<MemberExternalRenewParams>(serde_json::json!({
            "employee_id": "@ada",
            "extra": 1
        }))
        .is_err()
    );
    let renew: MemberExternalRenewParams = serde_json::from_value(serde_json::json!({
        "employee_id": "@ada",
        "ttl_secs": MAX_LEASE_TTL_SECS
    }))
    .unwrap();
    assert_eq!(renew.employee_id, "@ada");
    assert_eq!(renew.ttl_secs, Some(MAX_LEASE_TTL_SECS));

    assert!(
        serde_json::from_value::<MemberExternalPendingParams>(serde_json::json!({
            "unknown": true
        }))
        .is_err()
    );
    let all_pending: MemberExternalPendingParams =
        serde_json::from_value(serde_json::json!({})).unwrap();
    assert_eq!(all_pending.employee_id, None);

    let fam: WorkspaceFamiliarity = serde_json::from_str("\"future_variant\"").unwrap();
    assert_eq!(fam, WorkspaceFamiliarity::Unknown);

    let view = PendingExternalAuthView {
        request_id: "req-1".into(),
        handle: Some("@ada".into()),
        employee_id: "emp-1".into(),
        office_id: "off-1".into(),
        office_name: "HQ".into(),
        hostname: Some("macbook".into()),
        workspace_path: Some("/Users/dev/repo".into()),
        workspace_label: "~/repo".into(),
        pubkey_fingerprint: "3f9c2a7d10b4e8f1".into(),
        managed_turn_active: false,
        requested_at: "2026-11-27T00:00:00.000Z".into(),
        expires_at: "2026-11-27T00:10:00.000Z".into(),
        code_prefix: "K7QF-••••".into(),
        workspace_familiarity: WorkspaceFamiliarity::ReturningKnownKey,
        last_approved_by: Some("owner-1".into()),
        last_approved_at: Some("2026-11-26T10:00:00.000Z".into()),
    };
    let wire = serde_json::to_value(&view).unwrap();
    assert_eq!(wire["workspace_familiarity"], "returning_known_key");
    assert_eq!(wire["code_prefix"], "K7QF-••••");
    let back: PendingExternalAuthView = serde_json::from_value(wire).unwrap();
    assert_eq!(back, view);
}
