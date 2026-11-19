use super::*;

#[test]
fn pinned_full_default_casefold_and_nfc_vectors() {
    let vectors: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../crates/org-host-control/test-vectors/journal.json"
    ))
    .unwrap();
    assert_eq!(caseless::UNICODE_VERSION, (16, 0, 0));
    for case in vectors["login_normalization"].as_array().unwrap() {
        let output = normalize_login(case["input"].as_str().unwrap());
        assert_eq!(output, case["normalized"].as_str().unwrap());
        assert_eq!(normalize_login(&output), output);
    }
}
