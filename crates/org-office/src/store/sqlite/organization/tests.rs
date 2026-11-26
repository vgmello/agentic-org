use crate::store::sqlite::SqliteEngine;

#[test]
fn a_projection_opened_for_no_organization_refuses_the_family() {
    use crate::organization::ports::OrganizationRead;
    use org_store::Engine;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap().join("office.db");
    let engine = SqliteEngine::open(&path).unwrap();
    assert!(engine.snapshot().unwrap().teams().is_err());
    assert!(engine.begin().unwrap().organization().is_err());
}
