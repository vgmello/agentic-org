//! The design §5 status rules, as a pure state machine (no I/O), so they can be
#![allow(dead_code)] // Status::NeedsYou, Sess::status and Engine::get are used by the table tests

//! table-tested by replaying recorded event sequences.
//!
//! State is keyed by **session id**. The agent label is bound once, from the
//! session's SessionStart (the mod's bound identity, else `agent_type`, else
//! `session_title`), and never from later events: PreToolUse carries no
//! identity, and inside a subagent `agent_type` is the subagent's type.
//!
//! "Needs you" reasons are additive:
//! - transient (permission / question): cleared by a later sequenced event that
//!   shows progress (not by backup notifications, not by ToolCall);
//! - goal paused and stop failure: kept until UserPromptSubmit or SessionStart;
//! - crash: kept until SessionStart (resume) or dismiss.

use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Offline,
    Idle,
    Working,
    NeedsYou,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Permission,
    Question,
}

/// A pending permission dialog or question, and what can clear it: a Post/Failure/Denied
/// event of the same call (tool_use_id), else the same tool, else the same MCP server,
/// from the same loop (main thread = None, else the subagent's id).
#[derive(Clone, Debug)]
pub struct Pending {
    pub kind: Kind,
    pub text: String,
    pub tool: Option<String>,
    pub tool_use_id: Option<String>,
    pub mcp_server: Option<String>,
    pub loop_id: Option<String>,
}

#[derive(Debug)]
pub struct Sess {
    pub agent: String,
    pub pid: Option<i32>,
    pub base: Status,
    pub transient: Option<Pending>,
    pub goal_paused: Option<String>,
    pub stop_failure: Option<String>,
    pub crashed: bool,
    pub last_seq: i64,
    pub epoch: Option<String>,
    pub subagents: BTreeSet<String>,
    pub stopped_with_bg: bool,
    pub ended: bool,
    pub superseded: bool,
}

impl Sess {
    fn new(agent: String) -> Self {
        Sess {
            agent,
            pid: None,
            base: Status::Idle,
            transient: None,
            goal_paused: None,
            stop_failure: None,
            crashed: false,
            last_seq: 0,
            epoch: None,
            subagents: BTreeSet::new(),
            stopped_with_bg: false,
            ended: false,
            superseded: false,
        }
    }

    /// Active "Needs you" reasons, most important first: (key, text).
    pub fn needs(&self) -> Vec<(String, String)> {
        let mut v = vec![];
        if self.crashed {
            v.push(("crash".into(), "Session crashed".into()));
        }
        if let Some(g) = &self.goal_paused {
            v.push(("goal".into(), format!("Goal paused ({g})")));
        }
        if let Some(f) = &self.stop_failure {
            v.push(("stopfail".into(), format!("Stopped with an error: {f}")));
        }
        match &self.transient {
            Some(p) if p.kind == Kind::Permission => v.push(("permission".into(), format!("Wants to run {}", p.text))),
            Some(p) => v.push(("question".into(), format!("Asked you a question ({})", p.text))),
            None => {}
        }
        v
    }

    pub fn status(&self) -> Status {
        if self.needs().is_empty() {
            self.base
        } else {
            Status::NeedsYou
        }
    }

    pub fn status_text(&self) -> String {
        let n = self.needs();
        if n.is_empty() {
            format!("{:?}", self.base)
        } else {
            format!("NeedsYou[{}]", n.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("; "))
        }
    }
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub session: String,
    pub agent: String,
    pub status: String,
    pub note: String,
    /// New "Needs you" reasons to notify (text).
    pub notify: Vec<String>,
    /// A pid to watch with kqueue.
    pub watch: Option<i32>,
    pub dropped: bool,
}

#[derive(Default)]
pub struct Engine {
    pub sessions: HashMap<String, Sess>,
}

/// Truncate on a char boundary (never panics).
pub fn trunc(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Parse a request body leniently: invalid UTF-8 is replaced, never panics, and
/// only a JSON object is an event (the log decorates it with fields).
pub fn parse_body(body: &[u8]) -> Result<Value, String> {
    let text = String::from_utf8_lossy(body);
    match serde_json::from_str::<Value>(&text) {
        Ok(v) if v.is_object() => Ok(v),
        Ok(_) => Err(format!("not a JSON object: {}", trunc(&text, 200))),
        Err(e) => Err(format!("bad JSON ({e}): {}", trunc(&text, 200))),
    }
}

fn s<'a>(ev: &'a Value, k: &str) -> &'a str {
    ev[k].as_str().unwrap_or("")
}

fn opt(ev: &Value, k: &str) -> Option<String> {
    ev[k].as_str().filter(|x| !x.is_empty()).map(String::from)
}

/// Which loop raised the event: None for the main thread, else the subagent's id
/// (`loop_agent_id` from the reworked mod; older events only say `in_subagent`).
fn loop_of(ev: &Value) -> Option<String> {
    opt(ev, "loop_agent_id").or_else(|| (ev["in_subagent"].as_bool() == Some(true)).then(|| "subagent".to_string()))
}

/// Does this Post/Failure/Denied event resolve the pending item?
fn resolves(p: &Pending, ev: &Value) -> bool {
    if loop_of(ev) != p.loop_id {
        return false;
    }
    let tool = s(ev, "tool");
    if let (Some(a), Some(b)) = (&p.tool_use_id, opt(ev, "tool_use_id")) {
        return *a == b;
    }
    if let Some(t) = &p.tool {
        return t == tool;
    }
    if let Some(server) = &p.mcp_server {
        return tool.starts_with(&format!("mcp__{server}__"));
    }
    true // a backup notification with no tool: any completed call of that loop
}

/// Background tasks that keep a stopped lead Working: subagents only (shell,
/// monitor and workflow tasks can outlive the turn indefinitely).
fn background_subagents(ev: &Value, fallback: usize) -> usize {
    match ev["background_task_kinds"].as_array() {
        Some(kinds) => kinds.iter().filter(|k| k.as_str() == Some("subagent")).count(),
        None => ev["background_tasks"].as_u64().map(|n| n as usize).unwrap_or(fallback),
    }
}

impl Engine {
    pub fn get(&self, session: &str) -> Option<&Sess> {
        self.sessions.get(session)
    }

    /// Apply one event from session process `session_pid` (the kernel-verified socket peer,
    /// or its parent for the curl transport; <= 1 means unknown, e.g. reparented to launchd).
    pub fn event(&mut self, ev: &Value, session_pid: i32) -> Outcome {
        let mut out = Outcome::default();
        if ev["v"].as_i64() != Some(1) {
            out.dropped = true;
            out.note = format!("REJECT schema v={}", ev["v"]);
            return out;
        }
        let pid = (session_pid > 1).then_some(session_pid);
        let sid = ev["session_id"].as_str().unwrap_or("?").to_string();
        let hook = s(ev, "hook").to_string();
        out.session = sid.clone();
        let mut note = String::new();

        if hook == "SessionStart" {
            // A /clear or /resume successor: same process, predecessor superseded.
            let pred = self
                .sessions
                .iter()
                .find(|(k, x)| x.superseded && pid.is_some() && x.pid == pid && **k != sid)
                .map(|(k, x)| (k.clone(), x.agent.clone()));
            let bound = ev["agent"]
                .as_str()
                .or(ev["agent_type"].as_str())
                .or(ev["session_title"].as_str())
                .map(|x| x.to_string());
            let label = match (&pred, bound) {
                (Some((pk, pa)), _) => {
                    note.push_str(&format!("successor of {} (source={}); ", trunc(pk, 8), s(ev, "source")));
                    pa.clone()
                }
                (None, Some(b)) => b,
                (None, None) => format!("session:{}", trunc(&sid, 8)),
            };
            if let Some((pk, _)) = pred {
                self.sessions.remove(&pk);
            }
            let x = self.sessions.entry(sid.clone()).or_insert_with(|| Sess::new(label.clone()));
            // SessionStart resets everything, including a held paused goal or crash.
            if x.crashed {
                note.push_str("crash cleared by SessionStart; ");
            }
            *x = Sess { agent: x.agent.clone(), ..Sess::new(String::new()) };
            x.base = Status::Idle;
        }

        let Some(x) = self.sessions.get_mut(&sid) else {
            // Event for a session we never saw start (receiver started late): adopt it.
            let label = ev["agent"].as_str().map(|a| a.to_string()).unwrap_or_else(|| format!("session:{}", trunc(&sid, 8)));
            self.sessions.insert(sid.clone(), Sess::new(label));
            return self.event(ev, session_pid);
        };

        // A new module load (reload, restart) restarts seq: a new epoch resets the window.
        if let Some(ep) = opt(ev, "epoch") {
            if x.epoch.as_deref().is_some_and(|old| old != ep) {
                note.push_str(&format!("new epoch {} (module reloaded): seq window reset; ", trunc(&ep, 8)));
                x.last_seq = 0;
                // Re-bind identity from the new load, but never from a Subagent* event
                // (its agent_type is the subagent's).
                let hook = opt(ev, "hook").unwrap_or_default();
                if hook != "SubagentStart" && hook != "SubagentStop" {
                    if let Some(a) = opt(ev, "agent") {
                        x.agent = a;
                    }
                }
            }
            x.epoch = Some(ep);
        }
        out.agent = x.agent.clone();

        if let Some(q) = ev["seq"].as_i64() {
            if q <= x.last_seq && hook != "SessionStart" {
                out.dropped = true;
                out.note = format!("{note}STALE seq={q} (last {}) dropped", x.last_seq);
                out.status = x.status_text();
                return out;
            }
            x.last_seq = q;
        }
        // The session's pid is bound once, at SessionStart (or first sight); a later
        // event can't move it (a reparented curl would claim pid 1, a forged one any pid).
        if let Some(p) = pid {
            if x.pid.is_none() || (hook == "SessionStart" && x.pid != Some(p)) {
                x.pid = Some(p);
                if !x.ended {
                    out.watch = Some(p);
                }
            } else if x.pid != Some(p) {
                note.push_str(&format!("event from pid {p} ignored for liveness (bound {:?}); ", x.pid));
            }
        }

        let before: Vec<String> = x.needs().into_iter().map(|(k, _)| k).collect();
        let tool = s(ev, "tool").to_string();
        let ntype = s(ev, "notification_type").to_string();
        let lp = loop_of(ev);
        let pending = |kind: Kind, text: String| Pending {
            kind,
            text,
            tool: opt(ev, "tool"),
            tool_use_id: opt(ev, "tool_use_id"),
            mcp_server: opt(ev, "mcp_server"),
            loop_id: lp.clone(),
        };
        let set_transient = |x: &mut Sess, t: Pending, note: &mut String| match &x.transient {
            None => x.transient = Some(t),
            // A question outranks a permission for the same pending item (AskUserQuestion).
            Some(p) if p.kind == Kind::Permission && t.kind == Kind::Question => x.transient = Some(t),
            _ => note.push_str("already waiting; kept existing reason; "),
        };

        match hook.as_str() {
            "SessionStart" => {}
            "UserPromptSubmit" => {
                if x.goal_paused.take().is_some() {
                    note.push_str("paused goal cleared by owner's prompt; ");
                }
                if x.stop_failure.take().is_some() {
                    note.push_str("stop failure cleared by owner's prompt; ");
                }
                x.transient = None;
                x.stopped_with_bg = false;
                x.base = Status::Working;
            }
            "PreToolUse" if tool == "AskUserQuestion" => {
                set_transient(x, pending(Kind::Question, "AskUserQuestion".into()), &mut note);
                x.base = Status::Working;
            }
            // Not a clear: a parallel call or a subagent's call says nothing about the dialog.
            "PreToolUse" | "ToolCall" => x.base = Status::Working,
            "PostToolUse" | "PostToolUseFailure" | "PermissionDenied" => {
                if x.transient.as_ref().is_some_and(|p| resolves(p, ev)) {
                    x.transient = None;
                    note.push_str("pending item resolved by its own call; ");
                } else if x.transient.is_some() {
                    note.push_str("unrelated call; pending item kept; ");
                }
                x.base = Status::Working;
            }
            "PermissionRequest" => {
                if ev["decided"].as_bool() == Some(true) {
                    note.push_str(&format!("decided below ({}) -> no Needs you; ", ev["decision"]));
                } else if tool == "AskUserQuestion" {
                    set_transient(x, pending(Kind::Question, "AskUserQuestion".into()), &mut note);
                } else {
                    set_transient(x, pending(Kind::Permission, tool.clone()), &mut note);
                }
            }
            "Notification" => match ntype.as_str() {
                "permission_prompt" => set_transient(x, pending(Kind::Permission, format!("(notification) {}", s(ev, "message"))), &mut note),
                "elicitation_dialog" | "elicitation_url_dialog" => set_transient(x, pending(Kind::Question, ntype.clone()), &mut note),
                "elicitation_response" => {
                    if x.transient.as_ref().is_some_and(|p| p.kind == Kind::Question) {
                        x.transient = None;
                    }
                    note.push_str("elicitation answered; ");
                }
                "idle_prompt" => {
                    x.transient = None;
                    x.base = Status::Idle;
                }
                "agent_needs_input" => note.push_str("agent_needs_input is about ANOTHER session; ignored; "),
                _ => note.push_str(&format!("unmapped notification_type={ntype}; ")),
            },
            "Elicitation" => set_transient(x, pending(Kind::Question, format!("Elicitation from {}", s(ev, "mcp_server"))), &mut note),
            "Stop" => {
                if lp.is_some() {
                    note.push_str("subagent Stop ignored; ");
                } else {
                    if x.transient.as_ref().is_some_and(|p| p.loop_id.is_none()) {
                        x.transient = None; // the main turn ended: its dialog is gone
                    }
                    if ev["blocked"].as_bool() == Some(true) {
                        note.push_str("stop blocked below -> Working; ");
                        x.base = Status::Working;
                    } else {
                        let bg = background_subagents(ev, x.subagents.len());
                        if bg > 0 {
                            note.push_str(&format!("stopped with {bg} background subagent(s) -> Working; "));
                            x.base = Status::Working;
                            x.stopped_with_bg = true;
                        } else {
                            x.base = Status::Idle;
                            x.stopped_with_bg = false;
                        }
                    }
                }
            }
            "SubagentStart" => {
                x.subagents.insert(s(ev, "subagent_id").to_string());
                x.base = Status::Working;
            }
            "SubagentStop" => {
                if x.subagents.remove(s(ev, "subagent_id")) {
                    note.push_str(&format!("subagents={}; ", x.subagents.len()));
                    if x.stopped_with_bg && x.subagents.is_empty() {
                        note.push_str("last subagent done after lead stopped -> Idle; ");
                        x.base = Status::Idle;
                        x.stopped_with_bg = false;
                    }
                } else {
                    note.push_str("unmatched SubagentStop (fork) ignored; ");
                }
            }
            "GoalPaused" => {
                x.goal_paused = Some(format!("after {} failed checks", ev["blocks_in_a_row"].as_i64().unwrap_or(-1)));
            }
            "StopFailure" => {
                x.transient = None;
                x.stop_failure = Some(format!("{} {}", s(ev, "error"), trunc(s(ev, "error_details"), 120)).trim().to_string());
            }
            "SessionEnd" => {
                let reason = s(ev, "reason");
                if reason == "clear" || reason == "resume" {
                    x.superseded = true;
                    note.push_str(&format!("SessionEnd reason={reason}: unchanged, expect successor SessionStart; "));
                } else {
                    end_session(x);
                    note.push_str(&format!("SessionEnd reason={reason} -> Offline; "));
                }
            }
            other => note.push_str(&format!("unknown hook {other}; ")),
        }

        for (k, text) in x.needs() {
            if !before.contains(&k) {
                out.notify.push(text);
            }
        }
        out.status = x.status_text();
        out.note = note;
        out
    }

    /// A watched pid exited (call after the grace period). `state_file` returns a session's
    /// latest normalized event as the mod wrote it: a SessionEnd there whose POST never
    /// arrived (receiver hung, POSTs skipped) means a clean exit, not a crash.
    pub fn exited(&mut self, pid: i32, state_file: &dyn Fn(&str) -> Option<Value>) -> Vec<Outcome> {
        let mut outs = vec![];
        for (sid, x) in self.sessions.iter_mut().filter(|(_, x)| x.pid == Some(pid)) {
            let mut o = Outcome { session: sid.clone(), agent: x.agent.clone(), ..Default::default() };
            let ended_in_file = state_file(sid).is_some_and(|v| {
                v["hook"] == "SessionEnd" && !matches!(v["reason"].as_str(), Some("clear") | Some("resume"))
            });
            if x.ended {
                o.note = "exit after SessionEnd: clean".into();
            } else if ended_in_file {
                end_session(x);
                o.note = "exit; SessionEnd found in the state file (its POST never arrived) -> Offline, clean".into();
            } else {
                let had = x.crashed;
                x.crashed = true;
                x.transient = None;
                o.note = "exit with no SessionEnd (socket or state file) -> crash".into();
                if !had {
                    o.notify.push("Session crashed".into());
                }
            }
            o.status = x.status_text();
            outs.push(o);
        }
        outs
    }
}

fn end_session(x: &mut Sess) {
    x.ended = true;
    x.transient = None;
    x.goal_paused = None;
    x.stop_failure = None;
    x.base = Status::Offline;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Replay a recorded fixture (raw events as the receiver logged them, with `_peer_pid`).
    fn replay(e: &mut Engine, fixture: &str) -> Vec<(String, i64, Status, String, Vec<String>)> {
        fixture
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let ev: Value = serde_json::from_str(l).unwrap();
                // `_session_pid` (curl transport: the peer's parent) when recorded, else the peer itself.
                let pid = ev["_session_pid"].as_i64().or(ev["_peer_pid"].as_i64()).unwrap_or(1) as i32;
                let o = e.event(&ev, pid);
                let st = e.get(&o.session).map(|x| x.status()).unwrap_or(Status::Offline);
                (ev["hook"].as_str().unwrap_or("").to_string(), ev["seq"].as_i64().unwrap_or(-1), st, o.status, o.notify)
            })
            .collect()
    }

    fn at<'a>(rows: &'a [(String, i64, Status, String, Vec<String>)], hook: &str, seq: i64) -> &'a (String, i64, Status, String, Vec<String>) {
        rows.iter().find(|r| r.0 == hook && r.1 == seq).unwrap_or_else(|| panic!("no {hook} seq={seq}"))
    }

    fn notifications(rows: &[(String, i64, Status, String, Vec<String>)]) -> Vec<String> {
        rows.iter().flat_map(|r| r.4.clone()).collect()
    }

    #[test]
    fn goal_paused_is_held_through_stop_and_only_prompt_or_start_clears() {
        let mut e = Engine::default();
        let rows = replay(&mut e, include_str!("../tests/fixtures/57a6a89b-goal-pause.jsonl"));
        // Unmatched fork SubagentStop events don't disturb anything.
        assert_eq!(at(&rows, "SubagentStop", 3).2, Status::Working);
        assert_eq!(at(&rows, "Stop", 4).2, Status::Working); // blocked by goals
        let paused = rows.iter().find(|r| r.0 == "GoalPaused").unwrap();
        assert_eq!(paused.2, Status::NeedsYou);
        assert_eq!(paused.4, vec!["Goal paused (after 2 failed checks)".to_string()]);
        assert_eq!(at(&rows, "Stop", 6).2, Status::NeedsYou); // held through the unblocked Stop
        assert_eq!(notifications(&rows).len(), 1);
        // A later question adds a reason; it doesn't replace the paused goal.
        let sid = "57a6a89b-6ee5-4080-ac46-37f0c1dacbc9";
        let mut e2 = Engine::default();
        let fixture = include_str!("../tests/fixtures/57a6a89b-goal-pause.jsonl");
        let upto: String = fixture.lines().take_while(|l| !l.contains("\"SessionEnd\"")).map(|l| format!("{l}\n")).collect();
        replay(&mut e2, &upto);
        let pid = e2.get(sid).unwrap().pid.unwrap();
        let o = e2.event(&serde_json::json!({"v":1,"session_id":sid,"hook":"PreToolUse","tool":"AskUserQuestion","seq":100}), pid);
        assert!(o.status.contains("Goal paused") && o.status.contains("question"), "{}", o.status);
        // A ToolCall (sequenced heartbeat) clears neither.
        let o = e2.event(&serde_json::json!({"v":1,"session_id":sid,"hook":"ToolCall","tool":"mcp__goals__CreateGoal","seq":101}), pid);
        assert!(o.status.contains("Goal paused") && o.status.contains("question"), "{}", o.status);
        // Progress clears the question but keeps the paused goal.
        let o = e2.event(&serde_json::json!({"v":1,"session_id":sid,"hook":"PostToolUse","tool":"AskUserQuestion","seq":102}), pid);
        assert!(o.status.contains("Goal paused") && !o.status.contains("question"), "{}", o.status);
        let o = e2.event(&serde_json::json!({"v":1,"session_id":sid,"hook":"UserPromptSubmit","seq":103}), pid);
        assert_eq!(o.status, "Working");
    }

    #[test]
    fn ask_user_question_notifies_once_and_post_tool_use_clears() {
        let mut e = Engine::default();
        let rows = replay(&mut e, include_str!("../tests/fixtures/09b93d00-interactive.jsonl"));
        // seq 10 Pre(AUQ), 11 PermissionRequest(AUQ), 12 permission_prompt, 13 PostToolUse(AUQ)
        assert_eq!(at(&rows, "PreToolUse", 10).2, Status::NeedsYou);
        assert_eq!(at(&rows, "PreToolUse", 10).4.len(), 1);
        assert!(at(&rows, "PermissionRequest", 11).4.is_empty());
        assert!(at(&rows, "Notification", 12).4.is_empty());
        assert_eq!(at(&rows, "PostToolUse", 13).2, Status::Working);
    }

    #[test]
    fn permission_undecided_notifies_once_backup_does_not_renotify() {
        let mut e = Engine::default();
        let rows = replay(&mut e, include_str!("../tests/fixtures/09b93d00-interactive.jsonl"));
        assert_eq!(at(&rows, "PermissionRequest", 4).2, Status::NeedsYou);
        assert_eq!(at(&rows, "PermissionRequest", 4).4, vec!["Wants to run Bash".to_string()]);
        assert!(at(&rows, "Notification", 5).4.is_empty()); // permission_prompt 6 s later: backup only
        assert_eq!(at(&rows, "PostToolUse", 6).2, Status::Working);
        assert_eq!(at(&rows, "Stop", 7).2, Status::Idle);
        // Elicitation: Needs you, backup dialog doesn't re-notify, elicitation_response clears.
        assert_eq!(at(&rows, "Elicitation", 20).2, Status::NeedsYou);
        assert!(at(&rows, "Notification", 21).4.is_empty());
        assert_eq!(at(&rows, "Notification", 22).2, Status::Working);
    }

    #[test]
    fn permission_decided_by_a_hook_raises_nothing() {
        let mut e = Engine::default();
        let rows = replay(&mut e, include_str!("../tests/fixtures/b57290ef-hook-allows.jsonl"));
        assert_eq!(at(&rows, "PermissionRequest", 4).2, Status::Working);
        assert!(notifications(&rows).is_empty());
    }

    #[test]
    fn stop_blocked_is_working_unblocked_is_idle() {
        let mut e = Engine::default();
        let rows = replay(&mut e, include_str!("../tests/fixtures/57a6a89b-goal-pause.jsonl"));
        assert_eq!(at(&rows, "Stop", 4).2, Status::Working);
        let mut e = Engine::default();
        let rows = replay(&mut e, include_str!("../tests/fixtures/130d2229-read.jsonl"));
        assert_eq!(at(&rows, "Stop", 5).2, Status::Idle);
        assert_eq!(at(&rows, "SessionEnd", 6).2, Status::Offline);
    }

    #[test]
    fn stop_with_background_subagent_is_working_until_its_subagent_stop() {
        let mut e = Engine::default();
        let rows = replay(&mut e, include_str!("../tests/fixtures/b1357688-subagent.jsonl"));
        assert_eq!(at(&rows, "Stop", 7).2, Status::Working);
        assert_eq!(at(&rows, "SubagentStop", 9).2, Status::Idle);
    }

    #[test]
    fn unmatched_fork_subagent_stop_does_not_flip_status() {
        let mut e = Engine::default();
        let rows = replay(&mut e, include_str!("../tests/fixtures/09b93d00-interactive.jsonl"));
        // seq 8: SubagentStop from an internal fork right after the Stop (seq 7): stays Idle, no counter underflow.
        assert_eq!(at(&rows, "SubagentStop", 8).2, Status::Idle);
        assert_eq!(at(&rows, "SubagentStop", 15).2, Status::Idle);
    }

    #[test]
    fn clear_moves_state_to_the_successor_session_and_exit_is_offline() {
        let mut e = Engine::default();
        let rows = replay(&mut e, include_str!("../tests/fixtures/09b93d00-interactive.jsonl"));
        assert_eq!(at(&rows, "SessionEnd", 26).2, Status::Idle); // clear: unchanged
        assert_eq!(at(&rows, "SessionStart", 27).2, Status::Idle);
        assert!(e.get("09b93d00-508a-4f14-8cb7-6d64d376cf55").is_none(), "predecessor replaced");
        let succ = e.get("202068d2-fa0f-47a5-94da-57c43a5f4eca").unwrap();
        assert_eq!(succ.agent, "zz-spike-tester-sam");
        assert_eq!(at(&rows, "SessionEnd", 28).2, Status::Offline);
        // The process exit after SessionEnd is clean.
        let outs = e.exited(succ.pid.unwrap(), &|_| None);
        assert!(outs.iter().all(|o| o.notify.is_empty()));
    }

    #[test]
    fn resume_successor_then_kill_is_a_crash() {
        let mut e = Engine::default();
        let rows = replay(&mut e, include_str!("../tests/fixtures/d84714a9-resume-crash.jsonl"));
        assert_eq!(at(&rows, "SessionEnd", 4).2, Status::Idle);
        assert_eq!(at(&rows, "SessionStart", 5).2, Status::Idle);
        let succ = e.get("09b93d00-508a-4f14-8cb7-6d64d376cf55").unwrap();
        let outs = e.exited(succ.pid.unwrap(), &|_| None);
        assert_eq!(outs.len(), 1);
        assert_eq!(outs[0].notify, vec!["Session crashed".to_string()]);
        assert_eq!(e.get("09b93d00-508a-4f14-8cb7-6d64d376cf55").unwrap().status(), Status::NeedsYou);
    }

    #[test]
    fn clean_exit_is_not_a_crash() {
        let mut e = Engine::default();
        replay(&mut e, include_str!("../tests/fixtures/130d2229-read.jsonl"));
        let pid = e.get("130d2229-dea5-479f-9690-6796853e2735").unwrap().pid.unwrap();
        assert!(e.exited(pid, &|_| None).iter().all(|o| o.notify.is_empty()));
    }

    #[test]
    fn stop_failure_needs_you() {
        let mut e = Engine::default();
        let rows = replay(&mut e, include_str!("../tests/fixtures/03713d34-stopfailure.jsonl"));
        assert_eq!(at(&rows, "StopFailure", 3).2, Status::NeedsYou);
        assert_eq!(at(&rows, "StopFailure", 3).4.len(), 1);
    }

    #[test]
    fn identity_is_bound_once_per_session_without_ac_agent() {
        // Recorded live without AC_AGENT (new mod: agent bound once from SessionStart).
        let mut e = Engine::default();
        let rows = replay(&mut e, include_str!("../tests/fixtures/22c7c00b-no-ac-agent.jsonl"));
        assert_eq!(e.sessions.len(), 1);
        assert_eq!(e.sessions.values().next().unwrap().agent, "zz-spike-tester-sam");
        assert_eq!(at(&rows, "StopFailure", 3).2, Status::NeedsYou); // rate_limit, a natural StopFailure
        // Replay of the OLD mod without AC_AGENT (agent = AC_AGENT ?? e.agent_type): keyed by its
        // `agent` field this was 3 agents (the bug); keyed by session it is one, labelled at SessionStart.
        let fixture = include_str!("../tests/fixtures/b1357688-replayed-no-ac-agent.jsonl");
        let old_keys: BTreeSet<String> = fixture
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .map(|v| v["agent"].as_str().map(String::from).unwrap_or_else(|| format!("session:{}", &v["session_id"].as_str().unwrap()[..8])))
            .collect();
        assert_eq!(old_keys.len(), 3, "{old_keys:?}");
        let mut e = Engine::default();
        let rows = replay(&mut e, fixture);
        assert_eq!(e.sessions.len(), 1, "no phantom agents: {:?}", e.sessions.keys().collect::<Vec<_>>());
        assert_eq!(e.sessions.values().next().unwrap().agent, "zz-spike-tester-sam");
        assert_eq!(at(&rows, "Stop", 7).2, Status::Working);
        assert_eq!(at(&rows, "SubagentStop", 9).2, Status::Idle);
        // And with the OLD mod's events (no bound agent on PreToolUse, subagent's agent_type on SubagentStart),
        // keying by session still gives one state with the SessionStart label.
        let mut e = Engine::default();
        let sid = "s-old";
        e.event(&serde_json::json!({"v":1,"session_id":sid,"hook":"SessionStart","agent":null,"agent_type":"zz-x","seq":1}), 7);
        e.event(&serde_json::json!({"v":1,"session_id":sid,"hook":"PreToolUse","agent":null,"tool":"Agent","seq":2}), 7);
        e.event(&serde_json::json!({"v":1,"session_id":sid,"hook":"SubagentStart","agent":"general-purpose","subagent_id":"a1","seq":3}), 7);
        assert_eq!(e.sessions.len(), 1);
        assert_eq!(e.get(sid).unwrap().agent, "zz-x");
    }

    #[test]
    fn live_no_ac_agent_run_with_subagent_is_one_session() {
        // Live run without AC_AGENT, new mod, curl transport: subagent events carry the
        // subagent's agent_type in the payload but the bound identity in `agent`.
        let mut e = Engine::default();
        let rows = replay(&mut e, include_str!("../tests/fixtures/4d279cdf-no-ac-agent-subagent-live.jsonl"));
        assert_eq!(e.sessions.len(), 1);
        assert_eq!(e.sessions.values().next().unwrap().agent, "zz-spike-tester-sam");
        assert_eq!(at(&rows, "Stop", 7).2, Status::Working);
        assert_eq!(at(&rows, "SubagentStop", 11).2, Status::Idle);
        assert_eq!(at(&rows, "SessionEnd", 14).2, Status::Offline);
        let pid = e.sessions.values().next().unwrap().pid.unwrap();
        assert!(e.exited(pid, &|_| None).iter().all(|o| o.notify.is_empty()), "session pid resolved from the curl peer's parent");
    }

    #[test]
    fn goals_loaded_first_hides_its_block_from_presence() {
        // Recorded with --plugin-dir goals first: the first Stop was blocked by goals (the next
        // Stop has stop_hook_active=true), but presence, beneath goals, reports blocked=false.
        let fixture = include_str!("../tests/fixtures/59e9fda0-goals-first-active-goal.jsonl");
        let stops: Vec<Value> = fixture.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).filter(|v| v["hook"] == "Stop").collect();
        assert_eq!(stops[0]["blocked"], false);
        assert_eq!(stops[1]["stop_hook_active"], true);
        let mut e = Engine::default();
        let rows = replay(&mut e, fixture);
        assert_eq!(at(&rows, "Stop", 4).2, Status::Idle, "the false Idle this ordering produces");
        assert_eq!(at(&rows, "Stop", 6).2, Status::NeedsYou);
    }

    fn upto(fixture: &str, hook: &str, seq: i64) -> String {
        let mut out = String::new();
        for l in fixture.lines() {
            out.push_str(l);
            out.push('\n');
            let v: Value = serde_json::from_str(l).unwrap();
            if v["hook"] == hook && v["seq"] == seq {
                break;
            }
        }
        out
    }

    #[test]
    fn pending_permission_is_cleared_only_by_its_own_call() {
        // Recorded: PermissionRequest(Bash) pending at 09b93d00 seq 4. Injected after it: a
        // background subagent's Read, a parallel main-thread Read, then the Bash call itself.
        let sid = "09b93d00-508a-4f14-8cb7-6d64d376cf55";
        let fixture = include_str!("../tests/fixtures/09b93d00-interactive.jsonl");
        let mut e = Engine::default();
        replay(&mut e, &upto(fixture, "PermissionRequest", 4));
        let pid = e.get(sid).unwrap().pid.unwrap();
        let mut ev = |j: Value| e.event(&j, pid).status;
        let st = ev(serde_json::json!({"v":1,"session_id":sid,"seq":40,"hook":"PostToolUse","tool":"Read","loop_agent_id":"a1","in_subagent":true}));
        assert!(st.contains("Wants to run Bash"), "subagent call cleared it: {st}");
        let st = ev(serde_json::json!({"v":1,"session_id":sid,"seq":41,"hook":"PreToolUse","tool":"Read"}));
        assert!(st.contains("Wants to run Bash"), "parallel PreToolUse cleared it: {st}");
        let st = ev(serde_json::json!({"v":1,"session_id":sid,"seq":42,"hook":"PostToolUse","tool":"Read"}));
        assert!(st.contains("Wants to run Bash"), "another tool's Post cleared it: {st}");
        let st = ev(serde_json::json!({"v":1,"session_id":sid,"seq":43,"hook":"PostToolUse","tool":"Bash"}));
        assert_eq!(st, "Working");
        // With tool_use_ids: only the same call resolves a question.
        let st = ev(serde_json::json!({"v":1,"session_id":sid,"seq":44,"hook":"PreToolUse","tool":"AskUserQuestion","tool_use_id":"t1"}));
        assert!(st.contains("question"));
        let st = ev(serde_json::json!({"v":1,"session_id":sid,"seq":45,"hook":"PostToolUse","tool":"AskUserQuestion","tool_use_id":"t2"}));
        assert!(st.contains("question"), "a different call cleared it: {st}");
        let st = ev(serde_json::json!({"v":1,"session_id":sid,"seq":46,"hook":"PostToolUse","tool":"AskUserQuestion","tool_use_id":"t1"}));
        assert_eq!(st, "Working");
        // A subagent's own permission is resolved by that subagent's call.
        let st = ev(serde_json::json!({"v":1,"session_id":sid,"seq":47,"hook":"PermissionRequest","tool":"Bash","decided":false,"loop_agent_id":"a1","in_subagent":true}));
        assert!(st.contains("Wants to run Bash"));
        assert!(ev(serde_json::json!({"v":1,"session_id":sid,"seq":48,"hook":"PostToolUse","tool":"Bash"})).contains("Wants"), "main thread's Bash cleared a subagent's request");
        assert_eq!(ev(serde_json::json!({"v":1,"session_id":sid,"seq":49,"hook":"PostToolUse","tool":"Bash","loop_agent_id":"a1","in_subagent":true})), "Working");
    }

    #[test]
    fn only_background_subagents_keep_a_stopped_lead_working() {
        let fixture = include_str!("../tests/fixtures/130d2229-read.jsonl");
        let sid = "130d2229-dea5-479f-9690-6796853e2735";
        let mut e = Engine::default();
        replay(&mut e, &upto(fixture, "PostToolUse", 4));
        let pid = e.get(sid).unwrap().pid.unwrap();
        let o = e.event(&serde_json::json!({"v":1,"session_id":sid,"seq":50,"hook":"Stop","blocked":false,"background_tasks":1,"background_task_kinds":["shell"]}), pid);
        assert_eq!(o.status, "Idle", "a background shell must not keep it Working");
        e.event(&serde_json::json!({"v":1,"session_id":sid,"seq":51,"hook":"UserPromptSubmit"}), pid);
        let o = e.event(&serde_json::json!({"v":1,"session_id":sid,"seq":52,"hook":"Stop","blocked":false,"background_tasks":3,"background_task_kinds":["shell","subagent","monitor"]}), pid);
        assert_eq!(o.status, "Working");
    }

    #[test]
    fn pid_is_bound_at_session_start_and_a_reparented_curl_cannot_move_it() {
        let fixture = include_str!("../tests/fixtures/4d279cdf-no-ac-agent-subagent-live.jsonl");
        let sid = "4d279cdf-d3af-46e7-b654-38791750a8bc";
        let mut e = Engine::default();
        replay(&mut e, &upto(fixture, "Stop", 7));
        let real = e.get(sid).unwrap().pid.unwrap();
        // A curl reparented to launchd (ppid 1 -> session_pid -1 from the receiver), then a stray pid.
        e.event(&serde_json::json!({"v":1,"session_id":sid,"seq":60,"hook":"PostToolUse","tool":"Bash","transport":"curl"}), -1);
        e.event(&serde_json::json!({"v":1,"session_id":sid,"seq":61,"hook":"PostToolUse","tool":"Bash"}), 1);
        e.event(&serde_json::json!({"v":1,"session_id":sid,"seq":62,"hook":"PostToolUse","tool":"Bash"}), 4242);
        assert_eq!(e.get(sid).unwrap().pid, Some(real));
        let outs = e.exited(real, &|_| None);
        assert_eq!(outs.len(), 1);
        assert_eq!(outs[0].notify, vec!["Session crashed".to_string()]);
    }

    #[test]
    fn session_end_in_the_state_file_means_clean_exit_not_crash() {
        // The receiver was hung, so the SessionEnd POST was skipped; the mod's state file has it.
        let fixture = include_str!("../tests/fixtures/130d2229-read.jsonl");
        let sid = "130d2229-dea5-479f-9690-6796853e2735";
        let mut e = Engine::default();
        replay(&mut e, &upto(fixture, "Stop", 5));
        let pid = e.get(sid).unwrap().pid.unwrap();
        let file = |s: &str| (s == sid).then(|| serde_json::json!({"v":1,"session_id":sid,"hook":"SessionEnd","reason":"other","seq":6}));
        let outs = e.exited(pid, &file);
        assert!(outs[0].notify.is_empty(), "{:?}", outs[0].note);
        assert_eq!(e.get(sid).unwrap().status(), Status::Offline);
        // A state file showing /clear is not an end: that is still a crash.
        let mut e = Engine::default();
        replay(&mut e, &upto(fixture, "Stop", 5));
        let file = |_: &str| Some(serde_json::json!({"hook":"SessionEnd","reason":"clear"}));
        assert_eq!(e.exited(pid, &file)[0].notify, vec!["Session crashed".to_string()]);
    }

    #[test]
    fn a_new_epoch_resets_the_seq_window_and_rebinds_identity() {
        let mut e = Engine::default();
        let j = |seq: i64, hook: &str, epoch: &str, agent: &str| serde_json::json!({"v":1,"session_id":"s","seq":seq,"hook":hook,"epoch":epoch,"agent":agent});
        e.event(&j(1, "SessionStart", "A", "zz-a"), 7);
        for q in 2..=5 {
            e.event(&j(q, "PreToolUse", "A", "zz-a"), 7);
        }
        assert!(e.event(&j(3, "PostToolUse", "A", "zz-a"), 7).dropped, "stale within one epoch");
        // The module reloads: seq restarts at 1 under a new epoch, and is accepted.
        let o = e.event(&j(1, "UserPromptSubmit", "B", "zz-b"), 7);
        assert!(!o.dropped, "{}", o.note);
        assert_eq!(o.status, "Working");
        assert_eq!(e.get("s").unwrap().agent, "zz-b");
        assert!(e.event(&j(1, "Stop", "B", "zz-b"), 7).dropped);
    }

    #[test]
    fn a_new_epoch_never_rebinds_identity_from_a_subagent_event() {
        let mut e = Engine::default();
        let j = |seq: i64, hook: &str, epoch: &str, agent: &str| serde_json::json!({"v":1,"session_id":"s","seq":seq,"hook":hook,"epoch":epoch,"agent":agent});
        e.event(&j(1, "SessionStart", "A", "zz-lead"), 7);
        // After a reload the first event is a subagent's, carrying its own type.
        e.event(&j(1, "SubagentStart", "B", "general-purpose"), 7);
        assert_eq!(e.get("s").unwrap().agent, "zz-lead");
        e.event(&j(2, "SubagentStop", "B", "general-purpose"), 7);
        assert_eq!(e.get("s").unwrap().agent, "zz-lead");
    }

    #[test]
    fn stale_and_bad_bodies_never_panic() {
        // Invalid UTF-8 and a multi-byte char straddling the 200-byte log cut.
        assert!(parse_body(&[0xff, 0xfe, b'{']).is_err());
        // Valid JSON that isn't an object is not an event (main.rs decorates it as one).
        for body in ["[]", "1", "\"x\"", "null", "true"] {
            assert!(parse_body(body.as_bytes()).is_err(), "{body}");
        }
        assert!(parse_body(b"{}").is_ok());
        let mut body = "x".repeat(199).into_bytes();
        body.extend_from_slice("é{not json".as_bytes());
        assert!(parse_body(&body).is_err());
        assert_eq!(trunc("aé", 2), "a");
        assert_eq!(trunc("abc", 8), "abc");
        // A truncated (64 KiB cut) body mid-char.
        let big = format!("{{\"v\":1,\"message\":\"{}\"}}", "é".repeat(40_000));
        assert!(parse_body(&big.as_bytes()[..65_535]).is_err());
        // Stale seq is dropped.
        let mut e = Engine::default();
        e.event(&serde_json::json!({"v":1,"session_id":"s","hook":"SessionStart","agent":"a","seq":1}), 1);
        e.event(&serde_json::json!({"v":1,"session_id":"s","hook":"UserPromptSubmit","seq":3}), 1);
        assert!(e.event(&serde_json::json!({"v":1,"session_id":"s","hook":"Stop","seq":2}), 1).dropped);
        assert!(e.event(&serde_json::json!({"v":2,"session_id":"s","hook":"Stop","seq":4}), 1).dropped);
    }
}
