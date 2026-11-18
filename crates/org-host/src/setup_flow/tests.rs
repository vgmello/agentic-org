use super::*;

#[test]
fn ports_default_to_7420_and_named_instances_take_the_first_free_from_7421() {
    assert_eq!(choose_port(None, None, |_| true, &[]).unwrap(), 7420);
    assert_eq!(
        choose_port(Some("default"), None, |_| true, &[]).unwrap(),
        7420
    );
    assert_eq!(
        choose_port(Some("lab"), None, |p| p != 7421, &[]).unwrap(),
        7422
    );
    assert_eq!(
        choose_port(Some("lab"), Some(9000), |_| true, &[]).unwrap(),
        9000
    );
}

#[test]
fn ports_other_instances_saved_are_never_chosen() {
    // A stopped instance's saved port is free to bind, but it is still that instance's.
    let saved = [("a".to_owned(), 7421), ("b".to_owned(), 7423)];
    assert_eq!(
        choose_port(Some("lab"), None, |p| p != 7422, &saved).unwrap(),
        7424
    );
    let why = choose_port(Some("lab"), Some(7423), |_| true, &saved).unwrap_err();
    assert!(
        why.contains("saved address of instance \"b\"") && why.contains("--port"),
        "{why}"
    );
    let taken = [("lab".to_owned(), DEFAULT_PORT)];
    let why = choose_port(None, None, |_| true, &taken).unwrap_err();
    assert!(
        why.contains("instance \"lab\"") && why.contains("nothing was saved"),
        "{why}"
    );
}

#[test]
fn a_held_saved_port_names_the_port_and_the_way_out() {
    let why = port_held_elsewhere(7425, Some("lab"));
    assert!(
        why.contains("port 7425") && why.contains("another program"),
        "{why}"
    );
    assert!(why.ends_with("run agentic-org setup --name lab"), "{why}");
    assert!(port_held_elsewhere(7420, None).ends_with("run agentic-org setup"));
}

#[test]
fn an_occupied_port_is_an_actionable_error() {
    for (name, requested) in [(None, None), (None, Some(9000)), (Some("lab"), Some(9000))] {
        let why = choose_port(name, requested, |_| false, &[]).unwrap_err();
        assert!(why.contains("in use") && why.contains("--port"), "{why}");
        assert!(why.contains("nothing was saved"), "{why}");
    }
    assert!(choose_port(None, Some(0), |_| true, &[]).is_err());
}

#[test]
fn the_protection_report_states_encryption_modes_and_isolation() {
    let on = protection_report(&EncryptionState::On, "alex", &[]);
    assert_eq!(on[0], "Disk encryption (FileVault): on.");
    assert!(on[1].contains("readable only by alex (folders 700, files 600)"));
    assert!(on[2].contains("same-user mode"));
    let off = protection_report(&EncryptionState::Off, "alex", &[]);
    assert!(off[0].starts_with("Disk encryption is off."));
    let unknown = protection_report(&EncryptionState::Unknown("no volume".into()), "alex", &[]);
    assert!(unknown[0].contains("could not be determined (no volume)"));
    let open = protection_report(&EncryptionState::On, "alex", &[("/r/x".into(), 0o644)]);
    assert!(
        open[1].contains("1 path(s)") && open[2] == "  644 /r/x",
        "{open:?}"
    );
}

#[test]
fn modes_are_read_back_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("r");
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(root.join("a"), b"x").unwrap();
    std::fs::set_permissions(root.join("a"), std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(mode_exceptions(&root).unwrap().is_empty());
    std::fs::set_permissions(root.join("a"), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        mode_exceptions(&root).unwrap(),
        vec![(root.join("a"), 0o644)]
    );
}

#[test]
fn the_development_pin_is_valid_and_stable() {
    let pin = release_pin().unwrap();
    pin.validate().unwrap();
    assert_eq!(pin, release_pin().unwrap());
}
