use super::*;
use serde_json::json;

const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1";
const ORG: &str = "01900000-0000-7000-8000-000000000001";
const OFFICE: &str = "01900000-0000-7000-8000-000000000002";
const INCARNATION: &str = "01900000-0000-7000-8000-000000000003";
const MEMBER: &str = "01900000-0000-7000-8000-0000000000ee";

fn identity() -> Identity {
    Identity {
        instance_id: INSTANCE.into(),
        organization_id: ORG.into(),
        office_id: OFFICE.into(),
        journal_incarnation: INCARNATION.into(),
        writer_epoch: 1,
        machine_id: "ab".repeat(32),
    }
}

fn admission() -> Admission {
    Admission {
        version: VERSION,
        identity: identity(),
        durable_root: format!("/instance/{ORG}/durable"),
        runtime_root: format!("/instance/{ORG}/runtime"),
        owner_account: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        owner_member: MEMBER.into(),
    }
}

fn head(ordinal: u64, fill: char, organization: u64) -> Head {
    Head {
        ordinal,
        sha256: fill.to_string().repeat(64),
        high_waters: HighWaters {
            organization,
            work: 4,
            message_content: 5,
            message_state: 6,
        },
    }
}

#[test]
fn an_identity_holds_only_store_ids_and_a_live_epoch() {
    assert!(identity().check());
    for change in [
        |i: &mut Identity| i.instance_id = "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        |i: &mut Identity| i.organization_id = INSTANCE.to_uppercase(),
        |i: &mut Identity| i.office_id = "office".into(),
        // A ULID, as the host minted it before version 2 (P1).
        |i: &mut Identity| i.journal_incarnation = "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        |i: &mut Identity| i.writer_epoch = 0,
        |i: &mut Identity| i.machine_id = "AB".repeat(32),
        |i: &mut Identity| i.machine_id = "ab".repeat(31),
    ] {
        let mut wrong = identity();
        change(&mut wrong);
        assert!(!wrong.check(), "{wrong:?}");
    }
}

#[test]
fn an_admission_carries_two_separate_plain_roots() {
    assert!(admission().check());
    for change in [
        |a: &mut Admission| a.version = 1,
        |a: &mut Admission| a.durable_root = "relative/durable".into(),
        |a: &mut Admission| a.runtime_root = format!("/instance/{ORG}/../runtime"),
        |a: &mut Admission| a.runtime_root = format!("/instance/{ORG}/./runtime"),
        |a: &mut Admission| a.runtime_root = format!("/instance//{ORG}/runtime"),
        |a: &mut Admission| a.runtime_root = format!("/instance/{ORG}/runtime/"),
        |a: &mut Admission| a.durable_root = "/".into(),
        |a: &mut Admission| a.runtime_root = "/instance/\0/runtime".into(),
        |a: &mut Admission| a.runtime_root = a.durable_root.clone(),
        |a: &mut Admission| a.runtime_root = format!("{}/runtime", a.durable_root),
        |a: &mut Admission| a.durable_root = format!("{}/durable", a.runtime_root),
        |a: &mut Admission| a.owner_account = String::new(),
        |a: &mut Admission| a.owner_account = "test-account".into(),
        |a: &mut Admission| a.owner_account = "01900000-0000-4000-8000-0000000000ee".into(),
        |a: &mut Admission| a.owner_member = "not a member".into(),
        |a: &mut Admission| a.identity.writer_epoch = 0,
    ] {
        let mut wrong = admission();
        change(&mut wrong);
        assert!(!wrong.check(), "{wrong:?}");
    }
}

/// Version 2's frames: per-stream high-waters, the two data roots, no public or
/// protected root and no single origin high-water. Unknown fields are refused.
#[test]
fn version_two_frames_round_trip_and_refuse_version_one_fields() {
    let mut bytes = Vec::new();
    write(&mut bytes, &admission()).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["version"], 2);
    assert_eq!(value["identity"]["writer_epoch"], 1);
    assert!(value.get("public_root").is_none() && value.get("protected_root").is_none());
    assert_eq!(
        read::<Admission>(&mut bytes.as_slice()).unwrap(),
        Some(admission())
    );

    let confirm = Request::Confirm {
        identity: identity(),
        before: Some(head(1, 'a', 3)),
        after: head(4, 'b', 7),
    };
    let mut bytes = Vec::new();
    write(&mut bytes, &confirm).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        value["after"]["high_waters"],
        json!({"organization": 7, "work": 4, "message_content": 5, "message_state": 6})
    );
    assert!(matches!(
        read::<Request>(&mut bytes.as_slice()).unwrap(),
        Some(Request::Confirm { before: Some(b), after, .. }) if b == head(1, 'a', 3) && after == head(4, 'b', 7)
    ));

    let mut old = serde_json::to_value(admission()).unwrap();
    old["public_root"] = json!("/instance/state");
    let mut line = serde_json::to_vec(&old).unwrap();
    line.push(b'\n');
    assert!(read::<Admission>(&mut line.as_slice()).is_err());

    let mut old = serde_json::to_value(head(0, 'a', 0)).unwrap();
    old["origin_high_water"] = json!(73);
    let mut line = serde_json::to_vec(&json!({
        "op": "confirm", "identity": identity(), "before": null, "after": old
    }))
    .unwrap();
    line.push(b'\n');
    assert!(read::<Request>(&mut line.as_slice()).is_err());

    let mut negative = serde_json::to_value(identity()).unwrap();
    negative["writer_epoch"] = json!(-1);
    assert!(serde_json::from_value::<Identity>(negative).is_err());
}

#[test]
fn the_first_confirmation_is_the_anchor() {
    assert!(confirms(None, None, &head(0, 'a', 0)));
    assert!(confirms(None, None, &head(0, 'a', 9)));
    assert!(!confirms(None, None, &head(1, 'a', 0)));
    // A `before` the host never stored.
    assert!(!confirms(None, Some(&head(0, 'a', 0)), &head(1, 'b', 0)));
}

#[test]
fn a_confirmation_may_advance_many_ordinals_at_once() {
    let stored = head(2, 'a', 3);
    assert!(confirms(Some(&stored), Some(&stored), &head(3, 'b', 3)));
    assert!(confirms(Some(&stored), Some(&stored), &head(9, 'c', 40)));
}

#[test]
fn an_equal_confirmation_is_the_same_head() {
    let stored = head(2, 'a', 3);
    assert!(confirms(Some(&stored), Some(&stored), &stored.clone()));
    // The same ordinal with another hash, or other high-waters, is not the same head.
    assert!(!confirms(Some(&stored), Some(&stored), &head(2, 'b', 3)));
    assert!(!confirms(Some(&stored), Some(&stored), &head(2, 'a', 4)));
    // Going back is never confirmed.
    assert!(!confirms(Some(&stored), Some(&stored), &head(1, 'a', 3)));
}

#[test]
fn no_high_water_may_go_down() {
    let stored = head(2, 'a', 3);
    assert!(!confirms(Some(&stored), Some(&stored), &head(5, 'b', 2)));
    for lower in [
        |h: &mut HighWaters| h.work -= 1,
        |h: &mut HighWaters| h.message_content -= 1,
        |h: &mut HighWaters| h.message_state -= 1,
    ] {
        let mut after = head(5, 'b', 3);
        lower(&mut after.high_waters);
        assert!(!confirms(Some(&stored), Some(&stored), &after));
    }
}

#[test]
fn before_must_be_the_stored_head() {
    let stored = head(2, 'a', 3);
    assert!(!confirms(Some(&stored), None, &head(3, 'b', 3)));
    assert!(!confirms(
        Some(&stored),
        Some(&head(2, 'c', 3)),
        &head(3, 'b', 3)
    ));
    assert!(!confirms(
        Some(&stored),
        Some(&head(1, 'a', 3)),
        &head(3, 'b', 3)
    ));
}

#[test]
fn a_head_hash_is_lowercase_hex() {
    let mut after = head(3, 'b', 3);
    after.sha256 = "B".repeat(64);
    assert!(!confirms(
        Some(&head(2, 'a', 3)),
        Some(&head(2, 'a', 3)),
        &after
    ));
    after.sha256 = "b".repeat(63);
    assert!(!after.check());
}
