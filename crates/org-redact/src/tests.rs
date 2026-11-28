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

#[test]
fn an_escaped_quote_never_ends_a_quoted_value_early() {
    // A `\"` inside a double-quoted `KEY="…"` value is part of the value (`TASK-190` review).
    let raw = redact_text(r#"PASSWORD="abc\"leaked_tail" user=bob"#, &[]);
    assert_eq!(raw, r#"PASSWORD="[REDACTED]" user=bob"#);
    assert_clean(&raw, &["leaked_tail", "abc"]);
    // An unclosed one fails closed to the end of its line, never beyond.
    let open = redact_text("TOKEN=\"abc\\\"leaked_tail\nnext line", &[]);
    assert_eq!(open, "TOKEN=\"[REDACTED]\nnext line");
    // JSON-escaped values: a deeper `\\\"` is an escaped quote inside the value.
    let in_json = redact_text(r#"{"cmd":"PASSWORD=\"abc\\\"leaked_tail\" next=1"}"#, &[]);
    assert_eq!(in_json, r#"{"cmd":"PASSWORD=\"[REDACTED]\" next=1"}"#);
    let escaped_key = redact_text(r#"{\"api_key\":\"abc\\\"leaked_tail\",\"ok\":1}"#, &[]);
    assert_eq!(escaped_key, r#"{\"api_key\":\"[REDACTED]\",\"ok\":1}"#);
}

#[test]
fn quoted_header_parameters_never_end_a_header_value_early() {
    // Raw HTTP lines: quoted parameters pair up, so the whole value to the end of the line goes.
    let digest = redact_text(
        "Authorization: Digest username=\"alice\", response=\"9f86d081884c7d659a2feaa0c55ad015\"\nnext: visible",
        &[],
    );
    assert_eq!(digest, "Authorization: [REDACTED]\nnext: visible");
    let cookie = redact_text("Cookie: a=\"1\", sid=secretcookie123", &[]);
    assert_eq!(cookie, "Cookie: [REDACTED]");
    let escaped = redact_text(
        r#"Authorization: Digest realm="r\"x", response="5ecret0042""#,
        &[],
    );
    assert_eq!(escaped, "Authorization: [REDACTED]");

    // JSON-embedded lines: the value ends at the JSON string's closing quote, keeping the JSON.
    let bearer = redact_text(
        r#"{"msg":"Authorization: Bearer topsecret","ok":true}"#,
        &[],
    );
    assert_clean(&bearer, &["topsecret"]);
    let parsed: serde_json::Value = serde_json::from_str(&bearer).expect(&bearer);
    assert_eq!(parsed["ok"], json!(true));
    let json_digest = redact_text(
        r#"{"msg":"Authorization: Digest username=\"alice\", response=\"deadbeef1234\"","ok":true}"#,
        &[],
    );
    assert_eq!(
        json_digest,
        r#"{"msg":"Authorization: [REDACTED]","ok":true}"#
    );
    let parsed: serde_json::Value = serde_json::from_str(&json_digest).expect(&json_digest);
    assert_eq!(parsed["ok"], json!(true));
    // A JSON-escaped string holding the header, with doubly escaped parameter quotes.
    let nested = redact_text(
        r#"{\"msg\":\"Cookie: a=\\\"1\\\", sid=secretcookie123\",\"ok\":true}"#,
        &[],
    );
    assert_eq!(nested, r#"{\"msg\":\"Cookie: [REDACTED]\",\"ok\":true}"#);
    // An unclosed parameter quote inside a JSON string stops at that string's end.
    let unclosed = redact_text(
        r#"{"msg":"Cookie: a=\"1, sid=secretcookie123","ok":1}"#,
        &[],
    );
    assert_eq!(unclosed, r#"{"msg":"Cookie: [REDACTED]","ok":1}"#);
    // An escaped line break outside a parameter ends the header line.
    let lines = redact_text(r#"{"log":"Cookie: sid=abc123secret\nAccept: json"}"#, &[]);
    assert_eq!(lines, r#"{"log":"Cookie: [REDACTED]\nAccept: json"}"#);
    // An empty header value inside JSON redacts nothing and keeps the structure.
    let empty = r#"{"msg":"Authorization:","ok":true}"#;
    assert_eq!(redact_text(empty, &[]), empty);
}

#[test]
fn a_quoted_parameter_starting_with_json_punctuation_is_still_redacted_whole() {
    // `,`, `}` or `]` right after a parameter's opening quote never makes it look like the end of
    // a JSON string when the quote has a partner (`TASK-190` pass 2).
    for raw in [
        "Cookie: a=\",\", sid=secretcookie123",
        "Cookie: a=\"}\", sid=secretcookie123",
        "Cookie: a=\"]\", sid=secretcookie123",
    ] {
        assert_eq!(redact_text(raw, &[]), "Cookie: [REDACTED]", "{raw}");
    }
    let digest = redact_text(
        r#"Authorization: Digest realm=",", response="deadbeef1234""#,
        &[],
    );
    assert_eq!(digest, "Authorization: [REDACTED]");

    // Inside a JSON string the parameter quotes are escaped; the JSON structure survives.
    let in_json = redact_text(
        r#"{"msg":"Cookie: a=\",\", sid=secretcookie123","ok":true}"#,
        &[],
    );
    assert_eq!(in_json, r#"{"msg":"Cookie: [REDACTED]","ok":true}"#);
    let json_digest = redact_text(
        r#"{"msg":"Authorization: Digest realm=\",\", response=\"deadbeef1234\"","ok":true}"#,
        &[],
    );
    assert_eq!(
        json_digest,
        r#"{"msg":"Authorization: [REDACTED]","ok":true}"#
    );
    let bearer = redact_text(
        r#"{"msg":"Authorization: Bearer topsecret","ok":true}"#,
        &[],
    );
    for text in [&in_json, &json_digest, &bearer] {
        assert_clean(text, &["secretcookie123", "deadbeef1234", "topsecret"]);
        let parsed: serde_json::Value = serde_json::from_str(text).expect(text);
        assert_eq!(parsed["ok"], json!(true), "{text}");
    }
    // A quote ending the JSON string right after the header is not paired with the next key.
    let empty_cookie = r#"{"msg":"Cookie: a=","ok":true}"#;
    let kept = redact_text(empty_cookie, &[]);
    let parsed: serde_json::Value = serde_json::from_str(&kept).expect(&kept);
    assert_eq!(parsed["ok"], json!(true));
}

#[test]
fn json_escaped_line_breaks_and_tabs_are_word_and_line_boundaries() {
    let aws = aws_key_id();
    let cases = [
        (
            r#"{"log":"Accept: json\nCookie: sid=abc123secret"}"#.to_owned(),
            "abc123secret",
        ),
        (
            r#"{"msg":"Host: api.example.com\nX-Api-Key: custom_opaque_secret"}"#.to_owned(),
            "custom_opaque_secret",
        ),
        (
            r#"{"msg":"line1\nsk-ant-api03-secret1234"}"#.to_owned(),
            "secret1234",
        ),
        (
            r#"{"msg":"line1\nBearer TopSecret999"}"#.to_owned(),
            "TopSecret999",
        ),
        (format!(r#"{{"msg":"line1\n{aws}"}}"#), "Z7QX3TESTKEY0042"),
        (
            r#"{"env":"FOO=1\nPWD=mysecret123"}"#.to_owned(),
            "mysecret123",
        ),
        (
            r#"{"env":"FOO=1\r\nTOKEN=crlfsecret0042"}"#.to_owned(),
            "crlfsecret0042",
        ),
        (
            r#"{"msg":"col1\tBearer TabSecret999"}"#.to_owned(),
            "TabSecret999",
        ),
        (
            r#"{"env":"FOO=1\tPASSWORD=tabsecret0042"}"#.to_owned(),
            "tabsecret0042",
        ),
        (
            r#"{"msg":"a: b\tCookie: sid=tabcookie0042"}"#.to_owned(),
            "tabcookie0042",
        ),
    ];
    for (raw, secret) in &cases {
        let redacted = redact_text(raw, &[]);
        assert_clean(&redacted, &[secret]);
        assert!(redacted.contains(REDACTED), "{redacted}");
        serde_json::from_str::<serde_json::Value>(&redacted).expect(&redacted);
    }
    // The next line keeps its own name, and an unrelated line stays readable.
    let lines = redact_text(r#"{"log":"Accept: json\nCookie: sid=abc123secret"}"#, &[]);
    assert_eq!(lines, r#"{"log":"Accept: json\nCookie: [REDACTED]"}"#);
    let env = redact_text(r#"{"env":"FOO=1\nPWD=mysecret123"}"#, &[]);
    assert_eq!(env, r#"{"env":"FOO=1\nPWD=[REDACTED]"}"#);

    // An HTTP dump with CRLF escapes: every secret goes and the JSON stays valid.
    let http = redact_text(
        r#"{"msg":"GET / HTTP/1.1\r\nHost: api.example.com\r\nAuthorization: Digest username=\"alice\", response=\"deadbeef1234\"","ok":true}"#,
        &[],
    );
    assert_eq!(
        http,
        r#"{"msg":"GET / HTTP/1.1\r\nHost: api.example.com\r\nAuthorization: [REDACTED]","ok":true}"#
    );
    let parsed: serde_json::Value = serde_json::from_str(&http).expect(&http);
    assert_eq!(parsed["ok"], json!(true));
}

#[test]
fn an_escaped_backslash_before_n_is_not_a_json_line_break() {
    // `\\n` is a literal backslash and `n`: `nBearer` and `nPWD` are single words.
    for text in [r"foo\\nBearer TopSecret999", r"foo\\nPWD=visible123"] {
        assert_eq!(redact_text(text, &[]), text, "over-redacted: {text}");
    }
    // An escaped backslash inside a value is part of the value, never its end.
    let value = redact_text(r#"{"env":"PWD=abc\\def\/ghi","ok":1}"#, &[]);
    assert_eq!(value, r#"{"env":"PWD=[REDACTED]","ok":1}"#);
}

#[test]
fn auth_scheme_stops_at_json_escapes_without_leaking_next_line_or_breaking_json() {
    let cases = [
        (
            r#"{"msg":"line1\nBearer TopSecret999\nCookie: sid=abc123secret","ok":true}"#,
            &["TopSecret999", "abc123secret"][..],
        ),
        (
            r#"{"msg":"line1\r\nBasic QWRtaW46c2VjcmV0\r\nX-Api-Key: custom_opaque_secret","ok":true}"#,
            &["QWRtaW46c2VjcmV0", "custom_opaque_secret"][..],
        ),
        (
            r#"{"msg":"Bearer TopSecret999\n\"token\": \"secret123\"","ok":true}"#,
            &["TopSecret999", "secret123"][..],
        ),
        (
            r#"{"msg":"\"Bearer TopSecret999\"","ok":true}"#,
            &["TopSecret999"][..],
        ),
        (
            r#"{"msg":"Bearer TopSecret999\tCookie: sid=tabsecret777","ok":true}"#,
            &["TopSecret999", "tabsecret777"][..],
        ),
    ];
    for (raw, secrets) in cases {
        let redacted = redact_text(raw, &[]);
        assert_clean(&redacted, secrets);
        let parsed: serde_json::Value = serde_json::from_str(&redacted).expect(&redacted);
        assert_eq!(parsed["ok"], json!(true), "{redacted}");
    }
    // Empty/escaped sibling JSON keys after an empty Authorization header stay valid JSON.
    for valid in [
        r#"{"msg":"Authorization:","":true}"#,
        r#"["Authorization:",""]"#,
        r#"{"msg":"Authorization:","\u006fk":true}"#,
    ] {
        let redacted = redact_text(valid, &[]);
        serde_json::from_str::<serde_json::Value>(&redacted).expect(&redacted);
    }
}

#[test]
fn session_proof_cookies_vault_credentials_and_passphrases_are_redacted() {
    let vault_tok = ["hvs.", "vault-tok-445566"].join("");
    let raw = [
        "X-Org-Session-Proof: proof-secret-99887766".to_owned(),
        "Set-Cookie: __Host-org-default=cookie-secret-112233; HttpOnly; Path=/".to_owned(),
        format!("X-Vault-Token: {vault_tok}"),
        ["VAULT_CREDENTIAL=", "vault-cred-778899"].join(""),
        "BACKUP_PASSPHRASE='my-passphrase-0042'".to_owned(),
        r#"{"session_cookie":"sc-secret-123","recovery_code":"ABCD-EFGH-IJKL-MNOP","vault_secret":"vs-999"}"#.to_owned(),
    ]
    .join("\n");
    let redacted = redact_text(&raw, &[]);
    assert_clean(
        &redacted,
        &[
            "proof-secret-99887766",
            "cookie-secret-112233",
            vault_tok.as_str(),
            "vault-cred-778899",
            "my-passphrase-0042",
            "sc-secret-123",
            "ABCD-EFGH-IJKL-MNOP",
            "vs-999",
        ],
    );
    assert!(is_sensitive_key("X-Org-Session-Proof"));
    assert!(is_sensitive_key("vault_credential"));
    assert!(is_sensitive_key("backup_passphrase"));
}

#[test]
fn task272_github_app_pems_installation_tokens_and_webhook_secrets_are_redacted() {
    let pkcs8 = format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----",
        "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwTESTSECRET"
    );
    let ghs = format!("ghs_{}", "InstallToken0123456789ABCDEF");
    let ghp = format!("ghp_{}", "PersonalToken0123456789ABCDEF");
    let pat = github_pat();
    let whsec = format!("whsec_{}", "WebhookSecret0123456789ABCDEF");
    let sig_hex = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
    let raw = format!(
        "café {ghs} 日本語 {ghp} {pat} {whsec}\n\
         X-Hub-Signature-256: sha256={sig_hex}\n\
         x-hub-signature: sha1=deadbeef0123456789abcdef\n\
         {{\"private_key_pem\":\"PEM_VALUE_001\",\"webhook_secret\":\"HOOK_VAL_002\",\"ok\":true}}\n\
         {pkcs8}\n"
    );
    let scrubbed = redact_text(&raw, &[]);
    assert_clean(
        &scrubbed,
        &[
            &ghs,
            "InstallToken0123456789ABCDEF",
            &ghp,
            "PersonalToken0123456789ABCDEF",
            &pat,
            &whsec,
            "WebhookSecret0123456789ABCDEF",
            sig_hex,
            "deadbeef0123456789abcdef",
            "PEM_VALUE_001",
            "HOOK_VAL_002",
            "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwTESTSECRET",
        ],
    );
    assert!(scrubbed.contains("café [REDACTED] 日本語 [REDACTED]"));
    assert!(scrubbed.contains("[REDACTED PRIVATE KEY]"));
    assert!(is_sensitive_key("private_key_pem"));
    assert!(is_sensitive_key("githubPrivateKeyPem"));
    assert!(is_sensitive_key("webhook_secret"));

    let obj = json!({
        "private_key_pem": pkcs8,
        "webhook_secret": "raw_hook_secret_value",
        "title": format!("Fix auth leak {ghs} café")
    });
    let scrubbed_json = redact_json(&obj, &[]);
    assert_eq!(scrubbed_json["private_key_pem"], json!(REDACTED));
    assert_eq!(scrubbed_json["webhook_secret"], json!(REDACTED));
    assert_eq!(
        scrubbed_json["title"],
        json!("Fix auth leak [REDACTED] café")
    );
}
