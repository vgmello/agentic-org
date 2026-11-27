//! The organization slice over a recording transport: what each command sends and prints.

use serde_json::{json, Value};

use crate::runtime::transport::{OfficeTransport, Refusal, TransportOffice};
use org_proto::ErrorCode;

const CORE: &str = "0193a5c0-0000-7000-8000-000000000001";
const ADA: &str = "0193a5c0-0000-7000-8000-000000000020";

/// Answers each method with a canned reply and records every call.
struct Stub {
    calls: Vec<(String, Value)>,
    conflict_on: Option<&'static str>,
}

impl Stub {
    fn new() -> Self {
        Self {
            calls: Vec::new(),
            conflict_on: None,
        }
    }
}

fn card(name: &str, kind: &str) -> Value {
    json!({
        "id": ADA, "name": name, "handle": "ada", "kind": kind, "owner": false,
        "presence": "offline", "teams": [{"id": CORE, "name": "Core"}]
    })
}

impl OfficeTransport for Stub {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.calls.push((method.to_owned(), params));
        if self.conflict_on == Some(method) {
            return Err(Refusal::new(ErrorCode::Conflict, "stale", "retry"));
        }
        Ok(match method {
            "org.snapshot" => json!({
                "position": {"epoch": 1, "seq": 2},
                "teams": [{"id": CORE, "name": "Core", "version": 7}],
                "members": [{"id": ADA, "kind": "person", "name": "Ada", "owner": false, "version": 3}]
            }),
            "who" => json!({"members": [card("Ada", "person")]}),
            "team.set_max_members" | "member.set_may_hire" => {
                json!({"id": ADA, "revision": 4, "generation": 9})
            }
            _ => json!({"status": "committed", "position": {"epoch": 1, "seq": 3}}),
        })
    }
}

fn run(stub: &mut Stub, line: &str) -> (Value, u8) {
    let args: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
    let out = crate::execute_with_office("agent-os", &args, &mut TransportOffice(&mut *stub));
    let envelope = serde_json::from_str(&out.stdout).unwrap_or(Value::Null);
    (envelope, out.exit_code)
}

#[test]
fn who_is_directory_list_and_prints_the_declared_card_fields_only() {
    let mut stub = Stub::new();
    let (env, code) = run(&mut stub, "who --team Core");
    assert_eq!(code, 0, "{env}");
    assert_eq!(
        stub.calls,
        vec![("who".to_owned(), json!({"team": "Core"}))]
    );
    let first = &env["data"][0];
    assert_eq!(first["name"], "Ada");
    assert!(first.get("teams").is_none(), "{env}");
    assert_eq!(env["meta"]["total"], 1);
}

#[test]
fn team_inspect_finds_the_team_by_name_and_lists_its_member_cards() {
    let mut stub = Stub::new();
    let (env, code) = run(&mut stub, "team inspect --team core");
    assert_eq!(code, 0, "{env}");
    assert_eq!(env["data"]["id"], CORE);
    assert_eq!(env["data"]["members"][0]["teams"][0]["name"], "Core");
    assert_eq!(stub.calls[1], ("who".to_owned(), json!({"team": CORE})));
}

#[test]
fn person_and_agent_inspect_keep_to_their_kind() {
    let mut stub = Stub::new();
    let (env, code) = run(&mut stub, "person inspect --member ada");
    assert_eq!(code, 0, "{env}");
    assert_eq!(env["data"]["kind"], "person");
    let (env, code) = run(&mut stub, "agent inspect --member ada");
    assert_eq!(code, 1, "{env}");
    assert_eq!(env["error"]["code"], "not_found");
}

#[test]
fn a_person_update_mixing_field_groups_sends_nothing() {
    let mut stub = Stub::new();
    let line = format!("person update --member {ADA} --name Ada --title-in-team Lead --if-match 3");
    let (env, code) = run(&mut stub, &line);
    assert_eq!(code, 1, "{env}");
    assert_eq!(env["error"]["code"], "invalid_request");
    assert!(stub.calls.is_empty(), "{:?}", stub.calls);
}

#[test]
fn a_conflicting_team_rename_answers_the_current_revision_to_retry_with() {
    let mut stub = Stub::new();
    stub.conflict_on = Some("team.rename");
    let line = format!("team update --team {CORE} --name Platform --if-match 6");
    let (env, code) = run(&mut stub, &line);
    assert_eq!(code, 1, "{env}");
    assert_eq!(env["error"]["code"], "conflict");
    assert_eq!(env["error"]["retry_with"]["if-match"], 7, "{env}");
    let (method, params) = &stub.calls[0];
    assert_eq!(method, "team.rename");
    assert_eq!(params["expected_version"], 6);
    assert!(params["command_id"].is_string(), "{params}");
}

#[test]
fn team_limit_and_hiring_rights_are_revisioned_hiring_writes() {
    let mut stub = Stub::new();
    let line =
        format!("team set-limit --team {CORE} --max-members 12 --if-match 2 --idempotency-key k-1");
    let (env, code) = run(&mut stub, &line);
    assert_eq!(code, 0, "{env}");
    assert_eq!(env["meta"]["revision"], 4);
    let (method, params) = &stub.calls[0];
    assert_eq!(method, "team.set_max_members");
    assert_eq!(params["max_members"], 12);
    assert_eq!(
        params["command_id"],
        crate::runtime::transport::derive_uuid_v7("hiring-command", "k-1")
    );

    let line = format!("member set-hiring --member {ADA} --allow --revoke --if-match 1");
    let (env, code) = run(&mut stub, &line);
    assert_eq!(code, 1, "{env}");
    assert_eq!(stub.calls.len(), 1, "nothing more was sent");
    let line = format!("member set-hiring --member {ADA} --revoke --if-match 1");
    let (env, code) = run(&mut stub, &line);
    assert_eq!(code, 0, "{env}");
    assert_eq!(stub.calls[1].1["may_hire"], false);
}

#[test]
fn a_dry_run_team_create_is_sent_as_one_without_a_command_id() {
    let mut stub = Stub::new();
    let (env, code) = run(&mut stub, "team create --name Platform --dry-run");
    assert_eq!(code, 0, "{env}");
    let (method, params) = &stub.calls[0];
    assert_eq!(method, "team.create");
    assert_eq!(params["dry_run"], true);
    assert!(params.get("command_id").is_none(), "{params}");
}
