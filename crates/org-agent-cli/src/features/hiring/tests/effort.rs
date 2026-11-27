//! `TASK-181`: `--effort` on `agent execution-set` and `hr hire`, and `effort` in
//! `agent execution-get`, on the shared catalogue: `agent-os` runs it, and `agentic-org`
//! forwards its office commands to the same table, so both names are checked.

use serde_json::{json, Value};

use crate::runtime::transport::{OfficeTransport, TransportOffice};

const MEMBER: &str = "0193a5c0-0000-7000-8000-000000000051";
const PROFILE: &str = "0193a5c0-0000-7000-8000-000000000060";
const TEAM: &str = "0193a5c0-0000-7000-8000-000000000070";
const PROGRAMS: [&str; 2] = ["agent-os", "agentic-org"];

struct Recording {
    calls: Vec<(String, Value)>,
    reply: Value,
}

impl OfficeTransport for Recording {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, org_command::Refusal> {
        self.calls.push((method.to_owned(), params));
        Ok(self.reply.clone())
    }
}

fn run(program: &str, args: &[&str], reply: Value) -> (Value, u8, Vec<(String, Value)>) {
    let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
    let mut office = TransportOffice(Recording {
        calls: Vec::new(),
        reply,
    });
    let out = crate::execute_with_office(program, &args, &mut office);
    let envelope = serde_json::from_str(&out.stdout)
        .unwrap_or_else(|e| panic!("{program} {args:?}: not JSON ({e}): {}", out.stdout));
    (envelope, out.exit_code, office.0.calls)
}

fn execution_set(effort: &[&'static str]) -> Vec<&'static str> {
    let mut args = vec![
        "agent",
        "execution-set",
        "--member",
        MEMBER,
        "--model-profile-id",
        PROFILE,
        "--model-id",
        "sonnet",
        "--system-prompt-markdown",
        "You review code.",
        "--format",
        "json",
    ];
    args.extend_from_slice(effort);
    args
}

fn entity_reply() -> Value {
    json!({ "id": MEMBER, "revision": 2, "generation": 9 })
}

#[test]
fn execution_set_sends_every_effort_level_and_omits_it_when_not_given() {
    for program in PROGRAMS {
        for level in org_proto::hiring::AGENT_EFFORT_LEVELS {
            let (env, code, calls) = run(
                program,
                &execution_set(&["--effort", level]),
                entity_reply(),
            );
            assert_eq!(code, 0, "{program} --effort {level}: {env}");
            assert_eq!(env["ok"], true);
            let [(method, params)] = calls.as_slice() else {
                panic!("one call: {calls:?}");
            };
            assert_eq!(method, "member.agent_execution_set");
            assert_eq!(params["effort"], level, "{params}");
            assert_eq!(params["employee_id"], MEMBER);
        }
        let (_, code, calls) = run(program, &execution_set(&["--effort=xhigh"]), entity_reply());
        assert_eq!(code, 0);
        assert_eq!(calls[0].1["effort"], "xhigh");

        // A full replacement without --effort sends none: the runtime's default.
        let (_, code, calls) = run(program, &execution_set(&[]), entity_reply());
        assert_eq!(code, 0);
        assert!(calls[0].1.get("effort").is_none(), "{}", calls[0].1);
    }
}

#[test]
fn execution_set_refuses_an_unknown_effort_before_calling_the_office() {
    for program in PROGRAMS {
        for wrong in ["turbo", "", "none", "XHIGH", "High"] {
            let (env, code, calls) = run(
                program,
                &execution_set(&["--effort", wrong]),
                entity_reply(),
            );
            assert_eq!(code, 2, "{program} --effort {wrong:?}: {env}");
            assert_eq!(env["error"]["code"], "invalid_request", "{env}");
            assert!(calls.is_empty(), "{calls:?}");
        }
        let (env, code, calls) = run(
            program,
            &execution_set(&["--effort", "high", "--effort", "max"]),
            entity_reply(),
        );
        assert_eq!(code, 2, "{env}");
        assert!(calls.is_empty());
    }
}

#[test]
fn execution_get_prints_the_effort_or_leaves_it_out() {
    let record = |effort: Option<&str>| {
        let mut record = json!({
            "employee_id": MEMBER,
            "model_profile_id": PROFILE,
            "model_id": "sonnet",
            "tools_config": [],
            "permission_preset": "workspace_edits",
            "enabled": true,
            "system_prompt_markdown": "You review code.",
            "revision": 3
        });
        if let Some(effort) = effort {
            record["effort"] = json!(effort);
        }
        record
    };
    for program in PROGRAMS {
        let args = [
            "agent",
            "execution-get",
            "--member",
            MEMBER,
            "--format",
            "json",
        ];
        let (env, code, _) = run(program, &args, record(Some("xhigh")));
        assert_eq!(code, 0, "{env}");
        assert_eq!(env["data"]["effort"], "xhigh", "{env}");
        let (env, code, _) = run(program, &args, record(None));
        assert_eq!(code, 0, "{env}");
        assert!(env["data"].get("effort").is_none(), "{env}");
    }
}

#[test]
fn hr_hire_carries_the_effort_in_its_proposal() {
    let reply = json!({
        "request_id": "0193a5c0-0000-7000-8000-000000000080",
        "state": "hired",
        "employee_id": MEMBER,
        "revision": 1,
        "generation": 4
    });
    for program in PROGRAMS {
        let hire = |effort: &[&str]| {
            let mut args = vec![
                "hr",
                "hire",
                "--team",
                TEAM,
                "--name",
                "Atlas",
                "--handle",
                "atlas",
                "--reason",
                "Deep reviews",
                "--format",
                "json",
            ];
            args.extend_from_slice(effort);
            run(program, &args, reply.clone())
        };
        let (env, code, calls) = hire(&["--effort", "max"]);
        assert_eq!(code, 0, "{env}");
        assert_eq!(calls[0].0, "hr.hire");
        assert_eq!(calls[0].1["proposal"]["effort"], "max", "{}", calls[0].1);
        let (_, code, calls) = hire(&[]);
        assert_eq!(code, 0);
        assert!(calls[0].1["proposal"].get("effort").is_none());
        let (_, code, calls) = hire(&["--effort", "turbo"]);
        assert_eq!(code, 2);
        assert!(calls.is_empty());
    }
}
