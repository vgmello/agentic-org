//! Deterministic test helpers for `org-github` and downstream integration suites.
//!
//! Ephemeral 2048-bit RSA test keys are generated at runtime via `openssl` (cached per process in
//! a `OnceLock`) so ZERO literal private keys ever appear in source control or trigger secret
//! scanners.

use std::collections::VecDeque;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use ring::hmac;

use crate::error::GitHubError;
use crate::http::{GitHubHttpClient, GitHubHttpRequest, GitHubHttpResponse, HttpMethod};

/// Ephemeral 2048-bit RSA test key material in both PKCS#1 and PKCS#8 PEM encodings.
#[derive(Debug, Clone)]
pub struct TestRsaKeyPair {
    pub pkcs1_pem: String,
    pub pkcs8_pem: String,
}

static TEST_RSA_KEY: OnceLock<TestRsaKeyPair> = OnceLock::new();

/// Returns a process-cached ephemeral 2048-bit RSA keypair generated at test runtime.
pub fn test_rsa_keypair_pem() -> &'static TestRsaKeyPair {
    TEST_RSA_KEY.get_or_init(|| {
        let pkcs1_out = Command::new("openssl")
            .args(["genrsa", "2048"])
            .stderr(Stdio::null())
            .output()
            .expect("openssl genrsa 2048 must succeed in test environment");
        assert!(
            pkcs1_out.status.success(),
            "openssl genrsa 2048 exited with {}",
            pkcs1_out.status
        );
        let pkcs1_pem =
            String::from_utf8(pkcs1_out.stdout).expect("openssl PKCS#1 PEM must be UTF-8");

        let mut child = Command::new("openssl")
            .args(["pkcs8", "-topk8", "-nocrypt"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("openssl pkcs8 must spawn in test environment");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(pkcs1_pem.as_bytes())
            .expect("write PKCS#1 to openssl");
        let pkcs8_out = child.wait_with_output().expect("wait openssl pkcs8");
        assert!(
            pkcs8_out.status.success(),
            "openssl pkcs8 exited with {}",
            pkcs8_out.status
        );
        let pkcs8_pem =
            String::from_utf8(pkcs8_out.stdout).expect("openssl PKCS#8 PEM must be UTF-8");

        TestRsaKeyPair {
            pkcs1_pem,
            pkcs8_pem,
        }
    })
}

/// Computes a valid `X-Hub-Signature-256` header (`sha256=<64 lower hex>`) for test webhooks.
pub fn sign_webhook_hmac(secret: &str, payload: &[u8]) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let tag = hmac::sign(&key, payload);
    let mut hex = String::with_capacity(7 + 64);
    hex.push_str("sha256=");
    for b in tag.as_ref() {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

/// Recorded outbound HTTP call captured by [`FakeGitHubHttpClient`].
#[derive(Debug, Clone)]
pub struct RecordedGitHubCall {
    pub method: HttpMethod,
    pub url: String,
    pub bearer_token: String,
    pub json_body: Option<String>,
}

struct StubRule {
    method: HttpMethod,
    url_contains: String,
    responses: VecDeque<Result<GitHubHttpResponse, GitHubError>>,
    sticky_last: bool,
}

/// Deterministic in-memory [`GitHubHttpClient`] test double.
#[derive(Default)]
pub struct FakeGitHubHttpClient {
    rules: Mutex<Vec<StubRule>>,
    recorded: Mutex<Vec<RecordedGitHubCall>>,
}

impl FakeGitHubHttpClient {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a sticky JSON response returned whenever `method` matches and `url` contains
    /// `url_contains`.
    pub fn on_json(
        &self,
        method: HttpMethod,
        url_contains: impl Into<String>,
        status: u16,
        body: impl Into<String>,
    ) {
        self.push_rule(
            method,
            url_contains,
            vec![Ok(GitHubHttpResponse {
                status,
                body: body.into().into_bytes(),
            })],
            true,
        );
    }

    /// Registers a FIFO sequence of responses for repeated calls matching `(method, url_contains)`.
    pub fn on_sequence(
        &self,
        method: HttpMethod,
        url_contains: impl Into<String>,
        responses: Vec<Result<GitHubHttpResponse, GitHubError>>,
    ) {
        self.push_rule(method, url_contains, responses, false);
    }

    fn push_rule(
        &self,
        method: HttpMethod,
        url_contains: impl Into<String>,
        responses: Vec<Result<GitHubHttpResponse, GitHubError>>,
        sticky_last: bool,
    ) {
        let mut guard = self.rules.lock().unwrap_or_else(|e| e.into_inner());
        guard.push(StubRule {
            method,
            url_contains: url_contains.into(),
            responses: responses.into(),
            sticky_last,
        });
    }

    /// Returns a snapshot of every outbound request issued through this client.
    pub fn calls(&self) -> Vec<RecordedGitHubCall> {
        self.recorded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl GitHubHttpClient for FakeGitHubHttpClient {
    fn send(&self, req: &GitHubHttpRequest) -> Result<GitHubHttpResponse, GitHubError> {
        self.recorded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(RecordedGitHubCall {
                method: req.method,
                url: req.url.clone(),
                bearer_token: req.bearer_token.as_str().to_owned(),
                json_body: req.json_body.clone(),
            });

        let mut rules = self.rules.lock().unwrap_or_else(|e| e.into_inner());
        let best_idx = rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.method == req.method && req.url.contains(&rule.url_contains))
            .max_by_key(|(_, rule)| rule.url_contains.len())
            .map(|(idx, _)| idx);
        if let Some(idx) = best_idx {
            let rule = &mut rules[idx];
            if rule.responses.len() > 1 || !rule.sticky_last {
                if let Some(next) = rule.responses.pop_front() {
                    return next;
                }
            } else if let Some(first) = rule.responses.front() {
                return first.clone();
            }
        }
        Err(GitHubError::NotFound)
    }
}
