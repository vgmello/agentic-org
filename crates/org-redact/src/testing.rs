//! A shared adversarial secret corpus for call-site redaction tests (`TASK-190`).
//!
//! Fake values only, assembled at runtime so no literal key shape is committed.

/// One line of text per secret shape every call site must strip, plus the fragments that must
/// never appear after redaction.
pub struct SecretCorpus {
    pub text: String,
    pub fragments: Vec<String>,
}

pub fn secret_corpus() -> SecretCorpus {
    let pem_body = "MIIEvQIBADANcorpusPEMbody0042";
    let jwt_payload = "eyJzdWIiOiJjb3JwdXMifQcorpus";
    let pat_tail = "11CORPUSPAT0042_abcdefghijklmnop";
    let aws_tail = "CORPUSAWSKEY0042";
    let slack_tail = "1111111111-2222222222-CorpusSlack42";
    let text = [
        format!("-----BEGIN PRIVATE KEY-----\n{pem_body}\n-----END PRIVATE KEY-----"),
        format!("jwt eyJhbGciOiJIUzI1NiJ9corpus.{jwt_payload}.c2lnY29ycHVz"),
        "authorization: bearer corpus-bearer-0042".to_owned(),
        "fetch https://corpus-user:corpus-pass-0042@example.test/api".to_owned(),
        "{\"refresh_token\":\"corpus-json-0042\",\"apiKey\":\"corpus-apikey-0042\"}".to_owned(),
        format!("github_pat_{pat_tail} AKIA{aws_tail} xoxb-{slack_tail}"),
    ]
    .join("\n");
    let fragments = [
        pem_body,
        jwt_payload,
        "corpus-bearer-0042",
        "corpus-pass-0042",
        "corpus-json-0042",
        "corpus-apikey-0042",
        pat_tail,
        aws_tail,
        slack_tail,
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    SecretCorpus { text, fragments }
}

/// Panics when any corpus fragment survived in `haystack`.
pub fn assert_no_corpus_secret(corpus: &SecretCorpus, haystack: &str, site: &str) {
    for fragment in &corpus.fragments {
        assert!(
            !haystack.contains(fragment.as_str()),
            "{site} leaked `{fragment}` in:\n{haystack}"
        );
    }
}
