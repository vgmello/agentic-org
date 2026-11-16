use super::*;
#[test]
fn explicit_options_reject_join_secrets_duplicates_and_missing_automation() {
    for args in [
        vec!["--hq", "https://example.test"],
        vec!["--password", "forbidden"],
        vec!["--name", "../escape"],
        vec!["--offline", "--offline"],
        vec!["--non-interactive"],
        vec!["--config"],
    ] {
        assert!(options(&args.into_iter().map(str::to_owned).collect::<Vec<_>>()).is_err());
    }
    let args = [
        "--name",
        "lab",
        "--offline",
        "--non-interactive",
        "--config",
        "public.json",
        "--password-file",
        "private.txt",
    ]
    .map(str::to_owned);
    let parsed = options(&args).unwrap();
    assert_eq!(parsed.name.as_deref(), Some("lab"));
    assert!(parsed.offline && parsed.noninteractive);
}
#[test]
fn protected_password_input_is_bounded_and_never_enters_public_config() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("generated-password");
    fs::write(&path, b"generated temporary password\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let password = credential(Some(&path)).unwrap();
    assert_eq!(password.expose(), "generated temporary password");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(credential(Some(&path)).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&path, vec![b'x'; 1025]).unwrap();
    assert!(credential(Some(&path)).is_err());
    std::os::unix::fs::symlink(&path, temp.path().join("link")).unwrap();
    assert!(credential(Some(&temp.path().join("link"))).is_err());
    let bytes=serde_json::to_vec(&serde_json::json!({"version":1,"root":"/private/tmp/fixture","login":"Owner","organization":{"version":1,"name":"Example","slug":"example","repository_remote_url":"https://example.invalid/org.git","state_branch":"org/state","home_team_name":"Core"},"listen":{"address":"127.0.0.1","port":8080,"scheme":"http_loopback","origin":"http://127.0.0.1:8080","hsts":"off"},"password":"forbidden"})).unwrap();
    assert!(serde_json::from_slice::<Input>(&bytes).is_err());
}
#[test]
fn no_embedded_registry_refuses_before_any_account_or_setup_path() {
    if matches!(
        crate::release::require_build_trust(),
        Err(crate::release::Error::MissingBuildTrust)
    ) {
        let result = setup(&["--offline".into()]);
        assert!(result
            .unwrap_err()
            .contains("authentic trusted complete release"));
    }
}

#[test]
fn typed_native_bootstrap_request_round_trips_existing_wire_without_value_copies() {
    use org_proto::{frame, Call, Request, Response};
    use std::io::BufReader;
    let (client, server) = UnixStream::pair().unwrap();
    let server = std::thread::spawn(move || {
        let mut server = BufReader::new(server);
        let request: Request = frame::read_message(&mut server).unwrap().unwrap();
        assert_eq!(request.method, ops::Bootstrap::METHOD);
        assert!(request.principal.is_none());
        let params = request.params::<ops::Bootstrap>().unwrap();
        assert_eq!(
            params.password.expose(),
            "generated synthetic fixture password"
        );
        assert_eq!(params.login, "Owner");
        assert!(params.tls.is_none());
        frame::write_message(
            server.get_mut(),
            &Response::ok::<ops::Bootstrap>(
                request.id,
                &ops::BootstrapOutcome::Rejected {
                    refusal: ops::BootstrapRefusal::AlreadyBootstrapped,
                },
            )
            .unwrap(),
        )
        .unwrap();
    });
    let mut wire = Wire::new(client).unwrap();
    let result = wire
        .call::<ops::Bootstrap>(
            &ops::BootstrapParams {
                operation_id: crate::ids::generate().unwrap(),
                hq_operation_id: crate::ids::generate().unwrap(),
                login: "Owner".into(),
                password: Password::new("generated synthetic fixture password".into()).unwrap(),
                tls: None,
            },
            None,
        )
        .unwrap();
    assert!(matches!(
        result,
        ops::BootstrapOutcome::Rejected {
            refusal: ops::BootstrapRefusal::AlreadyBootstrapped
        }
    ));
    server.join().unwrap();
}
