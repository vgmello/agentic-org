//! `session context`: offline from this build alone and small, bound (`managed` or `external`)
//! with mode-truthful standing rules, identity and whole must-follow documents within 8 KiB,
//! never offline when a binding fails, and zero legacy `boot` / `instructions overview` shims.

use org_proto::external_auth::SessionMode;
use org_proto::processes::{
    DocumentKindWire, DocumentStatusWire, EffectiveProcessEntryWire, ProcessDocumentWire,
    ProcessEffectiveResult,
};
use org_proto::ErrorCode;
use serde_json::{json, Value};

use super::{CORE_RULES, EXTERNAL_RULES, MANDATORY_LIMIT, SKILL_INDEX};
use crate::runtime::transport::{OfficeTransport, Refusal, TransportOffice};

/// The most bytes an offline context envelope may take, in any format.
const OFFLINE_LIMIT: usize = 6 * 1024;

/// A transport that answers `auth.whoami`, `who` and `process.effective`, or refuses every call.
struct Stub {
    bound: bool,
    calls: Vec<(String, Value)>,
    documents: Vec<(Option<&'static str>, usize, bool)>,
    refuse: bool,
    /// The employee the binding names; `None` for a grant-bound person's channel.
    caller: Option<&'static str>,
    /// Local session mode hint (`Some(External)` for a workspace binding).
    local_mode: Option<SessionMode>,
    /// Mode `auth.whoami` answers with when called.
    whoami_mode: SessionMode,
    /// How `auth.whoami` is refused; `None` answers it.
    whoami_refused: Option<ErrorCode>,
}

const AGENT: &str = "0193a5c0-0000-7000-8000-000000000003";
const PERSON: &str = "0193a5c0-0000-7000-8000-000000000009";

impl Stub {
    fn new(bound: bool) -> Self {
        Self {
            bound,
            calls: Vec::new(),
            documents: Vec::new(),
            refuse: false,
            caller: Some(AGENT),
            local_mode: None,
            whoami_mode: SessionMode::Managed,
            whoami_refused: None,
        }
    }

    fn methods(&self) -> Vec<&str> {
        self.calls
            .iter()
            .map(|(method, _)| method.as_str())
            .collect()
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
        self.calls.push((method.to_owned(), params.clone()));
        if self.refuse {
            return Err(Refusal::new(
                ErrorCode::NotAuthorized,
                "The session token was not accepted.",
                "Check your session binding.",
            ));
        }
        match method {
            "auth.whoami" => match self.whoami_refused {
                Some(code) => Err(Refusal::new(code, "no auth.whoami here", "Retry.")),
                None => {
                    let mode = match self.whoami_mode {
                        SessionMode::External => "external",
                        _ => "managed",
                    };
                    Ok(json!({"member_id": PERSON, "office_id": "o", "mode": mode}))
                }
            },
            "member.session_inspect" => Err(Refusal::new(
                ErrorCode::NotFound,
                "target employee is not an agent",
                "Check the member.",
            )),
            "who" => Ok(json!({"members": [{
                "id": params["member"],
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
            other => panic!("context called `{other}`"),
        }
    }

    fn is_bound(&self) -> bool {
        self.bound
    }

    fn caller_member_id(&self) -> Option<String> {
        self.caller.map(str::to_owned)
    }

    fn session_mode(&self) -> Option<SessionMode> {
        self.local_mode
    }
}

fn context(stub: Stub, line: &[&str]) -> (crate::Output, Stub) {
    let args: Vec<String> = line.iter().map(|word| (*word).to_owned()).collect();
    let mut office = TransportOffice(stub);
    let output = crate::execute_with_office("agent-os", &args, &mut office);
    (output, office.0)
}

fn envelope(output: &crate::Output) -> Value {
    serde_json::from_str(&output.stdout).unwrap_or_else(|e| panic!("{e}: {}", output.stdout))
}

#[test]
fn an_unbound_context_is_offline_asks_no_office_and_stays_small_in_every_format() {
    for line in [
        &["context"][..],
        &["session", "context"],
        &["context", "--offline"],
    ] {
        let (output, stub) = context(Stub::new(false), line);
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
                crate::registry::runtime_table()
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
        let next = data["next_action"].as_str().unwrap();
        assert!(next.contains("login --as @<handle>"), "{next}");
        assert!(
            next.contains("returns your context automatically"),
            "{next}"
        );
    }
    for format in ["yaml", "compact"] {
        let (output, _) = context(Stub::new(false), &["context", "--format", format]);
        assert_eq!(output.exit_code, 0);
        assert!(
            output.stdout.len() <= OFFLINE_LIMIT,
            "{format}: {} bytes",
            output.stdout.len()
        );
    }
}

#[test]
fn legacy_boot_and_instructions_overview_shorthands_are_completely_removed() {
    for legacy in [
        &["boot"][..],
        &["session", "boot"],
        &["instructions", "overview"],
        &["boot", "--offline"],
    ] {
        let (output, stub) = context(Stub::new(false), legacy);
        assert_eq!(
            output.exit_code, 2,
            "legacy {legacy:?} must fail with usage error: {}",
            output.stdout
        );
        assert!(stub.calls.is_empty());
        assert_eq!(envelope(&output)["ok"], false);
    }
}

#[test]
fn an_offline_context_in_a_bound_shell_asks_nothing_and_points_to_the_bound_context() {
    let (output, stub) = context(Stub::new(true), &["context", "--offline"]);
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
        (true, &["context", "--offline", "--project", "P-1"][..]),
        (false, &["context", "--project", "P-1"]),
    ] {
        let (output, stub) = context(Stub::new(bound), line);
        assert_eq!(output.exit_code, 1, "{line:?}: {}", output.stdout);
        assert_eq!(envelope(&output)["error"]["code"], "invalid_request");
        assert!(stub.calls.is_empty());
    }
}

#[test]
fn a_bound_managed_context_answers_managed_mode_core_rules_and_whole_must_follow_documents() {
    let mut stub = Stub::new(true);
    stub.documents = vec![(Some("sec"), 100, true), (Some("style"), 50, false)];
    let (output, stub) = context(stub, &["context", "--project", "P-1"]);
    assert_eq!(output.exit_code, 0, "{}", output.stdout);
    let data = &envelope(&output)["data"];
    assert_eq!(data["mode"], "managed");
    assert_eq!(data["rules"], CORE_RULES.trim_end());
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
fn a_bound_external_context_answers_external_mode_and_external_rules_without_managed_wake_copy() {
    // 1. Local workspace binding knows external mode directly (skips extra auth.whoami call).
    let mut stub = Stub::new(true);
    stub.local_mode = Some(SessionMode::External);
    let (output, stub) = context(stub, &["context"]);
    assert_eq!(output.exit_code, 0, "{}", output.stdout);
    let data = &envelope(&output)["data"];
    assert_eq!(data["mode"], "external");
    assert_eq!(data["rules"], EXTERNAL_RULES.trim_end());
    let rules = data["rules"].as_str().unwrap();
    assert!(!rules.contains("Nobody watches your terminal"));
    assert!(!rules.contains("end your turn: the office wakes you"));
    assert_eq!(stub.methods(), ["who", "process.effective"]);

    // 2. `agent-os exec` env-token external session learns `external` from `auth.whoami`.
    let mut stub = Stub::new(true);
    stub.whoami_mode = SessionMode::External;
    let (output, stub) = context(stub, &["context"]);
    assert_eq!(output.exit_code, 0, "{}", output.stdout);
    let data = &envelope(&output)["data"];
    assert_eq!(data["mode"], "external");
    assert_eq!(data["rules"], EXTERNAL_RULES.trim_end());
    assert_eq!(stub.methods(), ["auth.whoami", "who", "process.effective"]);
}

#[test]
fn must_follow_documents_over_8_kib_are_left_out_whole_and_named() {
    let mut stub = Stub::new(true);
    stub.documents = vec![
        (Some("first"), 5000, true),
        (Some("second"), 4000, true),
        (None, 10, true),
    ];
    let (output, _) = context(stub, &["context"]);
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
fn a_bound_context_whose_office_refuses_fails_closed_and_never_goes_offline() {
    let mut stub = Stub::new(true);
    stub.refuse = true;
    let (output, _) = context(stub, &["context"]);
    assert_eq!(output.exit_code, 1, "{}", output.stdout);
    let env = envelope(&output);
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "not_authorized");
}

#[test]
fn a_binding_requested_by_the_environment_fails_closed_without_a_token() {
    use crate::runtime::binding::Binding;
    use crate::runtime::office::BoundOffice;
    let mut office = BoundOffice::new(Binding::default()).with_binding_requested(true);
    let args = vec!["context".to_owned()];
    let output = crate::execute_with_office("agent-os", &args, &mut TransportOffice(&mut office));
    assert_eq!(output.exit_code, 1, "{}", output.stdout);
    assert_eq!(envelope(&output)["error"]["code"], "not_authorized");
    // Without the request, the same office answers offline.
    let mut office = BoundOffice::new(Binding::default());
    let output = crate::execute_with_office("agent-os", &args, &mut TransportOffice(&mut office));
    assert_eq!(output.exit_code, 0, "{}", output.stdout);
    assert_eq!(envelope(&output)["data"]["mode"], "offline");
}

#[test]
fn a_person_on_a_grant_reads_context_as_the_member_the_office_bound_never_as_an_agent_session() {
    let mut stub = Stub::new(true);
    stub.caller = None;
    let (output, stub) = context(stub, &["context"]);
    assert_eq!(output.exit_code, 0, "{}", output.stdout);
    assert_eq!(envelope(&output)["data"]["identity"]["id"], PERSON);
    assert_eq!(
        stub.methods(),
        ["auth.whoami", "who", "process.effective"],
        "member.session_inspect is for agents only"
    );
    assert_eq!(stub.calls[1].1, json!({ "member": PERSON }));
}

#[test]
fn an_office_without_auth_whoami_falls_back_to_the_session_and_any_other_refusal_stands() {
    for (code, expected) in [
        (
            ErrorCode::Unsupported,
            vec!["auth.whoami", "member.session_inspect"],
        ),
        (
            ErrorCode::NotBound,
            vec!["auth.whoami", "member.session_inspect"],
        ),
        (ErrorCode::NotAuthorized, vec!["auth.whoami"]),
    ] {
        let mut stub = Stub::new(true);
        stub.caller = None;
        stub.whoami_refused = Some(code);
        let (output, stub) = context(stub, &["context"]);
        assert_eq!(output.exit_code, 1, "{code:?}: {}", output.stdout);
        assert_eq!(envelope(&output)["ok"], false);
        assert_eq!(stub.methods(), expected, "{code:?}");
    }
}

#[test]
fn a_signed_in_workspace_is_bound_and_its_context_fails_closed_when_the_binding_is_gone() {
    use crate::runtime::binding::Binding;
    use crate::runtime::office::BoundOffice;
    let dir = tempfile::tempdir().expect("tempdir");
    let mut office = BoundOffice::new(Binding {
        external_state: Some(dir.path().join("signed-out")),
        ..Binding::default()
    })
    .with_workspace_connector(crate::features::auth::connect_workspace);
    assert!(
        office.is_bound(),
        "a workspace binding without a token yet is bound"
    );
    let args = vec!["context".to_owned()];
    let output = crate::execute_with_office("agent-os", &args, &mut TransportOffice(&mut office));
    assert_eq!(output.exit_code, 1, "{}", output.stdout);
    let env = envelope(&output);
    assert_eq!(
        (env["ok"].clone(), env["data"].clone()),
        (json!(false), Value::Null),
        "{env}"
    );
}
