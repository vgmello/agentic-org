use super::*;
#[test]
fn missing_release_without_explicit_development_custody_refuses_before_registry_or_credentials() {
    let temp = tempfile::tempdir_in("/private/tmp").unwrap();
    let config = Config::new(
        temp.path().join("office-home"),
        temp.path().join("agentic-org-office"),
    );
    let listen:ListenConfig=serde_json::from_value(serde_json::json!({"scheme":"http_loopback","address":"127.0.0.1","port":9988,"origin":"http://127.0.0.1:9988","hsts":"off"})).unwrap();
    let result = BootstrapHost::start(
        config,
        &temp.path().join("locator"),
        &temp.path().join("runtime"),
        Some("fresh"),
        &temp.path().join("instance"),
        listen,
        &temp.path().join("no-listener.sock"),
    );
    assert!(result.is_err());
    assert_eq!(
        std::fs::read_dir(temp.path()).unwrap().count(),
        0,
        "no protected setup files before runtime admission"
    );
}
