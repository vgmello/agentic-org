//! `TASK-181`: `effort` on the execution and hire wire types stays optional both ways, so a
//! caller or office from before it reads and writes the same JSON.

use serde_json::json;

use super::{AgentExecutionRecord, AgentExecutionSetParams, HireProposalWire, AGENT_EFFORT_LEVELS};

#[test]
fn the_effort_levels_are_the_five_lowest_first() {
    assert_eq!(
        AGENT_EFFORT_LEVELS,
        ["low", "medium", "high", "xhigh", "max"]
    );
}

#[test]
fn execution_set_params_carry_an_effort_only_when_given() {
    let old = json!({
        "command_id": "c1",
        "employee_id": "e1",
        "model_profile_id": "p1",
        "model_id": "sonnet",
        "system_prompt_markdown": "Work."
    });
    let params: AgentExecutionSetParams = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(params.effort, None);
    let written = serde_json::to_value(&params).unwrap();
    assert!(written.get("effort").is_none(), "{written}");

    let mut with = old.clone();
    with["effort"] = json!("xhigh");
    let params: AgentExecutionSetParams = serde_json::from_value(with).unwrap();
    assert_eq!(params.effort.as_deref(), Some("xhigh"));
    assert_eq!(serde_json::to_value(&params).unwrap()["effort"], "xhigh");

    let mut null = old.clone();
    null["effort"] = json!(null);
    let params: AgentExecutionSetParams = serde_json::from_value(null).unwrap();
    assert_eq!(params.effort, None);

    // Still no unknown fields: a misspelling is not a silent default.
    let mut typo = old;
    typo["efort"] = json!("high");
    assert!(serde_json::from_value::<AgentExecutionSetParams>(typo).is_err());
}

#[test]
fn an_execution_record_and_a_hire_proposal_read_with_or_without_an_effort() {
    let record = json!({
        "employee_id": "e1",
        "model_profile_id": "p1",
        "model_id": "sonnet",
        "tools_config": [],
        "permission_preset": "safe_ask",
        "enabled": true,
        "system_prompt_markdown": "Work.",
        "revision": 1
    });
    let read: AgentExecutionRecord = serde_json::from_value(record.clone()).unwrap();
    assert_eq!(read.effort, None);
    assert_eq!(serde_json::to_value(&read).unwrap(), record);
    let mut with = record;
    with["effort"] = json!("max");
    let read: AgentExecutionRecord = serde_json::from_value(with.clone()).unwrap();
    assert_eq!(read.effort.as_deref(), Some("max"));
    assert_eq!(serde_json::to_value(&read).unwrap(), with);

    let proposal: HireProposalWire =
        serde_json::from_value(json!({ "name": "Atlas", "handle": "atlas", "effort": "low" }))
            .unwrap();
    assert_eq!(proposal.effort.as_deref(), Some("low"));
    let bare = HireProposalWire {
        effort: None,
        ..proposal
    };
    assert!(serde_json::to_value(&bare).unwrap().get("effort").is_none());
}
