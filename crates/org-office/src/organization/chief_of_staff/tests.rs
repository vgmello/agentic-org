use super::*;
use crate::organization::records::{self, By};
use org_store::Timestamp;

fn id(n: u64) -> Id {
    Id::parse(&format!("01900000-0000-7000-8000-{n:012x}")).unwrap()
}

#[test]
fn her_id_is_a_stable_uuid_v7_of_the_organization() {
    let org = id(0xa2);
    let first = chief_of_staff_id(&org, 0);
    assert!(records::is_stable_id(first.as_str()), "{first}");
    assert_eq!(
        first,
        chief_of_staff_id(&org, 0),
        "derivation is deterministic"
    );
    assert!(first.as_str().starts_with("01900000-0000-7"));
    assert_ne!(first, chief_of_staff_id(&org, 1));
    assert_ne!(first, chief_of_staff_id(&id(0xa3), 0));
    assert_ne!(first, ensure_operation_id(&org));
    assert!(records::is_stable_id(ensure_operation_id(&org).as_str()));
    for attempt in 0..=u8::MAX {
        let derived = chief_of_staff_id(&org, attempt);
        assert!(records::is_stable_id(derived.as_str()), "{derived}");
    }
}

#[test]
fn the_handle_falls_back_when_tess_is_taken() {
    let pick = |taken: &[&str]| free_handle::<()>(|h| Ok(taken.contains(&h))).unwrap();
    assert_eq!(pick(&[]), Some("tess"));
    assert_eq!(pick(&["tess"]), Some("chief-of-staff"));
    assert_eq!(pick(&["tess", "chief-of-staff"]), Some("tess-cos"));
    assert_eq!(pick(&CHIEF_OF_STAFF_HANDLES), None);
    for handle in CHIEF_OF_STAFF_HANDLES {
        assert!(records::is_handle(handle), "{handle}");
    }
}

#[test]
fn her_record_keeps_the_expansion_in_the_charter_alone() {
    let meta = Meta::created(&By::System, Timestamp(1_700_000_000_000));
    let tess = chief_of_staff_record(id(1), "tess", &id(2), meta);
    assert_eq!(tess.name, "Tess");
    assert_eq!(tess.title.as_deref(), Some("Chief of Staff"));
    assert_eq!(tess.title_in_team.as_deref(), Some("Chief of Staff"));
    assert_eq!(tess.builtin_key, Some(BuiltinKey::ChiefOfStaff));
    assert_eq!(tess.kind, EmployeeKind::Agent);
    assert!(tess.may_hire);
    assert_eq!(tess.reports_to_employee_id, None, "reports to the owner");
    assert_eq!(tess.home_team_id, id(2));
    assert!(tess.body.contains(CHIEF_OF_STAFF_CHARTER));
    for visible in [&tess.name, &tess.handle] {
        assert!(!visible.contains("Tactical"), "{visible}");
    }
    assert!(!tess.title.unwrap_or_default().contains("Tactical"));
    let bytes = records::encode_employee(&chief_of_staff_record(
        id(1),
        "tess",
        &id(2),
        Meta::created(&By::System, Timestamp(1)),
    ));
    assert!(records::decode_employee(&bytes).is_ok());
}
