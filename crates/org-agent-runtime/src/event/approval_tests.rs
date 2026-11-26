use serde_json::json;

use crate::{ApprovalId, ApprovalKind, ApprovalRequest, Escalation, ToolCapability};

#[test]
fn effective_escalation_fails_closed() {
    let request = |kind, escalation| {
        ApprovalRequest::new(ApprovalId::new("1"), kind, escalation, "x", json!({}))
            .effective_escalation()
    };
    use ApprovalKind as K;
    use Escalation as E;
    assert_eq!(request(K::Permissions, E::None), E::PermissionChange);
    assert_eq!(request(K::Unknown, E::None), E::Unknown);
    assert_eq!(request(K::Command, E::None), E::None);
    assert_eq!(request(K::Command, E::SandboxBypass), E::SandboxBypass);
    assert_eq!(request(K::Permissions, E::SandboxBypass), E::SandboxBypass);
    assert_eq!(
        request(K::FileChange, E::PermissionChange),
        E::PermissionChange
    );
    assert_eq!(request(K::Other, E::Unknown), E::Unknown);
    let mut unknown_cap =
        ApprovalRequest::new(ApprovalId::new("2"), K::Command, E::None, "Bash", json!({}));
    unknown_cap.capability = Some(ToolCapability::Unknown);
    assert_eq!(unknown_cap.effective_escalation(), E::Unknown);
}

#[test]
fn escalation_is_required_and_round_trips() {
    for escalation in [
        Escalation::None,
        Escalation::SandboxBypass,
        Escalation::PermissionChange,
        Escalation::Unknown,
    ] {
        let request = ApprovalRequest::new(
            ApprovalId::new("1"),
            ApprovalKind::Command,
            escalation,
            "Bash",
            json!({"command": "ls"}),
        );
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(
            serde_json::from_value::<ApprovalRequest>(json).unwrap(),
            request
        );
    }
    assert_eq!(
        serde_json::to_value(Escalation::SandboxBypass).unwrap(),
        json!("sandbox_bypass")
    );
    // A request without the key doesn't decode: it can't silently mean "no escalation".
    let mut json = serde_json::to_value(ApprovalRequest::new(
        ApprovalId::new("1"),
        ApprovalKind::Command,
        Escalation::None,
        "Bash",
        json!({}),
    ))
    .unwrap();
    json.as_object_mut().unwrap().remove("escalation");
    assert!(serde_json::from_value::<ApprovalRequest>(json).is_err());
}
