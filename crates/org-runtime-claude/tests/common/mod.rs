//! Shared test helpers: specs, the spike's fixtures, and a replay harness that drives a real
//! Claude session through a recorded transcript.
#![allow(dead_code)] // each test binary uses its own subset

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

use org_runtime::{
    AgentIdentity, AgentName, AgentRuntime, CommandPrefix, Control, ControlError, Decision,
    ExitInfo, LaunchContext, LaunchPlan, McpServer, McpServerName, Output, PathPattern,
    PermissionMode, PermissionPolicy, PlainId, Rule, RuleScope, RuleTool, RuntimeId, SandboxSpec,
    Session, SessionEvent, SessionId, SessionKind, SessionSpec, SpecPath, ToolName,
};
use org_runtime_claude::ClaudeRuntime;
use serde_json::{json, Value};

pub const WORKSPACE: &str = "/private/var/office/work/t1";
pub const HOME: &str = "/private/var/office/homes/ada";
pub const RUNTIME_DIR: &str = "/private/var/office/agents/a1/runtime";
pub const TMPDIR: &str = "/private/var/folders/xy/T/agentic-org/s1";
pub const NEW_ID: &str = "11111111-2222-4333-8444-555555555555";

pub fn p(path: &str) -> SpecPath {
    SpecPath::try_from(path).unwrap()
}

pub fn pat(pattern: &str) -> PathPattern {
    PathPattern::try_from(pattern).unwrap()
}

pub fn rule(tool: &str, scope: RuleScope) -> Rule {
    Rule {
        tool: RuleTool::try_from(tool).unwrap(),
        scope,
    }
}

pub fn prefix(p: &str) -> RuleScope {
    RuleScope::CommandPrefix(CommandPrefix::try_from(p).unwrap())
}

pub fn path_scope(p: &str) -> RuleScope {
    RuleScope::Path(pat(p))
}

/// A realistic task-session spec for agent `name`.
pub fn spec(name: &str, mode: PermissionMode) -> SessionSpec {
    let mut mcp_servers = BTreeMap::new();
    mcp_servers.insert(
        McpServerName::try_from("office").unwrap(),
        McpServer {
            command: p("/private/var/office/bin/org-mcp"),
            args: vec!["--stdio".into()],
        },
    );
    SessionSpec {
        runtime: RuntimeId::try_from("claude").unwrap(),
        kind: SessionKind::Task {
            task_id: PlainId::try_from("t1").unwrap(),
        },
        agent: AgentIdentity {
            name: AgentName::try_from(name).unwrap(),
            description: "Staff engineer".into(),
            prompt: "You are Ada.".into(),
        },
        model: None,
        effort: None,
        workspace: p(WORKSPACE),
        read_only_dirs: vec![p(HOME)],
        instructions: "Do the task.\nThen stop.".into(),
        permission_mode: mode,
        policy: PermissionPolicy {
            version: 1,
            allow: vec![rule("Bash", prefix("git status"))],
            deny: vec![],
        },
        sandbox: SandboxSpec {
            enabled: true,
            writable_roots: vec![p(WORKSPACE), p("/private/var/office/work.git/objects")],
            deny_write: vec![pat("/private/var/office/work.git/HEAD")],
            deny_read: vec![pat("/private/var/office/offices/*/office.db")],
            allowed_unix_sockets: vec![p("/private/var/office/run/ada.sock")],
        },
        mcp_servers,
        tools: None,
    }
}

pub fn ctx() -> LaunchContext {
    LaunchContext {
        runtime_path: PathBuf::from("/opt/homebrew/Caskroom/claude-code/2.1.311/claude"),
        runtime_version: Some("2.1.311".into()),
        runtime_dir: p(RUNTIME_DIR),
        tmpdir: p(TMPDIR),
        office_env_names: vec![],
        new_session_id: Some(SessionId::try_from(NEW_ID).unwrap()),
        resume: None,
    }
}

pub fn plan(spec: &SessionSpec, ctx: &LaunchContext) -> (LaunchPlan, Box<dyn Session>) {
    ClaudeRuntime::new()
        .plan_session(spec, ctx)
        .unwrap_or_else(|e| panic!("plan_session refused: {e}"))
}

pub fn session(name: &str, mode: PermissionMode) -> Box<dyn Session> {
    plan(&spec(name, mode), &ctx()).1
}

/// A session launched with the conversation id the fixture's Claude reported (its first
/// `system/init`), as the office would have passed it with `--session-id`.
pub fn session_for(fixture_name: &str, name: &str, mode: PermissionMode) -> Box<dyn Session> {
    let mut c = ctx();
    if let Some(id) = fixture(fixture_name)
        .iter()
        .find(|r| r.msg["subtype"] == "init")
        .and_then(|r| r.msg["session_id"].as_str())
    {
        c.new_session_id = Some(SessionId::try_from(id).unwrap());
    }
    plan(&spec(name, mode), &c).1
}

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spikes/claude/fixtures")
}

/// A recorded line: `"in"` is what the spike's harness wrote to Claude's stdin, `"out"`
/// what Claude printed.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub to_claude: bool,
    pub msg: Value,
}

/// The fixture's lines, with the scrubber's `<uuid-N>` placeholders turned back into valid
/// uuids (a placeholder isn't a valid session id).
pub fn fixture(name: &str) -> Vec<Recorded> {
    let raw = std::fs::read_to_string(fixtures_dir().join(name))
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    let raw = unscrub_uuids(&raw);
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let v: Value = serde_json::from_str(l).unwrap();
            Recorded {
                to_claude: v["dir"] == "in",
                msg: v["msg"].clone(),
            }
        })
        .collect()
}

pub fn unscrub_uuids(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("<uuid-") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 6..];
        let end = after.find('>').unwrap();
        let n: u64 = after[..end].parse().unwrap();
        out.push_str(&uuid(n));
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

pub fn uuid(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012}")
}

/// The `initialize` reply recorded in `agent-inline-json.jsonl` (its agents include
/// `zephyr` and `claude`), re-addressed to `request_id`.
pub fn recorded_initialize_reply(request_id: &str) -> Value {
    let mut reply = fixture("agent-inline-json.jsonl")
        .into_iter()
        .find(|r| !r.to_claude && r.msg["type"] == "control_response")
        .unwrap()
        .msg;
    reply["response"]["request_id"] = json!(request_id);
    reply
}

pub fn bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap()
}

/// The JSON of a write.
pub fn written(out: &Output) -> Option<Value> {
    match out {
        Output::Write(line) => {
            let bytes = line.as_bytes();
            assert_eq!(bytes.last(), Some(&b'\n'));
            Some(serde_json::from_slice(&bytes[..bytes.len() - 1]).unwrap())
        }
        _ => None,
    }
}

pub fn writes(out: &[Output]) -> Vec<Value> {
    out.iter().filter_map(written).collect()
}

pub fn events(out: &[Output]) -> Vec<SessionEvent> {
    out.iter()
        .filter_map(|o| match o {
            Output::Event(e) => Some(e.clone()),
            _ => None,
        })
        .collect()
}

/// One line per output, for the expected-output files.
///
/// A rate limit is rendered by its window only: the spike's fixtures scrub every
/// `rate_limit_info` value, and type-preserving synthetic values will replace them, so the
/// values are asserted in `ratelimit`'s own tests, not here.
pub fn render(out: &Output) -> String {
    match out {
        Output::Event(SessionEvent::RateLimit(limit)) => format!(
            "event rate_limit {}",
            serde_json::to_string(&limit.window).unwrap()
        ),
        Output::Write(_) => format!("write {}", written(out).unwrap()),
        Output::Event(e) => format!("event {}", serde_json::to_string(e).unwrap()),
        Output::CloseStdin => "close_stdin".to_owned(),
    }
}

/// A short label for a recorded message, for the expected-output files.
fn label(r: &Recorded) -> String {
    let m = &r.msg;
    let t = m["type"].as_str().unwrap_or("?");
    let sub = match t {
        "system" => m["subtype"].as_str().map(str::to_owned),
        "control_request" => m["request"]["subtype"].as_str().map(str::to_owned),
        "stream_event" => m["event"]["type"].as_str().map(str::to_owned),
        "result" => m["subtype"].as_str().map(str::to_owned),
        _ => None,
    };
    let who = if r.to_claude {
        "to claude"
    } else {
        "from claude"
    };
    match sub {
        Some(s) => format!("{who}: {t}/{s}"),
        None => format!("{who}: {t}"),
    }
}

/// A recorded transcript replayed through a real session, the way the office would drive
/// it: "out" lines go to `on_line`; "in" lines become the controls that should produce them,
/// and the session's writes are checked against them.
pub struct Replay {
    pub session: Box<dyn Session>,
    pub now: Instant,
    /// Every output, in order.
    pub outputs: Vec<Output>,
    /// The expected-output file's text.
    pub log: Vec<String>,
    /// The fixture's own request ids for requests the session sent, mapped to the session's.
    ids: HashMap<String, String>,
    /// Sends refused as Busy, sent again once the turn ends (as the office queues them).
    queued: Vec<(String, Value)>,
    /// Answers Decision can't express (`updatedInput`, a deny without a message, an error).
    pub inexpressible: Vec<Value>,
    /// An agent to add to a recorded `initialize` reply that doesn't list it: most of the
    /// spike's runs passed no `--agents`, and the office always does.
    pub inject_agent: Option<String>,
}

impl Replay {
    pub fn new(session: Box<dyn Session>) -> Self {
        Replay {
            session,
            now: Instant::now(),
            outputs: vec![],
            log: vec![],
            ids: HashMap::new(),
            queued: vec![],
            inexpressible: vec![],
            inject_agent: None,
        }
    }

    /// Adds `agent` to recorded `initialize` replies that don't list it.
    pub fn injecting(mut self, agent: &str) -> Self {
        self.inject_agent = Some(agent.to_owned());
        self
    }

    fn take(&mut self, out: Vec<Output>) -> Vec<Output> {
        for o in &out {
            self.log.push(render(o));
        }
        self.outputs.extend(out.iter().cloned());
        out
    }

    pub fn control(&mut self, control: Control) -> Result<Vec<Output>, ControlError> {
        self.log.push(format!("# control {control:?}"));
        let result = self.session.control(control, self.now);
        match &result {
            Ok(out) => {
                let out = self.take(out.clone());
                Ok(out)
            }
            Err(e) => {
                self.log.push(format!("# refused: {e:?}"));
                Err(*e)
            }
        }
    }

    pub fn line(&mut self, msg: &Value) -> Vec<Output> {
        let out = self.session.on_line(&bytes(msg), self.now);
        self.take(out)
    }

    /// Runs the whole fixture. `before` issues extra controls before feeding line `n`
    /// (1-based), e.g. a Close the harness did by closing stdin.
    pub fn run(&mut self, name: &str, before: &[(usize, Control)]) {
        let lines = fixture(name);
        self.log.push(format!("# {name}"));
        let start = self.session.start(self.now);
        let start = self.take(start);
        let init = written(&start[0]).expect("start writes initialize");
        assert_eq!(init["request"], json!({"subtype": "initialize"}));
        let ours = init["request_id"].as_str().unwrap().to_owned();
        let mut first = 0;
        if lines[0].to_claude && lines[0].msg["type"] == "control_request" {
            // The fixture's own initialize: the session's must match it, id aside.
            let mut theirs = lines[0].msg.clone();
            let their_id = theirs["request_id"].as_str().unwrap().to_owned();
            theirs["request_id"] = json!(ours);
            assert_eq!(
                init, theirs,
                "the initialize request differs from the recording"
            );
            self.ids.insert(their_id, ours);
            first = 1;
        } else {
            // This process skipped initialize; open with a recorded reply.
            self.log
                .push("# opening: the reply recorded in agent-inline-json".into());
            self.line(&recorded_initialize_reply(&ours));
        }
        for (i, rec) in lines.iter().enumerate().skip(first) {
            for (n, control) in before {
                if *n == i + 1 {
                    let _ = self.control(control.clone());
                }
            }
            self.log.push(format!("# line {}: {}", i + 1, label(rec)));
            if rec.to_claude {
                self.sent_to_claude(&rec.msg);
            } else {
                let mut msg = rec.msg.clone();
                if msg["type"] == "control_response" {
                    let id = msg["response"]["request_id"].as_str().unwrap_or_default();
                    if let Some(ours) = self.ids.get(id) {
                        msg["response"]["request_id"] = json!(ours);
                    }
                    if let (Some(agent), Some(agents)) = (
                        &self.inject_agent,
                        msg.pointer_mut("/response/response/agents")
                            .and_then(Value::as_array_mut),
                    ) {
                        if !agents.iter().any(|a| a["name"] == json!(agent)) {
                            self.log
                                .push(format!("# (added agent {agent:?} to the reply)"));
                            agents.push(json!({"name": agent, "description": "Staff engineer"}));
                        }
                    }
                }
                let out = self.line(&msg);
                self.send_queued_after(&out);
            }
        }
    }

    /// Ends the replay: stdout closes and the process exits.
    pub fn finish(&mut self, exit: ExitInfo) {
        self.log.push("# stdout closed".into());
        let out = self.session.on_stdout_closed(self.now);
        self.take(out);
        self.log.push(format!("# exit {exit:?}"));
        let out = self.session.on_exit(exit, self.now);
        self.take(out);
    }

    fn send_queued_after(&mut self, out: &[Output]) {
        let ended = out
            .iter()
            .any(|o| matches!(o, Output::Event(SessionEvent::TurnEnded { .. })));
        if ended && !self.queued.is_empty() {
            let (text, expected) = self.queued.remove(0);
            let out = self
                .control(Control::Send {
                    text,
                    urgent: false,
                })
                .expect("a queued send after the turn");
            assert_eq!(writes(&out), vec![expected]);
        }
    }

    fn sent_to_claude(&mut self, msg: &Value) {
        match msg["type"].as_str() {
            Some("user") => {
                let text = msg["message"]["content"].as_str().unwrap().to_owned();
                match self.control(Control::Send {
                    text: text.clone(),
                    urgent: false,
                }) {
                    Ok(out) => assert_eq!(writes(&out), vec![msg.clone()], "a user line"),
                    Err(ControlError::Busy) => self.queued.push((text, msg.clone())),
                    // Logged; a test that expects it checks the log.
                    Err(_) => {}
                }
            }
            Some("control_request") => {
                assert_eq!(msg["request"]["subtype"], "interrupt");
                let out = self.control(Control::Interrupt).expect("Interrupt");
                let w = writes(&out);
                assert_eq!(w.len(), 1);
                let ours = w[0]["request_id"].as_str().unwrap().to_owned();
                let mut theirs = msg.clone();
                theirs["request_id"] = json!(ours);
                assert_eq!(w[0], theirs, "the interrupt request");
                self.ids
                    .insert(msg["request_id"].as_str().unwrap().to_owned(), ours);
            }
            Some("control_response") => {
                let response = &msg["response"];
                let id = response["request_id"].as_str().unwrap();
                let d = &response["response"];
                let decision = match (response["subtype"].as_str(), d["behavior"].as_str()) {
                    (Some("success"), Some("allow")) if d.get("updatedInput").is_none() => {
                        Some(Decision::AllowOnce)
                    }
                    (Some("success"), Some("deny")) if d["message"].is_string() => {
                        Some(Decision::Deny {
                            message: d["message"].as_str().map(str::to_owned),
                            stop_turn: d["interrupt"] == json!(true),
                        })
                    }
                    _ => None,
                };
                match decision {
                    Some(decision) => {
                        let out = self
                            .control(Control::Answer {
                                request_id: org_runtime::ApprovalId::new(id),
                                decision,
                            })
                            .expect("Answer");
                        assert_eq!(writes(&out), vec![msg.clone()], "the approval answer");
                    }
                    None => {
                        self.log
                            .push("# not expressible through Decision: not sent".into());
                        self.inexpressible.push(msg.clone());
                    }
                }
            }
            other => panic!("unexpected line to Claude: {other:?}"),
        }
    }

    pub fn events(&self) -> Vec<SessionEvent> {
        events(&self.outputs)
    }

    pub fn writes(&self) -> Vec<Value> {
        writes(&self.outputs)
    }

    /// Compares the log with `tests/expected/<name>.txt` (or writes it, with
    /// `UPDATE_EXPECTED=1`).
    pub fn check_expected(&self, name: &str) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/expected")
            .join(format!("{name}.txt"));
        let actual = self.log.join("\n") + "\n";
        if std::env::var_os("UPDATE_EXPECTED").is_some() {
            std::fs::write(&path, &actual).unwrap();
            return;
        }
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e} (run with UPDATE_EXPECTED=1)", path.display()));
        if expected != actual {
            let diff = expected
                .lines()
                .zip(actual.lines())
                .position(|(a, b)| a != b)
                .unwrap_or(expected.lines().count().min(actual.lines().count()));
            panic!(
                "{name}: the outputs differ from {} at line {}:\nexpected: {:?}\nactual:   {:?}",
                path.display(),
                diff + 1,
                expected.lines().nth(diff),
                actual.lines().nth(diff)
            );
        }
    }
}

pub const EXIT_0: ExitInfo = ExitInfo {
    code: Some(0),
    signal: None,
};

pub fn tool(name: &str) -> ToolName {
    ToolName::try_from(name).unwrap()
}
