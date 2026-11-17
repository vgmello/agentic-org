use super::*;
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
};
fn input(root: PathBuf) -> Input {
    Input {version:1,root,login:"Owner".into(),organization:PlanV1{version:1,name:"Example".into(),slug:"example".into(),repository_remote_url:"https://example.invalid/org.git".into(),state_branch:"org/state".into(),home_team_name:"Core".into()},listen:serde_json::from_str(r#"{"address":"127.0.0.1","port":8080,"scheme":"http_loopback","origin":"http://127.0.0.1:8080","hsts":"off"}"#).unwrap()}
}
fn pin() -> ReleasePin {
    ReleasePin {
        release_id: "fixture-1".into(),
        sequence: 1,
        manifest_sha256: "a".repeat(64),
    }
}
#[test]
fn original_operations_survive_reopen_and_cannot_be_replaced() {
    let temp = super::super::test_directory();
    let base = temp.path().canonicalize().unwrap();
    fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
    let original = input(base.join("instance"));
    let journal = Journal::open(&base, None).unwrap();
    let r = journal.freeze(original.clone(), true, pin()).unwrap();
    assert_ne!(r.bootstrap_operation, r.hq_operation);
    assert!(Journal::open(&base, None).is_err());
    assert_eq!(journal.freeze(original.clone(), true, pin()).unwrap(), r);
    assert!(journal.freeze(original.clone(), false, pin()).is_err());
    let mut changed = original.clone();
    changed.organization.home_team_name = "Different".into();
    assert!(journal.freeze(changed, true, pin()).is_err());
    let mut new_pin = pin();
    new_pin.sequence = 2;
    assert!(journal.freeze(original.clone(), true, new_pin).is_err());
    let mut advanced = r.clone();
    advanced.instance_id = Some(crate::ids::generate().unwrap());
    advanced.provisioned = Some(Provisioned {
        organization_id: crate::ids::uuid_v7().unwrap(),
        office_id: crate::ids::uuid_v7().unwrap(),
        home_team_id: crate::ids::uuid_v7().unwrap(),
        remote_readiness: org_host_control::provision::RemoteReadiness::Unverified,
    });
    advanced.exited = true;
    journal.write(&advanced).unwrap();
    assert!(journal.write(&r).is_err());
    drop(journal);
    let reopened = Journal::open(&base, None).unwrap();
    assert_eq!(reopened.read().unwrap().unwrap(), advanced);
    assert_eq!(reopened.freeze(original, true, pin()).unwrap(), advanced);
    assert!(Journal::open(&base, Some("../escape")).is_err());
}
#[test]
fn strict_input_and_replaced_setup_lock_refuse_without_secrets() {
    let temp = super::super::test_directory();
    let base = temp.path().canonicalize().unwrap();
    fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
    let original = input(base.join("instance"));
    let mut raw = serde_json::to_value(&original).unwrap();
    raw.as_object_mut()
        .unwrap()
        .insert("password".into(), "forbidden".into());
    assert!(serde_json::from_value::<Input>(raw).is_err());
    let mut bad = original.clone();
    bad.organization.repository_remote_url = "file:///tmp/org".into();
    assert!(bad.validate().is_err());
    bad = original.clone();
    bad.root = base.join("../escape");
    assert!(bad.validate().is_err());
    let journal = Journal::open(&base, None).unwrap();
    let r = journal.freeze(original, true, pin()).unwrap();
    let lock = base.join("setup/default/setup.lock");
    fs::rename(&lock, lock.with_extension("original")).unwrap();
    symlink("setup.v1.json", lock).unwrap();
    assert!(journal.read().is_err());
    assert!(journal.write(&r).is_err());
}
