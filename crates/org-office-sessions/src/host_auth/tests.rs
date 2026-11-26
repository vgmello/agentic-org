//! Host auth unit tests: OAuth parsing and expiry, tmpdir placement, refresh rotation, and the\n//! launch-scoped Keychain cache.

use super::*;

#[test]
fn parses_claude_oauth_access_token_json() {
    let valid =
        br#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-example","expiresAt":1795620000000}}"#;
    let parsed = parse_claude_oauth_token(valid).unwrap();
    assert_eq!(parsed.access_token.as_str(), "sk-ant-oat01-example");
    assert_eq!(parsed.expires_at_ms, Some(1_795_620_000_000));
    assert!(is_oauth_expiring(
        parsed.expires_at_ms,
        1_795_620_000_000 - OAUTH_REFRESH_LEAD_MS
    ));
    assert!(!is_oauth_expiring(
        parsed.expires_at_ms,
        1_795_620_000_000 - OAUTH_REFRESH_LEAD_MS - 1
    ));
    assert!(!is_oauth_expiring(None, 1_795_620_000_000));
    assert!(parse_claude_oauth_token(br#"{"claudeAiOauth":{"accessToken":"  "}}"#).is_none());
    assert!(parse_claude_oauth_token(br#"not-json"#).is_none());
}

#[test]
fn keychain_cache_serves_one_launch_once_and_keeps_nothing_after_it() {
    use std::cell::Cell;
    let cache = KeychainTokenCache::new();
    let lookups = Cell::new(0u32);
    let lookup = || {
        lookups.set(lookups.get() + 1);
        Some(ClaudeOAuthToken {
            access_token: Zeroizing::new(format!("tok-{}", lookups.get())),
            expires_at_ms: Some(1),
        })
    };
    let ttl = Duration::from_secs(60);
    // Outside a launch nothing is cached.
    assert_eq!(
        cache
            .get_or_lookup(ttl, lookup)
            .unwrap()
            .access_token
            .as_str(),
        "tok-1"
    );
    assert_eq!(
        cache
            .get_or_lookup(ttl, lookup)
            .unwrap()
            .access_token
            .as_str(),
        "tok-2"
    );
    {
        let _launch = cache.scope();
        for _ in 0..4 {
            let tok = cache.get_or_lookup(ttl, lookup).unwrap();
            assert_eq!(tok.access_token.as_str(), "tok-3");
        }
        assert_eq!(lookups.get(), 3, "four reads in one launch run one lookup");
        cache.invalidate();
        let tok = cache.get_or_lookup(ttl, lookup).unwrap();
        assert_eq!(
            tok.access_token.as_str(),
            "tok-4",
            "a refresh probe forces a re-read"
        );
        let tok = cache.get_or_lookup(Duration::ZERO, lookup).unwrap();
        assert_eq!(
            tok.access_token.as_str(),
            "tok-5",
            "an expired answer is re-read"
        );
    }
    assert!(
        cache.state.lock().unwrap().entry.is_none(),
        "the token is dropped when the last launch ends"
    );
    {
        let _launch = cache.scope();
        assert!(cache.get_or_lookup(ttl, || None).is_none());
        assert!(cache
            .get_or_lookup(ttl, || panic!("cached negative answer must be reused"))
            .is_none());
    }
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
fn allows_host_subscription_auth_enforces_mode_and_empty_vault() {
    assert!(allows_host_subscription_auth(&[], None));
    assert!(allows_host_subscription_auth(
        &[],
        Some(ProviderAuthMode::SubscriptionCli)
    ));
    assert!(!allows_host_subscription_auth(
        &[],
        Some(ProviderAuthMode::ApiKey)
    ));
    assert!(!allows_host_subscription_auth(
        &[],
        Some(ProviderAuthMode::CloudIam)
    ));
    let non_empty = vec![(OsString::from("ANTHROPIC_API_KEY"), OsString::from("sk-1"))];
    assert!(!allows_host_subscription_auth(
        &non_empty,
        Some(ProviderAuthMode::SubscriptionCli)
    ));
}

#[test]
fn host_config_dir_for_claude_is_always_none_and_cred_read_is_bounded() {
    assert!(host_config_dir_for("claude", true).is_none());
    assert!(host_config_dir_for("codex", false).is_none());
    assert!(host_config_dir_for("agy", false).is_none());

    let dir = tempfile::tempdir().unwrap();
    let valid = dir.path().join("valid.json");
    std::fs::write(
        &valid,
        r#"{"claudeAiOauth":{"accessToken":"sk-ant-bounded-tok"}}"#,
    )
    .unwrap();
    assert_eq!(
        read_claude_credentials_from_path(&valid)
            .as_ref()
            .map(|e| e.access_token.as_str()),
        Some("sk-ant-bounded-tok")
    );

    let oversized = dir.path().join("huge.json");
    let pad = " ".repeat(MAX_HOST_CLAUDE_CRED_BYTES + 16);
    std::fs::write(
        &oversized,
        format!(r#"{{"claudeAiOauth":{{"accessToken":"sk-too-big"}}}}{pad}"#),
    )
    .unwrap();
    assert!(read_claude_credentials_from_path(&oversized).is_none());

    let link = dir.path().join("link.json");
    std::os::unix::fs::symlink(&valid, &link).unwrap();
    assert!(read_claude_credentials_from_path(&link).is_none());
}

#[test]
fn pick_fresher_oauth_entry_prefers_newer_keychain_over_stale_file() {
    let stale = ClaudeOAuthToken {
        access_token: Zeroizing::new("stale-file-tok".into()),
        expires_at_ms: Some(1_000),
    };
    let fresh = ClaudeOAuthToken {
        access_token: Zeroizing::new("fresh-keychain-tok".into()),
        expires_at_ms: Some(9_000_000),
    };
    let picked = pick_fresher_oauth_entry(Some(stale), Some(fresh)).unwrap();
    assert_eq!(picked.access_token.as_str(), "fresh-keychain-tok");
    assert_eq!(picked.expires_at_ms, Some(9_000_000));

    let no_exp = ClaudeOAuthToken {
        access_token: Zeroizing::new("no-exp".into()),
        expires_at_ms: None,
    };
    let with_exp = ClaudeOAuthToken {
        access_token: Zeroizing::new("with-exp".into()),
        expires_at_ms: Some(5_000),
    };
    let picked2 = pick_fresher_oauth_entry(Some(no_exp), Some(with_exp)).unwrap();
    assert_eq!(picked2.access_token.as_str(), "with-exp");
}

#[test]
fn refresh_host_claude_oauth_scrubs_env_isolates_flags_and_verifies_rotation() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let dir = tempfile::tempdir().unwrap();
    let capture = dir.path().join("capture.txt");
    let fake_claude = dir.path().join("fake-claude");
    let script = format!(
        "#!/bin/sh\nprintf 'ARGS=%s\\nPWD=%s\\nAPI=%s\\nOAUTH=%s\\nAUTOUPD=%s\\n' \"$*\" \"$(pwd)\" \"${{ANTHROPIC_API_KEY:-}}\" \"${{CLAUDE_CODE_OAUTH_TOKEN:-}}\" \"${{DISABLE_AUTOUPDATER:-}}\" > {}\nexit 0\n",
        capture.display()
    );
    std::fs::write(&fake_claude, script).unwrap();
    std::fs::set_permissions(&fake_claude, std::fs::Permissions::from_mode(0o755)).unwrap();

    // Case 1: script exits 0 but token does NOT rotate -> must return false.
    let unrotated = refresh_host_claude_oauth_with(&fake_claude, false, None, || {
        Some(ClaudeOAuthToken {
            access_token: Zeroizing::new("still-expired".into()),
            expires_at_ms: Some(1),
        })
    });
    assert!(
        !unrotated,
        "refresh must return false when token stays expired and unchanged"
    );
    let log = std::fs::read_to_string(&capture).unwrap();
    assert!(
        log.contains("--setting-sources  --tools  --disable-slash-commands --strict-mcp-config --mcp-config "),
        "refresh probe must pass isolation flags: {log}"
    );
    assert!(
        log.contains("API=\n") && log.contains("OAUTH=\n") && log.contains("AUTOUPD=1\n"),
        "refresh probe must scrub API/OAuth tokens and disable autoupdater: {log}"
    );
    assert!(
        log.contains("org-oauth-refresh-"),
        "refresh probe must run in isolated temp dir: {log}"
    );

    // Case 2: second read returns rotated token -> must return true.
    let called_once = Arc::new(AtomicBool::new(false));
    let called_clone = Arc::clone(&called_once);
    let rotated = refresh_host_claude_oauth_with(&fake_claude, false, None, move || {
        if !called_clone.swap(true, Ordering::SeqCst) {
            Some(ClaudeOAuthToken {
                access_token: Zeroizing::new("old-tok".into()),
                expires_at_ms: Some(1),
            })
        } else {
            Some(ClaudeOAuthToken {
                access_token: Zeroizing::new("new-rotated-tok".into()),
                expires_at_ms: Some(crate::credentials::now_ms() + 3_600_000),
            })
        }
    });
    assert!(
        rotated,
        "refresh must return true after verified token rotation"
    );
}

#[test]
fn refresh_host_claude_oauth_reactive_handles_peer_rotation_and_early_revocation() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let dir = tempfile::tempdir().unwrap();
    let capture = dir.path().join("capture.txt");
    let fake_claude = dir.path().join("fake-claude");
    std::fs::write(
        &fake_claude,
        format!("#!/bin/sh\necho ran > {}\nexit 0\n", capture.display()),
    )
    .unwrap();
    std::fs::set_permissions(&fake_claude, std::fs::Permissions::from_mode(0o755)).unwrap();

    // Case 1: reactive 401 refresh when another session/host already rotated `T1 -> T2`
    // MUST return true immediately WITHOUT spawning a probe or poisoning the shared pool.
    let future_exp = crate::credentials::now_ms() + 3_600_000;
    let fp_t1 = oauth_token_fingerprint("revoked-early");
    let peer_rotated =
        refresh_host_claude_oauth_with(&fake_claude, true, Some(&fp_t1), move || {
            Some(ClaudeOAuthToken {
                access_token: Zeroizing::new("already-rotated-by-peer".into()),
                expires_at_ms: Some(future_exp),
            })
        });
    assert!(
        peer_rotated,
        "reactive refresh must succeed immediately when host token already rotated away from failed_token_fp"
    );
    assert!(
        !capture.exists(),
        "already-rotated reactive refresh must not spawn redundant probe"
    );

    // Case 2: reactive 401 refresh on early-revoked token whose local expiresAt is still
    // in the future and still matches failed_token_fp MUST probe and require rotation.
    let unchanged_reactive =
        refresh_host_claude_oauth_with(&fake_claude, true, Some(&fp_t1), move || {
            Some(ClaudeOAuthToken {
                access_token: Zeroizing::new("revoked-early".into()),
                expires_at_ms: Some(future_exp),
            })
        });
    assert!(
        !unchanged_reactive,
        "reactive refresh must require token rotation when local token still matches failed_token_fp"
    );
    let reactive_flag = Arc::new(AtomicBool::new(false));
    let reactive_clone = Arc::clone(&reactive_flag);
    let rotated_reactive =
        refresh_host_claude_oauth_with(&fake_claude, true, Some(&fp_t1), move || {
            if !reactive_clone.swap(true, Ordering::SeqCst) {
                Some(ClaudeOAuthToken {
                    access_token: Zeroizing::new("revoked-early".into()),
                    expires_at_ms: Some(future_exp),
                })
            } else {
                Some(ClaudeOAuthToken {
                    access_token: Zeroizing::new("fresh-rotated-early".into()),
                    expires_at_ms: Some(future_exp + 1_000),
                })
            }
        });
    assert!(
        rotated_reactive,
        "reactive refresh must return true when early-revoked token rotates"
    );
}
