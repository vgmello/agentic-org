use super::LinkedOrg;
use serde_json::json;

/// `name` is additive: an answer without it still reads, and an empty name is not
/// written, so a peer that does not know the field sees what it saw before.
#[test]
fn a_linked_organization_carries_its_name_and_reads_an_answer_without_one() {
    let named = json!({"org_id": "0193a5c0-0000-7000-8000-000000000001", "name": "Acme",
        "employee_id": "0193a5c0-0000-7000-8000-000000000006", "office_ids": []});
    let read: LinkedOrg = serde_json::from_value(named.clone()).unwrap();
    assert_eq!(read.name, "Acme");
    assert_eq!(serde_json::to_value(&read).unwrap(), named);
    let unnamed = json!({"org_id": "0193a5c0-0000-7000-8000-000000000001",
        "employee_id": "0193a5c0-0000-7000-8000-000000000006", "office_ids": []});
    let read: LinkedOrg = serde_json::from_value(unnamed.clone()).unwrap();
    assert_eq!(read.name, "");
    assert_eq!(serde_json::to_value(&read).unwrap(), unnamed);
}
