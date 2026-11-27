//! Live turn-control wire contracts (`TASK-103`): coarse activity, interrupt and steer.

use serde_json::json;

use super::*;

fn idle_view_json() -> serde_json::Value {
    json!({
        "employee_id": "emp-1",
        "office_id": "office-1",
        "state": "idle",
        "private_home": "/home/a",
        "same_user_isolation_notice": "n",
        "permission_preset": "workspace_edits",
        "sandbox_enabled": true,
        "consecutive_stalls": 0,
        "stall_locked": false,
        "recovery_required": false
    })
}

#[test]
fn a_view_without_activity_decodes_idle_and_omits_it_on_the_wire() {
    let view: SessionView = serde_json::from_value(idle_view_json()).unwrap();
    assert_eq!(view.activity_phase, None);
    assert_eq!(view.active_tool_name, None);
    assert_eq!(view.active_tool_started_at, None);
    assert!(!view.supports_steer);
    let wire = serde_json::to_value(&view).unwrap();
    assert!(wire.get("activity_phase").is_none());
    assert!(wire.get("active_tool_name").is_none());
    assert_eq!(wire["supports_steer"], json!(false));
}

#[test]
fn activity_phases_are_snake_case_and_unknown_values_decode_as_unknown() {
    let mut raw = idle_view_json();
    raw["state"] = json!("running");
    raw["activity_phase"] = json!("running_tool");
    raw["active_tool_name"] = json!("Bash");
    raw["active_tool_started_at"] = json!("2026-11-27T10:00:00Z");
    raw["supports_steer"] = json!(true);
    let view: SessionView = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(view.activity_phase, Some(ActivityPhase::RunningTool));
    let wire = serde_json::to_value(&view).unwrap();
    for key in [
        "activity_phase",
        "active_tool_name",
        "active_tool_started_at",
        "supports_steer",
    ] {
        assert_eq!(wire[key], raw[key], "{key}");
    }
    for (phase, wire) in [
        (ActivityPhase::Thinking, "thinking"),
        (ActivityPhase::Responding, "responding"),
        (ActivityPhase::Interrupting, "interrupting"),
    ] {
        assert_eq!(serde_json::to_value(phase).unwrap(), json!(wire));
    }
    let future: ActivityPhase = serde_json::from_value(json!("compacting")).unwrap();
    assert_eq!(future, ActivityPhase::Unknown);
}

#[test]
fn an_activity_frame_says_idle_with_null_and_drops_tool_detail_on_request() {
    let frame = MemberActivityChangedData {
        member_id: "emp-1".into(),
        phase: Some(ActivityPhase::RunningTool),
        active_tool_name: Some("Bash".into()),
        active_tool_started_at: Some("2026-11-27T10:00:00Z".into()),
        supports_steer: true,
        changed_at: "2026-11-27T10:00:00Z".into(),
    };
    let private = frame.clone().without_tool_detail();
    assert_eq!(private.phase, Some(ActivityPhase::RunningTool));
    assert_eq!(private.active_tool_name, None);
    assert_eq!(private.active_tool_started_at, None);
    let wire = serde_json::to_value(&private).unwrap();
    assert!(wire.get("active_tool_name").is_none());
    let idle = MemberActivityChangedData {
        phase: None,
        ..frame
    };
    assert_eq!(serde_json::to_value(&idle).unwrap()["phase"], json!(null));
}

#[test]
fn interrupt_and_steer_are_member_calls_with_closed_params() {
    assert_eq!(MemberSessionInterrupt::METHOD, "member.session_interrupt");
    assert_eq!(MemberSessionSteer::METHOD, "member.session_steer");
    let interrupt: MemberSessionInterruptParams =
        serde_json::from_value(json!({"command_id": "c", "employee_id": "e"})).unwrap();
    assert_eq!(interrupt.employee_id, "e");
    assert!(serde_json::from_value::<MemberSessionInterruptParams>(
        json!({"command_id": "c", "employee_id": "e", "reason": "x"})
    )
    .is_err());
    let steer: MemberSessionSteerParams =
        serde_json::from_value(json!({"command_id": "c", "employee_id": "e", "text": "go left"}))
            .unwrap();
    assert_eq!(steer.text, "go left");
    assert!(serde_json::from_value::<MemberSessionSteerParams>(
        json!({"command_id": "c", "employee_id": "e"})
    )
    .is_err());
    assert!(serde_json::from_value::<MemberSessionSteerParams>(
        json!({"command_id": "c", "employee_id": "e", "text": "t", "partition": "p"})
    )
    .is_err());
}
