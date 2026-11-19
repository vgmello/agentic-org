//! Conformance checks for [`Session`] implementations (the `testkit` feature).
//!
//! The [`Session`] trait states three contract rules the office relies on. These checks
//! drive any session through them (and through the approval and steering rules), using the
//! adapter's own fixture lines (a [`Script`]), and panic with a message naming the broken
//! rule. [`check_plan`] checks a [`LaunchPlan`] for banned switches. Every adapter runs them
//! in its tests:
//!
//! ```ignore
//! let script = Script {
//!     opening: vec![...],
//!     turn: vec![...],
//!     turn_end: vec![...],
//!     approval: Some(ScriptApproval::new(request_line, id)),
//!     ..Default::default()
//! };
//! let caps = my_runtime.capabilities();
//! org_runtime::testkit::check_all(&mut || my_session(), &script, &caps, Instant::now());
//! org_runtime::testkit::check_plan(&plan, &spec, &ctx, &caps);
//! ```
//!
//! `make` builds a fresh session in its initial state each time it's called (e.g. from
//! `plan_session` on a fixture spec); the checks need several, some of them side by side.

use std::time::{Duration, Instant};

use crate::{
    ApprovalId, Capabilities, Control, ControlError, Decision, ExitInfo, LaunchContext, LaunchPlan,
    Output, Session, SessionEvent, SessionSpec, MAX_LINE_BYTES,
};

/// Runtime lines that walk a session through one turn: what the runtime would write to stdout,
/// each without its `\n`. Build it with `..Default::default()` so fields added later don't
/// break you.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Script {
    /// Fed right after `start`: they finish the opening, so the session emits
    /// [`SessionEvent::Ready`].
    pub opening: Vec<Vec<u8>>,
    /// Fed after the checks' normal [`Control::Send`]: they leave the turn running (e.g. a
    /// turn-started message). Empty if the send alone starts the turn.
    pub turn: Vec<Vec<u8>>,
    /// Fed while the turn runs: they end it.
    pub turn_end: Vec<Vec<u8>>,
    /// Lines the runtime may write to **stderr** (e.g. its error format). Rule 2 feeds them
    /// while idle, rule 3 before each exit (during a turn too), so they may end a turn but
    /// must never end the session. May be empty.
    pub stderr: Vec<Vec<u8>>,
    /// Lines that raise (and withdraw) an approval during the turn, for the approval checks.
    /// `None` for a runtime without an approval bridge: those checks are then skipped.
    pub approval: Option<ScriptApproval>,
}

/// The approval part of a [`Script`]. `#[non_exhaustive]` so it can grow: build it with
/// [`ScriptApproval::new`], then set `cancel`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ScriptApproval {
    /// Fed during the turn: the session emits [`SessionEvent::ApprovalRequested`] with `id`.
    pub request: Vec<u8>,
    /// The id that request carries.
    pub id: ApprovalId,
    /// Fed after `request`: the runtime withdraws it, and the session emits
    /// [`SessionEvent::ApprovalCancelled`]. `None` for a runtime that never withdraws one
    /// ([`Capabilities::runtime_cancels_approvals`] false): that check is then skipped.
    pub cancel: Option<Vec<u8>>,
}

impl ScriptApproval {
    /// The line that raises the approval, and the id it carries; no cancel line yet.
    pub fn new(request: Vec<u8>, id: ApprovalId) -> Self {
        ScriptApproval {
            request,
            id,
            cancel: None,
        }
    }
}

/// Runs every session check.
pub fn check_all(
    make: &mut dyn FnMut() -> Box<dyn Session>,
    script: &Script,
    caps: &Capabilities,
    now: Instant,
) {
    check_busy_during_turn(make, script, now);
    check_busy_while_approval_pending(make, script, now);
    check_error_changes_nothing(make, script, now);
    check_ended_once_and_last(make, script, now);
    check_answer_after_cancel(make, script, now);
    check_urgent_send(make, script, caps, now);
    check_usage_before_turn_end(make, script, now);
}

/// Rule 1 with an approval pending: a normal [`Control::Send`] still returns
/// [`ControlError::Busy`]. Skipped without [`Script::approval`].
pub fn check_busy_while_approval_pending(
    make: &mut dyn FnMut() -> Box<dyn Session>,
    script: &Script,
    now: Instant,
) {
    let Some(approval) = &script.approval else {
        return;
    };
    let mut session = make();
    open(session.as_mut(), script, now);
    into_turn(session.as_mut(), script, now);
    raise(session.as_mut(), approval, now);
    assert_eq!(
        session.control(send(false), now),
        Err(ControlError::Busy),
        "rule 1 broken: a normal Send while an approval is pending must return Busy"
    );
}

/// After the runtime withdraws an approval ([`SessionEvent::ApprovalCancelled`]), answering
/// it returns [`ControlError::UnknownApproval`]. Skipped without a cancel line.
pub fn check_answer_after_cancel(
    make: &mut dyn FnMut() -> Box<dyn Session>,
    script: &Script,
    now: Instant,
) {
    let Some(approval) = &script.approval else {
        return;
    };
    let Some(cancel) = &approval.cancel else {
        return;
    };
    let mut session = make();
    open(session.as_mut(), script, now);
    into_turn(session.as_mut(), script, now);
    raise(session.as_mut(), approval, now);
    let out = session.on_line(cancel, now);
    assert!(
        out.contains(&Output::Event(SessionEvent::ApprovalCancelled {
            id: approval.id.clone()
        })),
        "testkit: the script's cancel line didn't withdraw the approval: {out:?}"
    );
    assert_eq!(
        session.control(answer(&approval.id), now),
        Err(ControlError::UnknownApproval),
        "approval rule broken: an Answer after ApprovalCancelled must return UnknownApproval"
    );
}

/// An urgent [`Control::Send`] during a turn: accepted where the runtime can steer
/// ([`Capabilities::steer`]), [`ControlError::NeedsInterrupt`] where it can't.
pub fn check_urgent_send(
    make: &mut dyn FnMut() -> Box<dyn Session>,
    script: &Script,
    caps: &Capabilities,
    now: Instant,
) {
    let mut session = make();
    open(session.as_mut(), script, now);
    into_turn(session.as_mut(), script, now);
    let result = session.control(send(true), now);
    if caps.steer {
        assert!(
            result.is_ok(),
            "steering rule broken: an urgent Send must steer on a runtime that can: {result:?}"
        );
    } else {
        assert_eq!(
            result,
            Err(ControlError::NeedsInterrupt),
            "steering rule broken: an urgent Send must return NeedsInterrupt where the \
             runtime can't steer"
        );
    }
}

/// Values that, set anywhere (a flag's value, a config key, a protocol message), make a
/// runtime skip its permission checks or its sandbox: Claude's `bypassPermissions` mode,
/// Codex's `danger-full-access` sandbox, Gemini/Antigravity's `yolo` approval mode.
/// Compared case-insensitively, as whole values.
const BANNED_VALUES: &[&str] = &["bypasspermissions", "danger-full-access", "yolo"];

/// Flags that do it on their own (Gemini/Antigravity's `-y` / `--yolo`), as whole argv
/// tokens; and every `--dangerously-*` flag (Claude's `--dangerously-skip-permissions`,
/// Codex's `--dangerously-bypass-approvals-and-sandbox`, ...).
const BANNED_FLAGS: &[&str] = &["-y", "--yolo"];
const BANNED_FLAG_PREFIX: &str = "--dangerously-";

/// The banned switch one argv token (or env value) carries, if any:
/// - a banned flag (`-y`, `--yolo`, `--dangerously-*`);
/// - a banned value on its own or after the last `=`, with whitespace and quotes trimmed
///   (`bypassPermissions`, `--permission-mode=bypassPermissions`,
///   `sandbox_mode = "danger-full-access"`, `sandbox_mode = danger-full-access`);
/// - a structured value setting one (`{"permissions":{"defaultMode":"bypassPermissions"}}`,
///   as its own token or after `--settings=`), by [`banned_quoted`].
fn banned_token(token: &str) -> Option<String> {
    let lower = token.to_lowercase();
    if lower.starts_with(BANNED_FLAG_PREFIX) || BANNED_FLAGS.contains(&lower.as_str()) {
        return Some(lower);
    }
    let value = lower
        .rsplit_once('=')
        .map_or(lower.as_str(), |(_, value)| value);
    let value = value.trim_matches(|c: char| c == '"' || c == '\'' || c.is_whitespace());
    if BANNED_VALUES.contains(&value) {
        return Some(value.to_owned());
    }
    banned_quoted(token).map(str::to_owned)
}

/// The banned value a config text, protocol line or structured argument sets, if any: one as
/// a whole quoted string **in a key's value position**, right after a `:` or `=` (spaces
/// allowed): `"defaultMode":"bypassPermissions"`, `sandbox_mode = 'danger-full-access'`.
///
/// Not a match: a key that merely contains one (`"disableBypassPermissionsMode"`), a rule
/// that names a flag, or one **inside a list** (`{"disallowedModes":["bypassPermissions"]}`,
/// a hardening list that forbids it). The limit: a list that *allows* one isn't caught either;
/// an adapter that writes mode or sandbox lists checks them in its own tests.
fn banned_quoted(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    BANNED_VALUES.iter().copied().find(|value| {
        [format!("\"{value}\""), format!("'{value}'")]
            .iter()
            .any(|quoted| {
                lower.match_indices(quoted.as_str()).any(|(at, _)| {
                    let before = lower[..at].trim_end();
                    before.ends_with(':') || before.ends_with('=')
                })
            })
    })
}

/// Checks a session's [`LaunchPlan`] against the contract:
/// - no banned switch: no argv token is a `--dangerously-*` flag, `-y` or `--yolo`, or sets
///   a bypass value (`bypassPermissions`, `danger-full-access`, `yolo`, on its own or after
///   `=`); no `env_add` value sets one; no generated file sets one as a quoted string (the
///   instructions file is left out: its text is the office's own). Compared
///   case-insensitively, as whole tokens and values, so a hardening key like
///   `disableBypassPermissionsMode` or a deny rule naming a flag isn't refused. (The
///   session's own protocol writes are checked by the session checks, for runtimes that set
///   their mode in-band.)
/// - its `cwd` is the spec's workspace, and neither the run's `TMPDIR` nor its runtime dir is
///   `/`;
/// - where the runtime enforces writable roots, the sandbox is on, and the plan carries the
///   writable roots at all, the run's `TMPDIR` ([`LaunchContext::tmpdir`]) is one of them:
///   it appears as a whole argv token, after `=` in one, or quoted (`"<tmpdir>"`) in an
///   argument (e.g. a list, `writable_roots=["…"]`) or a generated file. "Carries the
///   writable roots" means some writable root of the spec appears the same way. A runtime
///   that gets them **in-band** instead (Codex's `thread/start`) has a plan with none, so this
///   part is skipped, and the adapter checks `TMPDIR` in its own tests. (No check here can
///   tell a writable root from a read-only one naming the same path; the adapter's own tests
///   check which flag carries it.)
pub fn check_plan(plan: &LaunchPlan, spec: &SessionSpec, ctx: &LaunchContext, caps: &Capabilities) {
    for arg in &plan.args {
        if arg == spec.agent.name.as_str() {
            continue; // an agent may be called anything its rule allows
        }
        if let Some(banned) = banned_token(arg) {
            panic!("plan rule broken: an argument carries a banned switch ({banned}): {arg:?}");
        }
    }
    for (name, value) in &plan.env_add {
        if let Some(banned) = banned_token(value) {
            panic!(
                "plan rule broken: env_add {} carries a banned value ({banned})",
                name.as_str()
            );
        }
    }
    let instructions = spec.instructions.as_bytes();
    let files: Vec<(&str, String)> = plan
        .files
        .iter()
        .filter(|file| file.contents != instructions)
        .map(|file| {
            (
                file.rel_path.as_str(),
                String::from_utf8_lossy(&file.contents).into_owned(),
            )
        })
        .collect();
    for (name, text) in &files {
        if let Some(banned) = banned_quoted(text) {
            panic!("plan rule broken: the file {name} sets a banned value ({banned})");
        }
    }
    assert_eq!(
        plan.cwd, spec.workspace,
        "plan rule broken: the cwd must be the spec's workspace"
    );
    assert!(
        !ctx.tmpdir.is_root() && !ctx.runtime_dir.is_root(),
        "plan rule broken: the run's TMPDIR or runtime dir is `/`"
    );
    // Whether the plan names `path` the way a root list would.
    let mentions = |path: &str| {
        let quoted = format!("\"{path}\"");
        plan.args.iter().any(|arg| {
            arg == path
                || arg.contains(&quoted)
                || arg.rsplit_once('=').is_some_and(|(_, value)| {
                    value.trim_matches(|c: char| c == '"' || c == '\'' || c.is_whitespace()) == path
                })
        }) || files.iter().any(|(_, text)| text.contains(&quoted))
    };
    let carries_roots = spec
        .sandbox
        .writable_roots
        .iter()
        .any(|root| mentions(root.as_str()));
    if spec.sandbox.enabled && caps.enforces_writable_roots && carries_roots {
        let tmpdir = ctx.tmpdir.as_str();
        assert!(
            mentions(tmpdir),
            "plan rule broken: the run's TMPDIR ({tmpdir}) isn't a writable root in the plan"
        );
    }
}

/// Checks the lines a session writes: none may set a bypass value (for runtimes that set their
/// mode or sandbox in-band, like Codex's app-server or an ACP mode switch).
fn check_writes(out: &[Output], what: &str) {
    for output in out {
        if let Output::Write(line) = output {
            let text = String::from_utf8_lossy(line.as_bytes());
            if let Some(banned) = banned_quoted(&text) {
                panic!("bypass rule broken: {what} writes a banned value ({banned}): {text}");
            }
        }
    }
}

/// Usage comes before `TurnEnded`: no [`SessionEvent::Usage`] after the turn's
/// [`SessionEvent::TurnEnded`].
pub fn check_usage_before_turn_end(
    make: &mut dyn FnMut() -> Box<dyn Session>,
    script: &Script,
    now: Instant,
) {
    let mut session = make();
    open(session.as_mut(), script, now);
    let out = [
        into_turn(session.as_mut(), script, now),
        end_turn(session.as_mut(), script, now),
    ]
    .concat();
    if let Some(end) = out
        .iter()
        .position(|o| matches!(o, Output::Event(SessionEvent::TurnEnded { .. })))
    {
        assert!(
            !out[end..]
                .iter()
                .any(|o| matches!(o, Output::Event(SessionEvent::Usage(_)))),
            "usage rule broken: Usage came after the turn's TurnEnded: {out:?}"
        );
    }
}

/// Rule 1: a normal [`Control::Send`] during a turn returns [`ControlError::Busy`], and is
/// accepted again once the turn ends.
pub fn check_busy_during_turn(
    make: &mut dyn FnMut() -> Box<dyn Session>,
    script: &Script,
    now: Instant,
) {
    let mut session = make();
    open(session.as_mut(), script, now);
    into_turn(session.as_mut(), script, now);
    assert_eq!(
        session.control(send(false), now),
        Err(ControlError::Busy),
        "rule 1 broken: a normal Send during a turn must return Busy"
    );
    end_turn(session.as_mut(), script, now);
    assert!(
        session.control(send(false), now).is_ok(),
        "testkit: after the script's turn_end lines, a normal Send was still refused (the \
         script doesn't end the turn, or rule 2 is broken: the refused Send was queued)"
    );
}

/// Rule 2: a [`Session::control`] that returns `Err` changes nothing.
///
/// It runs two identical sessions side by side, gives only one of them each control that
/// fails, then feeds both the same input to the end. Their outputs and deadlines must stay
/// identical at every step, which they can't if the failed control changed anything they
/// act on later. It's a black-box check: a change that never shows on the script's inputs
/// (say, a stray entry in a table only an unknown id would reach) goes unseen.
pub fn check_error_changes_nothing(
    make: &mut dyn FnMut() -> Box<dyn Session>,
    script: &Script,
    now: Instant,
) {
    let mut pair = Pair {
        a: make(),
        b: make(),
        now,
    };
    pair.both("start", &|s| s.start(now));
    pair.try_on_a("a Send while opening", send(false), false);
    pair.try_on_a("an Answer while opening", answer_never_raised(), true);
    for line in &script.opening {
        pair.both("an opening line", &|s| s.on_line(line, now));
    }
    pair.try_on_a(
        "an Answer for an approval never raised",
        answer_never_raised(),
        true,
    );
    for line in &script.stderr {
        pair.both("a stderr line", &|s| s.on_stderr_line(line, now));
    }
    pair.both("a Send while idle", &|s| {
        s.control(send(false), now)
            .unwrap_or_else(|e| panic!("testkit: a normal Send while idle was refused: {e}"))
    });
    for line in &script.turn {
        pair.both("a turn line", &|s| s.on_line(line, now));
    }
    if let Some(approval) = &script.approval {
        pair.both("an approval request", &|s| {
            s.on_line(&approval.request, now)
        });
        pair.try_on_a(
            "a normal Send while an approval is pending",
            send(false),
            true,
        );
        pair.try_on_a(
            "an Answer to the wrong approval",
            answer_never_raised(),
            true,
        );
        // The refused Answer changed nothing: the real one is accepted alike by both.
        pair.both_result("the Answer to the pending approval", &|s| {
            s.control(answer(&approval.id), now)
        });
    }
    pair.try_on_a("a normal Send during the turn", send(false), true);
    pair.try_on_a(
        "an Answer never raised, during the turn",
        answer_never_raised(),
        true,
    );
    pair.try_on_a("an urgent Send during the turn", send(true), false);
    for line in &script.turn_end {
        pair.both("a turn_end line", &|s| s.on_line(line, now));
    }
    // A refused Send must not have been queued: this one writes the same as on the twin.
    pair.both("a Send after the turn", &|s| {
        s.control(send(false), now).unwrap_or_else(|e| {
            panic!("testkit: after the script's turn_end lines, a normal Send was refused: {e}")
        })
    });
    let later = now + Duration::from_secs(3600);
    let latest = later + Duration::from_secs(3600);
    pair.both("a late tick", &|s| s.on_tick(later));
    pair.both("stdout closing", &|s| s.on_stdout_closed(later));
    pair.both("a tick after stdout closed", &|s| s.on_tick(latest));
    pair.both("the exit", &|s| s.on_exit(EXIT, latest));
}

/// Rule 3: [`SessionEvent::Ended`] comes only from [`Session::on_exit`], exactly once, as its
/// last output, carrying the exit; after it every call is silent, there's no deadline, and
/// every control returns [`ControlError::Closed`].
///
/// It checks exits at every stage: right after the start, while idle, during a turn, after
/// [`Control::Close`], each with stdout closed first and (where it matters) still open, and
/// each after a tick far past every deadline, the script's stderr lines and a truncated line,
/// and (once stdout closed) another tick far past that, none of which may end the session.
/// Each exit carries a different [`ExitInfo`], which `Ended` must carry back.
pub fn check_ended_once_and_last(
    make: &mut dyn FnMut() -> Box<dyn Session>,
    script: &Script,
    now: Instant,
) {
    type Lead = fn(&mut dyn Session, &Script, Instant) -> Vec<Output>;
    let scenarios: [(&str, Lead, bool); 8] = [
        (
            "an exit with an approval pending (or during a turn, without approval lines)",
            |s, script, now| {
                let mut out = [open(s, script, now), into_turn(s, script, now)].concat();
                if let Some(approval) = &script.approval {
                    out.extend(raise(s, approval, now));
                }
                out
            },
            true,
        ),
        (
            "an exit right after the start",
            |s, _, now| s.start(now),
            true,
        ),
        (
            "an exit right after the start, stdout still open",
            |s, _, now| s.start(now),
            false,
        ),
        ("an exit while idle", open, true),
        (
            "an exit during a turn",
            |s, script, now| [open(s, script, now), into_turn(s, script, now)].concat(),
            true,
        ),
        (
            "an exit during a turn, stdout still open",
            |s, script, now| [open(s, script, now), into_turn(s, script, now)].concat(),
            false,
        ),
        (
            "an exit after the turn",
            |s, script, now| {
                [
                    open(s, script, now),
                    into_turn(s, script, now),
                    end_turn(s, script, now),
                ]
                .concat()
            },
            true,
        ),
        (
            "an exit after Close",
            |s, script, now| {
                let mut out = open(s, script, now);
                out.extend(
                    s.control(Control::Close, now)
                        .unwrap_or_else(|e| panic!("testkit: Close was refused: {e}")),
                );
                out
            },
            true,
        ),
    ];
    let late = now + Duration::from_secs(24 * 3600);
    let after_eof = late + Duration::from_secs(24 * 3600);
    for (i, (name, lead, stdout_closes_first)) in scenarios.into_iter().enumerate() {
        let exit = varied_exit(i);
        let mut session = make();
        let mut before = lead(session.as_mut(), script, now);
        // Things that happen before an exit and must not end the session themselves.
        for line in &script.stderr {
            before.extend(session.on_stderr_line(line, now));
        }
        before.extend(session.on_truncated(MAX_LINE_BYTES + 1, now));
        before.extend(session.on_tick(late));
        if stdout_closes_first {
            before.extend(session.on_stdout_closed(late));
            // A session must not end itself some time after EOF: only the exit ends it.
            before.extend(session.on_tick(after_eof));
        }
        assert!(
            !before.iter().any(is_ended),
            "rule 3 broken ({name}): Ended came before on_exit: {before:?}"
        );
        let out = session.on_exit(exit, after_eof);
        assert_eq!(
            out.iter().filter(|o| is_ended(o)).count(),
            1,
            "rule 3 broken ({name}): on_exit must emit Ended exactly once: {out:?}"
        );
        assert_eq!(
            out.last(),
            Some(&Output::Event(SessionEvent::Ended { exit })),
            "rule 3 broken ({name}): Ended, with the exit given, must be on_exit's last output"
        );
        assert_finished(session.as_mut(), script, name, after_eof);
    }
}

/// After Ended: every call is silent, no deadline, every control is refused as Closed.
fn assert_finished(session: &mut dyn Session, script: &Script, name: &str, now: Instant) {
    let later = now + Duration::from_secs(3600);
    let mut calls = vec![
        ("on_exit again", session.on_exit(varied_exit(1), later)),
        ("on_stdout_closed", session.on_stdout_closed(later)),
        ("on_truncated", session.on_truncated(10, later)),
        ("on_stderr_line", session.on_stderr_line(b"error", later)),
        ("on_tick", session.on_tick(later)),
        ("on_line (not JSON)", session.on_line(b"not json", later)),
    ];
    let lines = script
        .opening
        .iter()
        .chain(&script.turn)
        .chain(&script.turn_end);
    for line in lines {
        calls.push(("on_line (a script line)", session.on_line(line, later)));
    }
    if let Some(approval) = &script.approval {
        calls.push((
            "on_line (the approval request)",
            session.on_line(&approval.request, later),
        ));
        if let Some(cancel) = &approval.cancel {
            calls.push((
                "on_line (the approval cancel)",
                session.on_line(cancel, later),
            ));
        }
    }
    for line in &script.stderr {
        calls.push((
            "on_stderr_line (a script line)",
            session.on_stderr_line(line, later),
        ));
    }
    for (what, out) in calls {
        assert!(
            out.is_empty(),
            "rule 3 broken ({name}): {what} after Ended emitted {out:?}"
        );
    }
    assert_eq!(
        session.next_deadline(),
        None,
        "rule 3 broken ({name}): a deadline after Ended"
    );
    for control in [
        send(false),
        send(true),
        Control::Interrupt,
        Control::Close,
        answer_never_raised(),
    ] {
        assert_eq!(
            session.control(control.clone(), later),
            Err(ControlError::Closed),
            "rule 3 broken ({name}): {control:?} after Ended must return Closed"
        );
    }
}

const EXIT: ExitInfo = ExitInfo {
    code: Some(0),
    signal: None,
};

/// A different exit per scenario, so a session can't pass by always reporting the same one.
fn varied_exit(i: usize) -> ExitInfo {
    match i % 4 {
        0 => EXIT,
        1 => ExitInfo {
            code: Some(3),
            signal: None,
        },
        2 => ExitInfo {
            code: None,
            signal: Some(15),
        },
        _ => ExitInfo {
            code: None,
            signal: None,
        },
    }
}

fn send(urgent: bool) -> Control {
    Control::Send {
        text: "org-runtime testkit".to_owned(),
        urgent,
    }
}

fn answer(id: &ApprovalId) -> Control {
    Control::Answer {
        request_id: id.clone(),
        decision: Decision::AllowOnce,
    }
}

/// Feeds the approval request; the session must raise it with the script's id.
fn raise(session: &mut dyn Session, approval: &ScriptApproval, now: Instant) -> Vec<Output> {
    let out = session.on_line(&approval.request, now);
    check_writes(&out, "the approval request");
    let request = out.iter().find_map(|o| match o {
        Output::Event(SessionEvent::ApprovalRequested(request)) if request.id == approval.id => {
            Some(request)
        }
        _ => None,
    });
    let Some(request) = request else {
        panic!(
            "testkit: the script's approval line didn't raise approval {:?}: {out:?}",
            approval.id
        )
    };
    assert_eq!(
        request.escalation,
        request.effective_escalation(),
        "escalation rule broken: a {:?} request must not carry {:?}",
        request.kind,
        request.escalation
    );
    out
}

fn answer_never_raised() -> Control {
    Control::Answer {
        request_id: ApprovalId::new("org-runtime-testkit-never-raised"),
        decision: Decision::AllowOnce,
    }
}

fn is_ended(output: &Output) -> bool {
    matches!(output, Output::Event(SessionEvent::Ended { .. }))
}

/// Starts the session and feeds the opening; it must emit Ready.
fn open(session: &mut dyn Session, script: &Script, now: Instant) -> Vec<Output> {
    let mut out = session.start(now);
    for line in &script.opening {
        out.extend(session.on_line(line, now));
    }
    check_writes(&out, "the opening");
    assert!(
        out.iter()
            .any(|o| matches!(o, Output::Event(SessionEvent::Ready { .. }))),
        "testkit: the script's opening lines didn't make the session Ready: {out:?}"
    );
    out
}

/// Sends a normal message while idle and feeds the turn lines.
fn into_turn(session: &mut dyn Session, script: &Script, now: Instant) -> Vec<Output> {
    let mut out = session
        .control(send(false), now)
        .unwrap_or_else(|e| panic!("testkit: a normal Send while idle was refused: {e}"));
    for line in &script.turn {
        out.extend(session.on_line(line, now));
    }
    check_writes(&out, "a Send");
    out
}

fn end_turn(session: &mut dyn Session, script: &Script, now: Instant) -> Vec<Output> {
    let mut out = Vec::new();
    for line in &script.turn_end {
        out.extend(session.on_line(line, now));
    }
    check_writes(&out, "the end of a turn");
    out
}

/// A control applied to one session of a [`Pair`].
type ControlStep<'a> = dyn Fn(&mut dyn Session) -> Result<Vec<Output>, ControlError> + 'a;

/// Two sessions in lockstep, for rule 2.
struct Pair {
    a: Box<dyn Session>,
    b: Box<dyn Session>,
    now: Instant,
}

impl Pair {
    /// The same step on both; they must answer alike.
    fn both(&mut self, what: &str, step: &dyn Fn(&mut dyn Session) -> Vec<Output>) {
        let a = step(self.a.as_mut());
        let b = step(self.b.as_mut());
        assert_eq!(
            a, b,
            "rule 2 broken: after a refused control, the session answered {what} differently"
        );
        self.same_deadline(what);
    }

    /// The same control on both; their results (success or error) must be alike.
    fn both_result(&mut self, what: &str, step: &ControlStep) {
        let a = step(self.a.as_mut());
        let b = step(self.b.as_mut());
        assert_eq!(
            a, b,
            "rule 2 broken: after a refused control, the session answered {what} differently"
        );
        self.same_deadline(what);
    }

    /// `control` on `a` alone if it fails there (the point of the check); if it succeeds, on
    /// `b` too, so they stay twins. `must_fail`: succeeding is itself a bug.
    fn try_on_a(&mut self, what: &str, control: Control, must_fail: bool) {
        match self.a.control(control.clone(), self.now) {
            Err(_) => self.same_deadline(what),
            Ok(out) => {
                let twin = self.b.control(control, self.now);
                // If the twin (which never saw the earlier refused controls) refuses it, an
                // earlier refusal changed `a`: that's rule 2, not a script problem.
                assert!(
                    !(must_fail && twin.is_err()),
                    "rule 2 broken: {what} was accepted after earlier refused controls, but \
                     refused by the twin that never saw them"
                );
                assert!(
                    !must_fail,
                    "testkit: {what} must be refused, but was accepted: {out:?}"
                );
                assert_eq!(
                    Ok(out),
                    twin,
                    "rule 2 broken (or the session isn't deterministic): {what} was handled \
                     differently by the twin that never saw the earlier refused controls"
                );
                self.same_deadline(what);
            }
        }
    }

    fn same_deadline(&self, what: &str) {
        assert_eq!(
            self.a.next_deadline(),
            self.b.next_deadline(),
            "rule 2 broken: the deadline differs after {what}"
        );
    }
}
