//! The projects slice: what `project create|list|inspect` send and print.

use serde_json::{json, Value};

use crate::runtime::transport::{derive_uuid_v7, OfficeTransport, Refusal, TransportOffice};

const PROJECT: &str = "0193a5c0-0000-7000-8000-000000000030";

struct Stub(Vec<(String, Value)>);

fn record() -> Value {
    json!({
        "id": PROJECT, "number": 1, "key": "P-1", "slug": "billing", "name": "Billing",
        "owner_team_id": "0193a5c0-0000-7000-8000-000000000010",
        "contributing_team_ids": [], "archived": false, "revision": 1
    })
}

impl OfficeTransport for Stub {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.0.push((method.to_owned(), params));
        Ok(match method {
            "project.list" => json!({"projects": [record(), record()]}),
            "project.inspect" => record(),
            _ => json!({"status": "committed", "position": {"epoch": 1, "seq": 11}}),
        })
    }
}

fn run(stub: &mut Stub, line: &str) -> (Value, u8) {
    let args: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
    let out = crate::execute_with_office("agent-os", &args, &mut TransportOffice(&mut *stub));
    (serde_json::from_str(&out.stdout).unwrap(), out.exit_code)
}

#[test]
fn a_retried_create_makes_the_same_project_and_owning_team() {
    let mut stub = Stub(Vec::new());
    let (env, code) = run(
        &mut stub,
        "project create --name Billing --idempotency-key k-7",
    );
    assert_eq!(code, 0, "{env}");
    let id = derive_uuid_v7("project.create.id", "k-7");
    assert_eq!(env["data"]["id"], id.as_str());
    assert_eq!(env["data"]["status"], "committed");
    let (method, params) = &stub.0[0];
    assert_eq!(method, "project.create");
    assert_eq!(params["project_id"], id.as_str());
    assert_eq!(
        params["create_team_id"],
        derive_uuid_v7("project.create.owner_team", "k-7").as_str()
    );
    assert_eq!(
        params["command_id"],
        derive_uuid_v7("org-command", "k-7").as_str()
    );
}

#[test]
fn list_pages_and_inspect_reads_one_record() {
    let mut stub = Stub(Vec::new());
    let (env, code) = run(&mut stub, "project list --include-archived --limit 1");
    assert_eq!(code, 0, "{env}");
    assert_eq!(stub.0[0].1, json!({"include_archived": true}));
    assert_eq!(env["meta"]["next_cursor"], "1");
    assert_eq!(env["meta"]["total"], 2);
    let (env, code) = run(&mut stub, "project inspect --project P-1");
    assert_eq!(code, 0, "{env}");
    assert_eq!(env["data"]["key"], "P-1");
    let (env, code) = run(&mut stub, "project list --cursor nope");
    assert_eq!(code, 1, "{env}");
    assert_eq!(
        stub.0.len(),
        2,
        "a bad cursor is refused before the office is asked"
    );
}
