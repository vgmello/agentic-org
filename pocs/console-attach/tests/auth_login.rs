use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use console_attach::auth_login::{
    AuthFlowKind, AuthManager, AuthProvider, AuthState, StartLoginOptions,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    fn new(prefix: &str) -> Self {
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "/tmp/ca-auth-{prefix}-{}-{seq}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn mock_claude_paste_code_flow_authenticates_end_to_end() {
    let dir = TempDirGuard::new("mock-claude");
    let script = dir.path.join("mock-claude-auth.py");
    fs::write(
        &script,
        r#"#!/usr/bin/env python3
import sys
print("Opening browser to sign in…", flush=True)
print("If the browser didn't open, visit: https://claude.com/cai/oauth/authorize?code=true&state=test123", flush=True)
sys.stdout.write("Paste code here if prompted > ")
sys.stdout.flush()
code = sys.stdin.readline().strip()
if code == "SECRET-CODE-42":
    print("Login successful!", flush=True)
    sys.exit(0)
else:
    print(f"Invalid code: {code}", flush=True)
    sys.exit(1)
"#,
    )
    .expect("write mock script");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod +x");

    let mgr = AuthManager::new();
    let mut opts = StartLoginOptions::for_provider(AuthProvider::Claude);
    opts.binary_override = Some(script.to_string_lossy().into_owned());
    opts.config_dir = Some(dir.path.join("cfg"));

    let snap = mgr.start_login(opts).expect("start_login");
    assert_eq!(snap.flow, AuthFlowKind::PasteCode);
    assert_eq!(snap.state, AuthState::AwaitingCode);
    assert_eq!(
        snap.url.as_deref(),
        Some("https://claude.com/cai/oauth/authorize?code=true&state=test123")
    );

    mgr.submit_code(&snap.login_id, "SECRET-CODE-42")
        .expect("submit_code");

    let mut authed = false;
    for _ in 0..40 {
        let st = mgr.status(&snap.login_id).expect("status");
        if st.state == AuthState::Authenticated {
            authed = true;
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    assert!(
        authed,
        "expected Authenticated state after submitting valid code"
    );
}

#[test]
fn real_claude_login_extracts_live_oauth_url_in_isolated_dir() {
    let has_claude = std::process::Command::new("which")
        .arg("claude")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !has_claude {
        return;
    }

    let dir = TempDirGuard::new("real-claude");
    let mgr = AuthManager::new();
    let mut opts = StartLoginOptions::for_provider(AuthProvider::Claude);
    opts.config_dir = Some(dir.path.join("claude-cfg"));
    opts.ready_timeout = Duration::from_secs(10);

    let snap = mgr.start_login(opts).expect("start real claude login");
    let _ = mgr.cancel(&snap.login_id);

    assert_eq!(snap.flow, AuthFlowKind::PasteCode);
    assert_eq!(
        snap.state,
        AuthState::AwaitingCode,
        "unexpected snapshot: {snap:?}"
    );
    let url = snap.url.as_deref().unwrap_or("");
    assert!(
        url.starts_with("https://claude.com/") && url.contains("oauth/authorize"),
        "unexpected Claude OAuth URL: '{url}'"
    );
}

#[test]
fn real_codex_device_login_extracts_live_url_and_code_in_isolated_dir() {
    let has_codex = std::process::Command::new("which")
        .arg("codex")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !has_codex {
        return;
    }

    let dir = TempDirGuard::new("real-codex");
    let mgr = AuthManager::new();
    let mut opts = StartLoginOptions::for_provider(AuthProvider::Codex);
    opts.config_dir = Some(dir.path.join("codex-home"));
    opts.ready_timeout = Duration::from_secs(10);

    let snap = mgr.start_login(opts).expect("start real codex login");
    let _ = mgr.cancel(&snap.login_id);

    assert_eq!(snap.flow, AuthFlowKind::DeviceCode);
    assert_eq!(
        snap.state,
        AuthState::AwaitingDeviceAuth,
        "unexpected snapshot: {snap:?}"
    );
    assert_eq!(
        snap.url.as_deref(),
        Some("https://auth.openai.com/codex/device")
    );
    let code = snap.user_code.as_deref().unwrap_or("");
    assert!(
        code.contains('-') && code.len() >= 7,
        "unexpected Codex device code: '{code}'"
    );
}

#[test]
fn mock_claude_setup_token_extracts_and_redacts_oauth_token() {
    let dir = TempDirGuard::new("mock-claude-token");
    let script = dir.path.join("mock-claude-token.py");
    fs::write(
        &script,
        r#"#!/usr/bin/env python3
import sys
print("Browser didn't open? Use the url below to sign in", flush=True)
print("https://claude.com/cai/oauth/authorize?code=true&client_id=9d1c", flush=True)
print("250a&scope=user%3Ainference&state=tok123", flush=True)
sys.stdout.write("Paste code here if prompted > ")
sys.stdout.flush()
code = sys.stdin.readline().strip()
if code == "MINT-CODE-77":
    print("Your OAuth token (valid for 1 year):", flush=True)
    print("sk-ant-oat01-TESTTOKEN_1234567890_ABCDEF", flush=True)
    print("export CLAUDE_CODE_OAUTH_TOKEN=sk-ant-oat01-TESTTOKEN_1234567890_ABCDEF", flush=True)
    sys.exit(0)
sys.exit(1)
"#,
    )
    .expect("write mock setup-token script");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod +x");

    let mgr = AuthManager::new();
    let mut opts = StartLoginOptions::for_provider(AuthProvider::ClaudeToken);
    opts.binary_override = Some(script.to_string_lossy().into_owned());
    opts.config_dir = Some(dir.path.join("cfg"));

    let snap = mgr.start_login(opts).expect("start_login claude_token");
    assert_eq!(snap.flow, AuthFlowKind::PasteCode);
    assert_eq!(snap.state, AuthState::AwaitingCode);
    assert_eq!(
        snap.url.as_deref(),
        Some("https://claude.com/cai/oauth/authorize?code=true&client_id=9d1c250a&scope=user%3Ainference&state=tok123")
    );

    mgr.submit_code(&snap.login_id, "MINT-CODE-77")
        .expect("submit_code");

    let mut final_snap = None;
    for _ in 0..40 {
        let st = mgr.status(&snap.login_id).expect("status");
        if st.state == AuthState::Authenticated {
            final_snap = Some(st);
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    let st = final_snap.expect("expected Authenticated state");
    assert_eq!(
        st.oauth_token.as_deref(),
        Some("sk-ant-oat01-TESTTOKEN_1234567890_ABCDEF")
    );
    assert!(
        !st.output_preview
            .contains("sk-ant-oat01-TESTTOKEN_1234567890_ABCDEF"),
        "raw token should be redacted from output_preview"
    );
    assert!(st.output_preview.contains("sk-ant-oat01-***REDACTED***"));
}

#[test]
fn real_claude_setup_token_extracts_live_oauth_url() {
    let has_claude = std::process::Command::new("which")
        .arg("claude")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !has_claude {
        return;
    }

    let dir = TempDirGuard::new("real-claude-token");
    let mgr = AuthManager::new();
    let mut opts = StartLoginOptions::for_provider(AuthProvider::ClaudeToken);
    opts.config_dir = Some(dir.path.join("cfg"));
    opts.ready_timeout = Duration::from_secs(12);

    let snap = mgr
        .start_login(opts)
        .expect("start real claude setup-token");
    let _ = mgr.cancel(&snap.login_id);

    assert_eq!(snap.flow, AuthFlowKind::PasteCode);
    assert_eq!(
        snap.state,
        AuthState::AwaitingCode,
        "unexpected snapshot: {snap:?}"
    );
    let url = snap.url.as_deref().unwrap_or("");
    assert!(
        url.starts_with("https://claude.com/")
            && url.contains("oauth/authorize")
            && url.contains("user%3Ainference"),
        "unexpected Claude setup-token URL: '{url}'"
    );
}
