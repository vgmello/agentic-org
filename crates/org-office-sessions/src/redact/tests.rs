//! Adversarial tests of the single redaction choke-point (`TASK-190`, rules §3.3 and §3.6).

use serde_json::json;

use super::{is_sensitive_key, redact_json, redact_text, REDACTED};

/// Fixture secrets (fake, built at runtime so no literal key shape is committed).
fn github_pat() -> String {
    format!(
        "github_pat_{}",
        "11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyz"
    )
}

fn aws_key_id() -> String {
    format!("AKIA{}", "Z7QX3TESTKEY0042")
}

fn aws_session_key_id() -> String {
    format!("ASIA{}", "Z7QX3TESTKEY0043")
}

fn slack_bot() -> String {
    format!("xoxb-{}", "1234567890-0987654321-AbCdEfGhIjKlMnOp")
}

fn jwt() -> String {
    format!(
        "eyJ{}.eyJ{}.{}",
        "hbGciOiJIUzI1NiJ9abc", "zdWIiOiJ0ZXN0In0xyz", "c2lnbmF0dXJlLXNpZw"
    )
}

fn pem_block() -> String {
    format!(
        "-----BEGIN RSA PRIVATE KEY-----\n{}\n{}\n-----END RSA PRIVATE KEY-----",
        "MIIEpAIBAAKCAQEAPEMLINEONE", "PEMLINETWOsecretmaterial"
    )
}

fn assert_clean(redacted: &str, secrets: &[&str]) {
    for secret in secrets {
        assert!(
            !redacted.contains(secret),
            "leaked `{secret}` in:\n{redacted}"
        );
    }
}

#[test]
fn every_required_secret_shape_is_redacted() {
    let pat = github_pat();
    let aws = aws_key_id();
    let slack = slack_bot();
    let session_aws = aws_session_key_id();
    // Assembled at runtime so the committed source holds no literal key shape (gitleaks).
    let openai_env = format!("sk-proj-{}", "envopenai0042abcdef");
    let anthropic_env = ["anthropic", "env", "0042"].join("-");
    let slack_user = format!("xoxp-{}", "SLACKUSER0042");
    let slack_app = format!("xoxa-{}", "SLACKAPP0042");
    let slack_refresh = format!("xoxr-{}", "SLACKREFRESH0042");
    let token = jwt();
    let pem = pem_block();
    let raw = format!(
        "pem:\n{pem}\njwt={token} \n\
         authorization: bearer lowercase-bearer-0042\n\
         Proxy-Authorization: BASIC YWRtaW46c2VjcmV0MTIz\n\
         clone https://deploy:hunter2-pass@git.example.com/repo.git\n\
         {{\"access_token\":\"json-access-0042\",\"apiKey\":\"json-apikey-0042\",\"password\":\"json-pw-0042\"}}\n\
         export OPENAI_API_KEY={openai_env} ANTHROPIC_API_KEY='{anthropic_env}'\n\
         pat {pat} aws {aws} slack {slack}\n\
         keys sk-ant-api03-ANTHROPICKEY0042 sk-live-STRIPEKEY0042 sk-svcacct-SVCACCT0042 \
         sk-admin-ADMINKEY0042 ghp_GHPTOKEN00420042 gho_GHOTOKEN00420042 ghs_GHSTOKEN00420042 \
         ghu_GHUTOKEN00420042 {session_aws} AIzaSyA-GOOGLEKEY0042abcdefghij \
         {slack_user} {slack_app} {slack_refresh}\n"
    );
    let redacted = redact_text(&raw, &[]);
    assert_clean(
        &redacted,
        &[
            "PEMLINEONE",
            "PEMLINETWOsecretmaterial",
            "hbGciOiJIUzI1NiJ9abc",
            "lowercase-bearer-0042",
            "YWRtaW46c2VjcmV0MTIz",
            "hunter2-pass",
            "json-access-0042",
            "json-apikey-0042",
            "json-pw-0042",
            "envopenai0042",
            "anthropic-env-0042",
            &pat,
            "abcdefghijklmnopqrstuvwxyz",
            &aws,
            "Z7QX3TESTKEY0042",
            &slack,
            "AbCdEfGhIjKlMnOp",
            "ANTHROPICKEY0042",
            "STRIPEKEY0042",
            "SVCACCT0042",
            "ADMINKEY0042",
            "GHPTOKEN0042",
            "GHOTOKEN0042",
            "GHSTOKEN0042",
            "GHUTOKEN0042",
            "Z7QX3TESTKEY0043",
            "GOOGLEKEY0042",
            "SLACKUSER0042",
            "SLACKAPP0042",
            "SLACKREFRESH0042",
        ],
    );
    assert!(redacted.contains("[REDACTED PRIVATE KEY]"));
    assert!(redacted.contains("https://[REDACTED]@git.example.com/repo.git"));
    assert!(redacted.contains("\"access_token\":\"[REDACTED]\""));
}

#[test]
fn active_secrets_win_even_when_one_contains_another() {
    let redacted = redact_text(
        "opaque-cred-abcdef and opaque-cred-abcdef-extended and opaq",
        // Below the 6-byte minimum: replaced only by the shape passes, never verbatim.
        &["opaque-cred-abcdef", "opaque-cred-abcdef-extended", "opaq"],
    );
    assert_eq!(redacted, "[REDACTED] and [REDACTED] and opaq");
}

#[test]
fn mixed_case_quotes_and_several_secrets_on_one_line() {
    let line = "BeArEr Tok3n-one-0042, \"Token\": 'quoted-two-0042', \
                X-Api-Key: header-three-0042 \\\"refresh_token\\\":\\\"escaped-four-0042\\\" \
                'password'='single-five-0042' url=https://u:p6-0042@h.example/x?access_token=q7-0042";
    let redacted = redact_text(line, &[]);
    assert_clean(
        &redacted,
        &[
            "Tok3n-one-0042",
            "quoted-two-0042",
            "header-three-0042",
            "escaped-four-0042",
            "single-five-0042",
            "p6-0042",
            "q7-0042",
        ],
    );
}

#[test]
fn a_secret_split_across_lines_or_left_unterminated_fails_closed() {
    // A PEM block without its END line: everything after BEGIN is key material.
    let truncated = "log start\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\n";
    let redacted = redact_text(truncated, &[]);
    assert_eq!(redacted, "log start\n[REDACTED PRIVATE KEY]");
    // A header-less truncation still names the key type.
    let cut = redact_text("x -----BEGIN EC PRIVATE KEY", &[]);
    assert_eq!(cut, "x [REDACTED PRIVATE KEY]");
    // An unterminated quoted value is redacted to the end of its line, not beyond.
    let open_quote = redact_text("{\"secret\": \"never-closed-0042\nnext line", &[]);
    assert_eq!(open_quote, "{\"secret\": \"[REDACTED]\nnext line");
    // Public PEM blocks are not secret and stay.
    let cert = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----";
    assert_eq!(redact_text(cert, &[]), cert);
}

#[test]
fn multibyte_text_next_to_secrets_never_panics_or_leaks() {
    let raw = format!(
        "é{}é token=ünïcode-sécret-0042 ✓Bearer ✓abcdef0042 \"api_key\":\"日本語-0042\" ghp_ÉÉÉ",
        github_pat()
    );
    let redacted = redact_text(&raw, &[]);
    assert_clean(&redacted, &[&github_pat(), "sécret-0042", "日本語-0042"]);
    // UTF-8 straddling the PEM label bound must not panic.
    let _ = redact_text(&format!("-----BEGIN {}PRIVATE", "é".repeat(40)), &[]);
    let _ = redact_text(&format!("\"{}\": \"x\"", "é".repeat(80)), &[]);
}

#[test]
fn prose_and_identifiers_stay_readable() {
    for text in [
        "a basic understanding of bearer instruments",
        "branch task-190-grade-a-sessions-and-more-words merged",
        "ASIAN_FOOD_RECIPES and AKIA_PLACEHOLDER",
        "ssh://git@github.com/org/repo.git",
        "claude-sonnet-4-5 and gpt-5-codex",
        "let token_count = a == b;",
        "std::io::Error and crate::token::Token",
    ] {
        assert_eq!(redact_text(text, &[]), text, "over-redacted: {text}");
    }
}

#[test]
fn redaction_is_idempotent() {
    let once = redact_text(
        &format!(
            "Authorization: Bearer abc-0042\nTOKEN=xyz-0042 {} {}",
            jwt(),
            pem_block()
        ),
        &[],
    );
    assert_eq!(redact_text(&once, &[]), once);
}

#[test]
fn json_values_are_redacted_structurally_and_stay_valid() {
    let value = json!({
        "type": "tool",
        "input": {
            "command": format!("curl -H 'Authorization: Bearer {}' {}", "live-0042", aws_key_id()),
            "headers": { "X-Api-Key": "hdr-0042", "accept": "json" },
            "client_secret": { "nested": "whole-object-0042" },
        },
        "output": [pem_block(), "plain"],
        "count": 3,
    });
    let redacted = redact_json(&value, &[]);
    let text = serde_json::to_string(&redacted).unwrap();
    assert_clean(
        &text,
        &[
            "live-0042",
            "Z7QX3TESTKEY0042",
            "hdr-0042",
            "whole-object-0042",
            "PEMLINEONE",
        ],
    );
    assert_eq!(redacted["input"]["headers"]["accept"], json!("json"));
    assert_eq!(redacted["input"]["client_secret"], json!(REDACTED));
    assert_eq!(redacted["count"], json!(3));
    assert_eq!(redacted["output"][1], json!("plain"));
}

#[test]
fn sensitive_key_names_ignore_case_and_punctuation() {
    for key in [
        "Authorization",
        "x-api-key",
        "X-Api-Key",
        "apiKey",
        "api_key",
        "OPENAI_API_KEY",
        "refresh_token",
        "client-secret",
        "Set-Cookie",
        "X-Amz-Signature",
        "sig",
    ] {
        assert!(is_sensitive_key(key), "{key}");
    }
    for key in ["input_tokens", "tokenizer", "model", "path", "accept"] {
        assert!(!is_sensitive_key(key), "{key}");
    }
}
