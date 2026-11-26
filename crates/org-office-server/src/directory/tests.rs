use super::*;
use org_office::TestOffice;

#[test]
fn members_resolve_by_id_handle_and_owner_from_the_office_projection() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let snapshot = office.org_snapshot().unwrap();
    let directory = OfficeDirectory::of(Arc::clone(&office));
    let owner = snapshot.members.iter().find(|m| m.owner).unwrap();
    for address in [owner.id.as_str(), "owner", " OWNER "] {
        let (found, position) = directory.resolve_member(address).unwrap().unwrap();
        assert_eq!(found.id, owner.id, "{address}");
        assert_eq!(position, snapshot.position);
    }
    assert!(directory.resolve_member("nobody-here").unwrap().is_none());
}

#[test]
fn an_ambiguous_handle_or_an_unreadable_projection_fails_closed() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mut snapshot = office.org_snapshot().unwrap();
    let mut twin = snapshot.members.iter().find(|m| m.owner).unwrap().clone();
    twin.id = "01900000-0000-7000-8000-0000000000ff".into();
    snapshot.members.push(twin);
    let directory = OfficeDirectory::new(Box::new(move || Ok(snapshot.clone())));
    assert!(directory.resolve_member("owner").is_err());
    let failing = OfficeDirectory::new(Box::new(|| {
        Err(org_office::OfficeError::Storage("disk".into()))
    }));
    assert!(failing.resolve_member("owner").is_err());
}
