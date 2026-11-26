//! `session boot`: offline from this build alone and small, bound with identity and whole
//! must-follow documents within 8 KiB, and never offline when a binding fails.

use org_proto::processes::{
    DocumentKindWire, DocumentStatusWire, EffectiveProcessEntryWire, ProcessDocumentWire,
    ProcessEffectiveResult,
};
use serde_json::{json, Value};

use super::{CORE_RULES, MANDATORY_LIMIT, OFFLINE_LIMIT, SKILL_INDEX};
use crate::transport::{OfficeTransport, Refusal, TransportOffice};
use org_proto::ErrorCode;

/// A transport that answers `who` and `process.effective`, or refuses every call.
struct Stub {
    bound: bool,
    calls: Vec<(String, Value)>,
    documents: Vec<(Option<&'static str>, usize, bool)>,
    refuse: bool,
}

impl Stub {
    fn new(bound: bool) -> Self {
        Self {
            bound,
            calls: Vec::new(),
            documents: Vec::new(),
            refuse: false,
        }
    }
}

fn document(
    key: Option<&str>,
    body_bytes: usize,
    mandatory: bool,
    n: usize,
) -> EffectiveProcessEntryWire {
    let id = format!("0193a5c0-0000-7000-8000-{n:012}");
    EffectiveProcessEntryWire {
        id: id.clone(),
        key: key.map(str::to_owned),
        kind: DocumentKindWire::Standard,
        applies: true,
        mandatory,
        winner_kind: Some("defined".into()),
        document: ProcessDocumentWire {
            id,
            key: key.map(str::to_owned),
            revision: 2,
            kind: DocumentKindWire::Standard,
            title: format!("Doc {n}"),
            status: DocumentStatusWire::Current,
            owner_employee_id: None,
            owner_name: None,
            level: "organization".into(),
            team_id: None,
            project_id: None,
            scope_name: None,
            overridable: !mandatory,
            summary: "Summary.".into(),
            body_markdown: "x".repeat(body_bytes),
            path: "processes/doc.md".into(),
            created_at: None,
            created_by_kind: "person".into(),
            created_by_id: None,
            updated_at: None,
            updated_by_kind: "person".into(),
            updated_by_id: None,
        },
        replaces: None,
        problem: None,
        chain: Vec::new(),
    }
}

impl OfficeTransport for Stub {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.calls.push((method.to_owned(), params));
        if self.refuse {
            return Err(Refusal::new(
                ErrorCode::NotAuthorized,
                "The session token was not accepted.",
                "Check your session binding.",
            ));
        }
        match method {
            "who" => Ok(json!({"members": [{
                "id": "0193a5c0-0000-7000-8000-000000000003",
                "name": "Ada",
                "handle": "ada",
                "kind": "agent",
                "role_title": "Engineer",
                "teams": [{"id": "0193a5c0-0000-7000-8000-000000000001", "name": "Core"}],
                "presence": "busy"
            }]})),
            "process.effective" => {
                let entries = self
                    .documents
                    .iter()
                    .enumerate()
                    .map(|(n, (key, bytes, mandatory))| document(*key, *bytes, *mandatory, n))
                    .collect();
                Ok(serde_json::to_value(ProcessEffectiveResult {
                    scope_chain: vec!["Organization".into()],
                    entries,
                    problems: Vec::new(),
                    computed_at: "2026-11-26T00:00:00Z".into(),
                    stale_since: None,
                })
                .expect("encodes"))
            }
            other => panic!("boot called `{other}`"),
        }
    }

    fn is_bound(&self) -> bool {
        self.bound
    }

    fn caller_member_id(&self) -> Option<String> {
        Some("0193a5c0-0000-7000-8000-000000000003".into())
    }
}

fn boot(stub: Stub, line: &[&str]) -> (crate::Output, Stub) {
    let args: Vec<String> = line.iter().map(|word| (*word).to_owned()).collect();
    let mut office = TransportOffice(stub);
    let output = crate::execute_with_office("agent-os", &args, &mut office);
    (output, office.0)
}

fn envelope(output: &crate::Output) -> Value {
    serde_json::from_str(&output.stdout).unwrap_or_else(|e| panic!("{e}: {}", output.stdout))
}

#[test]
fn an_unbound_boot_is_offline_asks_no_office_and_stays_small_in_every_format() {
    for line in [
        &["boot"][..],
        &["session", "boot"],
        &["instructions", "overview"],
        &["boot", "--offline"],
    ] {
        let (output, stub) = boot(Stub::new(false), line);
        assert_eq!(output.exit_code, 0, "{line:?}: {}", output.stdout);
        assert!(stub.calls.is_empty(), "{line:?} asked the office");
        assert!(
            output.stdout.len() <= OFFLINE_LIMIT,
            "{} bytes",
            output.stdout.len()
        );
        let data = &envelope(&output)["data"];
        assert_eq!(data["mode"], "offline");
        assert_eq!(data["rules"], CORE_RULES.trim_end());
        assert_eq!(
            data["catalogue"].as_str().map(str::to_owned),
            Some(
                crate::table::runtime_table()
                    .catalogue()
                    .trim_end()
                    .to_owned()
            )
        );
        assert_eq!(
            data["skills"].as_array().map(Vec::len),
            Some(SKILL_INDEX.len())
        );
        assert!(data.get("identity").is_none() && data.get("mandatory").is_none());
        assert!(data["next_action"]
            .as_str()
            .unwrap()
            .contains("login --as @<handle>"));
    }
    for format in ["yaml", "compact"] {
        let (output, _) = boot(Stub::new(false), &["boot", "--format", format]);
        assert_eq!(output.exit_code, 0);
        assert!(
            output.stdout.len() <= OFFLINE_LIMIT,
            "{format}: {} bytes",
            output.stdout.len()
        );
    }
}

#[test]
fn an_offline_boot_in_a_bound_shell_asks_nothing_and_points_to_the_bound_boot() {
    let (output, stub) = boot(Stub::new(true), &["boot", "--offline"]);
    assert_eq!(output.exit_code, 0, "{}", output.stdout);
    assert!(stub.calls.is_empty());
    let data = &envelope(&output)["data"];
    assert_eq!(data["mode"], "offline");
    assert!(data["next_action"]
        .as_str()
        .unwrap()
        .contains("without --offline"));
}

#[test]
fn project_needs_the_office_and_is_never_dropped() {
    for (bound, line) in [
        (true, &["boot", "--offline", "--project", "P-1"][..]),
        (false, &["boot", "--project", "P-1"]),
    ] {
        let (output, stub) = boot(Stub::new(bound), line);
        assert_eq!(output.exit_code, 1, "{line:?}: {}", output.stdout);
        assert_eq!(envelope(&output)["error"]["code"], "invalid_request");
        assert!(stub.calls.is_empty());
    }
}

#[test]
fn a_bound_boot_answers_identity_and_whole_must_follow_documents() {
    let mut stub = Stub::new(true);
    stub.documents = vec![(Some("sec"), 100, true), (Some("style"), 50, false)];
    let (output, stub) = boot(stub, &["boot", "--project", "P-1"]);
    assert_eq!(output.exit_code, 0, "{}", output.stdout);
    let data = &envelope(&output)["data"];
    assert_eq!(data["mode"], "bound");
    assert_eq!(data["identity"]["handle"], "ada");
    assert_eq!(data["identity"]["teams"][0]["name"], "Core");
    let mandatory = data["mandatory"].as_array().unwrap();
    assert_eq!(mandatory.len(), 1, "only must-follow documents");
    assert_eq!(mandatory[0]["key"], "sec");
    assert_eq!(mandatory[0]["body"].as_str().unwrap().len(), 100);
    assert_eq!(data["mandatory_truncated"], false);
    assert!(data.get("mandatory_omitted").is_none());
    let effective = &stub
        .calls
        .iter()
        .find(|(m, _)| m == "process.effective")
        .unwrap()
        .1;
    assert_eq!(effective["for"]["project_id"], "P-1", "{effective}");
}

#[test]
fn must_follow_documents_over_8_kib_are_left_out_whole_and_named() {
    let mut stub = Stub::new(true);
    stub.documents = vec![
        (Some("first"), 5000, true),
        (Some("second"), 4000, true),
        (None, 10, true),
    ];
    let (output, _) = boot(stub, &["boot"]);
    assert_eq!(output.exit_code, 0, "{}", output.stdout);
    let data = &envelope(&output)["data"];
    let mandatory = data["mandatory"].as_array().unwrap();
    assert_eq!(mandatory.len(), 1);
    assert_eq!(
        mandatory[0]["body"].as_str().unwrap().len(),
        5000,
        "never cut"
    );
    assert_eq!(data["mandatory_truncated"], true);
    let omitted: Vec<&str> = data["mandatory_omitted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(omitted, ["second", "0193a5c0-0000-7000-8000-000000000002"]);
    assert!(data["next_action"]
        .as_str()
        .unwrap()
        .contains("agent-os process inspect --key second"));
    let kept: usize = mandatory
        .iter()
        .map(|doc| doc["body"].as_str().unwrap().len() + doc["title"].as_str().unwrap().len())
        .sum();
    assert!(kept <= MANDATORY_LIMIT);
}

#[test]
fn a_bound_boot_whose_office_refuses_fails_closed_and_never_goes_offline() {
    let mut stub = Stub::new(true);
    stub.refuse = true;
    let (output, _) = boot(stub, &["boot"]);
    assert_eq!(output.exit_code, 1, "{}", output.stdout);
    let env = envelope(&output);
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "not_authorized");
}

#[test]
fn a_binding_requested_by_the_environment_fails_closed_without_a_token() {
    use crate::office::{Binding, BoundOffice};
    let mut office = BoundOffice::new(Binding::default()).with_binding_requested(true);
    let args = vec!["boot".to_owned()];
    let output = crate::execute_with_office("agent-os", &args, &mut office);
    assert_eq!(output.exit_code, 1, "{}", output.stdout);
    assert_eq!(envelope(&output)["error"]["code"], "not_authorized");
    // Without the request, the same office answers offline.
    let mut office = BoundOffice::new(Binding::default());
    let output = crate::execute_with_office("agent-os", &args, &mut office);
    assert_eq!(output.exit_code, 0, "{}", output.stdout);
    assert_eq!(envelope(&output)["data"]["mode"], "offline");
}
