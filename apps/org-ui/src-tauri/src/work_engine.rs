//! The mock office's work-item rules for tasks and checklists (ada's W7 design §1.10, §2, §4.4, §4.7), the Rust twin of
//! `src/lib/work/engine.ts`: both run the cases in `crates/org-work/test-vectors/work-commands.json`. It stands in for the office until
//! `org_proto::work` exists. Calls and answers are JSON in the contract's shapes; errors carry the contract's codes.
//! Bugs, requests, links and comments are not here.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// A refusal with the contract's error code as text (`checklist_incomplete` is ada's W7 code, not yet in `org-proto`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkError {
    pub code: &'static str,
    pub message: String,
}

type ApiError = WorkError;

const TITLE_MAX: usize = 120;
const DESCRIPTION_MAX: usize = 20_000;
const DOD_MAX: usize = 1_000;
const REASON_MAX: usize = 1_000;
const BLOCKED_MAX: usize = 200;
const STEP_MAX: usize = 200;
const MAX_STEPS: usize = 100;
const ANSWERS_KEPT: usize = 1000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub text: String,
    pub required: bool,
    pub done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<String>,
    pub version: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixLink {
    pub id: String,
    /// The bug this link's task fixes.
    pub to: String,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    /// The `WorkItemSummary` of the contract, as JSON.
    pub summary: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_of_done: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_anyway: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub what_happened: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_at: Option<String>,
    pub checklist: Vec<Step>,
    pub watchers: Vec<String>,
    /// The links this item makes (a fix task to the bug it fixes).
    #[serde(default)]
    pub links: Vec<FixLink>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WorkState {
    pub incarnation: u64,
    pub seq: u64,
    pub next: BTreeMap<String, u64>,
    pub items: BTreeMap<String, Task>,
    /// change_id -> (the call as text, the answer)
    pub answers: BTreeMap<String, (String, Value)>,
    /// The change ids in the order they were answered, oldest first.
    pub order: Vec<String>,
}

pub struct Project {
    pub id: String,
    pub archived: bool,
    pub team_ids: Vec<String>,
    /// The area ids of the project (General first); empty while the mock has none.
    pub areas: Vec<String>,
    pub general_area: Option<String>,
}

/// What the engine needs to know about the world around the items.
pub struct Ctx {
    pub now: String,
    pub actor: String,
    pub projects: Vec<Project>,
    pub members: Vec<String>,
}

fn err(code: &'static str, message: impl Into<String>) -> ApiError {
    WorkError {
        code,
        message: message.into(),
    }
}
fn invalid<T>(message: impl Into<String>) -> Result<T, ApiError> {
    Err(err("invalid_request", message))
}

fn str_of<'a>(call: &'a Value, key: &str) -> Option<&'a str> {
    call.get(key).and_then(Value::as_str)
}
fn flag(call: &Value, key: &str) -> bool {
    call.get(key).and_then(Value::as_bool).unwrap_or(false)
}
fn has(call: &Value, key: &str) -> bool {
    call.get(key).is_some_and(|v| !v.is_null())
}

fn one_line(text: Option<&str>, max: usize, what: &str) -> Result<String, ApiError> {
    let t = text.unwrap_or("").trim();
    if t.is_empty() || t.contains(['\r', '\n']) {
        return invalid(format!("{what} must be one line of text."));
    }
    if t.chars().count() > max {
        return invalid(format!("{what} can be at most {max} characters."));
    }
    Ok(t.to_string())
}
fn long_text(text: &str, max: usize, what: &str) -> Result<String, ApiError> {
    if text.chars().count() > max {
        return invalid(format!("{what} can be at most {max} characters."));
    }
    Ok(text.to_string())
}

fn counts(steps: &[Step]) -> Value {
    json!({
        "total": steps.len(),
        "done": steps.iter().filter(|s| s.done).count(),
        "required_left": steps.iter().filter(|s| s.required && !s.done).count(),
    })
}

/// The item `id` if it is a live item of `kind` (`task` or `bug`).
fn kind_of<'a>(state: &'a mut WorkState, id: &str, kind: &str) -> Result<&'a mut Task, ApiError> {
    let task = state
        .items
        .get_mut(id)
        .filter(|t| t.summary["deleted"] != json!(true))
        .ok_or_else(|| err("not_found", "That item is not here."))?;
    if task.summary["kind"] != json!(kind) {
        return invalid(format!(
            "That is a {}, not a {kind}.",
            task.summary["kind"].as_str().unwrap_or("item")
        ));
    }
    Ok(task)
}

const SEVERITIES: [&str; 4] = ["critical", "major", "minor", "trivial"];

fn bug_text(text: Option<&str>, what: &str, required: bool) -> Result<Option<String>, ApiError> {
    let t = text.unwrap_or("").trim();
    if required && t.is_empty() {
        return invalid(format!("{what} can't be empty."));
    }
    if t.is_empty() {
        return Ok(None);
    }
    long_text(t, 10_000, what).map(Some)
}

fn step_owner(state: &WorkState, step_id: &str) -> Result<String, ApiError> {
    state
        .items
        .iter()
        .find(|(_, t)| {
            t.summary["deleted"] != json!(true) && t.checklist.iter().any(|s| s.id == step_id)
        })
        .map(|(id, _)| id.clone())
        .ok_or_else(|| err("not_found", "That checklist step is not here."))
}
fn expect_version(task: &Task, version: Option<u64>) -> Result<(), ApiError> {
    if task.summary["version"].as_u64() != version {
        return Err(err(
            "conflict",
            "This changed since you opened it. Reload to see the latest.",
        ));
    }
    Ok(())
}
fn touch(state_seq: &mut u64, task: &mut Task, now: &str) {
    let v = task.summary["version"].as_u64().unwrap_or(0) + 1;
    task.summary["version"] = json!(v);
    task.summary["updated_at"] = json!(now);
    task.summary["checklist"] = counts(&task.checklist);
    *state_seq += 1;
}

fn new_steps(list: &Value, existing: &[Step]) -> Result<Vec<Step>, ApiError> {
    let list = list.as_array().map(Vec::as_slice).unwrap_or(&[]);
    if list.is_empty() || existing.len() + list.len() > MAX_STEPS {
        return invalid(format!("A checklist can have at most {MAX_STEPS} steps."));
    }
    let mut seen: Vec<String> = existing.iter().map(|s| s.id.clone()).collect();
    let mut out = Vec::new();
    for s in list {
        let id = str_of(s, "step_id").unwrap_or("").to_string();
        if seen.contains(&id) {
            return Err(err("already_exists", "That step is already there."));
        }
        seen.push(id.clone());
        out.push(Step {
            id,
            text: one_line(str_of(s, "text"), STEP_MAX, "A step")?,
            required: flag(s, "required"),
            done: false,
            task_id: None,
            checked_by: None,
            checked_at: None,
            version: 1,
        });
    }
    Ok(out)
}

fn method_of(call: &Value) -> &str {
    str_of(call, "method").unwrap_or("")
}

fn short_id(id: &str) -> String {
    let chars: Vec<char> = id.chars().collect();
    chars[chars.len().saturating_sub(6)..]
        .iter()
        .collect::<String>()
        .to_lowercase()
}

/// Add the link `task_id` fixes `bug_id` (design §1.7): same project, a live bug, no repeats.
fn add_fix_link(
    next: &mut WorkState,
    task_id: &str,
    link_id: &str,
    bug_id: &str,
    now: &str,
    actor: &str,
    changed: &mut Vec<String>,
) -> Result<(), ApiError> {
    let bug_project = match next.items.get(bug_id) {
        Some(b) if b.summary["deleted"] != json!(true) && b.summary["kind"] == json!("bug") => {
            b.summary["project_id"].clone()
        }
        _ => return Err(err("not_found", "That bug is not here.")),
    };
    if link_id.is_empty() || link_id.len() > 128 {
        return invalid("link_id must be a text of 1 to 128 characters.");
    }
    if next
        .items
        .values()
        .any(|i| i.links.iter().any(|l| l.id == link_id))
    {
        return Err(err("already_exists", "That link already exists."));
    }
    let task = next.items.get_mut(task_id).expect("checked by the caller");
    if task.summary["project_id"] != bug_project {
        return invalid("A fix task and its bug must be in the same project.");
    }
    if task.summary["fixes"]
        .as_array()
        .is_some_and(|f| f.contains(&json!(bug_id)))
    {
        return Err(err("already_exists", "That task already fixes that bug."));
    }
    task.links.push(FixLink {
        id: link_id.to_string(),
        to: bug_id.to_string(),
        created_by: actor.to_string(),
        created_at: now.to_string(),
    });
    task.summary["fixes"]
        .as_array_mut()
        .expect("fixes is a list")
        .push(json!(bug_id));
    let bug = next.items.get_mut(bug_id).expect("checked above");
    bug.summary["fixed_by"]
        .as_array_mut()
        .expect("fixed_by is a list")
        .push(json!(task_id));
    bug.summary["updated_at"] = json!(now);
    if !changed.iter().any(|c| c == bug_id) {
        changed.push(bug_id.to_string());
    }
    Ok(())
}

/// What just happened to a fix task or its bug: the rules of design §2.4 fire on these edges, never on a scan.
enum FixTrigger<'a> {
    Task { from: &'a str, to: &'a str },
    Linked { task_status: &'a str },
    Triaged,
}

/// A bug follows its fix tasks (design §2.4). While Triaged it goes In progress when a fix task goes In progress (or an
/// In progress task is linked, or the bug is triaged while one is In progress); it goes Fixed when a fix task goes Done
/// (or the bug is triaged) and every fix task is Done, at least one Completed, and no required step is left; it goes
/// back to In progress when a fix task goes back from Done while the bug is Fixed. Cancelled-only fix tasks and
/// unticked required steps change nothing, and neither does linking to a Fixed bug or unlinking.
fn follow_fix_tasks(
    next: &mut WorkState,
    bug_id: &str,
    now: &str,
    changed: &mut Vec<String>,
    trigger: FixTrigger,
) {
    let Some(bug) = next
        .items
        .get(bug_id)
        .filter(|b| b.summary["deleted"] != json!(true) && b.summary["kind"] == json!("bug"))
    else {
        return;
    };
    let fix_tasks: Vec<&Task> = bug.summary["fixed_by"]
        .as_array()
        .map(|ids| {
            ids.iter()
                .filter_map(|id| id.as_str())
                .filter_map(|id| next.items.get(id))
                .filter(|t| t.summary["deleted"] != json!(true))
                .collect()
        })
        .unwrap_or_default();
    let status = bug.summary["status"].as_str().unwrap_or("").to_string();
    let all_done = !fix_tasks.is_empty()
        && fix_tasks
            .iter()
            .all(|t| t.summary["status"] == json!("done"));
    let any_completed = fix_tasks
        .iter()
        .any(|t| t.summary["resolution"] == json!("completed"));
    let started = match &trigger {
        FixTrigger::Task { to, .. } => *to == "in_progress",
        FixTrigger::Linked { task_status } => *task_status == "in_progress",
        FixTrigger::Triaged => fix_tasks
            .iter()
            .any(|t| t.summary["status"] == json!("in_progress")),
    };
    let finished = matches!(
        &trigger,
        FixTrigger::Task { to: "done", .. } | FixTrigger::Triaged
    );
    let mut to: Option<&str> = None;
    if status == "triaged" && started {
        to = Some("in_progress");
    }
    if finished
        && (status == "triaged" || status == "in_progress")
        && all_done
        && any_completed
        && !bug.checklist.iter().any(|c| c.required && !c.done)
    {
        to = Some("fixed");
    }
    if status == "fixed"
        && matches!(&trigger, FixTrigger::Task { from: "done", to } if *to != "done")
    {
        to = Some("in_progress");
    }
    let Some(to) = to else { return };
    let seq = &mut next.seq;
    let bug = next.items.get_mut(bug_id).expect("found above");
    bug.summary["status"] = json!(to);
    touch(seq, bug, now);
    if !changed.iter().any(|c| c == bug_id) {
        changed.push(bug_id.to_string());
    }
}

/// Apply one call (`{ "method": "task.create", "change_id": …, … }`). Returns the answer (`WorkAccepted`); the state
/// changes only when the call is accepted.
pub fn apply(state: &mut WorkState, call: &Value, ctx: &Ctx) -> Result<Value, ApiError> {
    let change_id = str_of(call, "change_id").unwrap_or("").to_string();
    let call_text = call.to_string();
    if let Some((kept, answer)) = state.answers.get(&change_id) {
        if *kept != call_text {
            return invalid("That change id was used for a different change.");
        }
        return Ok(answer.clone());
    }
    let mut next = state.clone();
    let kind_needed = if matches!(
        method_of(call),
        "task.update" | "task.assign" | "task.set_status"
    ) {
        Some("task")
    } else if method_of(call).starts_with("bug.") && method_of(call) != "bug.report" {
        Some("bug")
    } else {
        None
    };
    if let Some(kind) = kind_needed {
        kind_of(&mut next, str_of(call, "item_id").unwrap_or(""), kind)?;
    }
    let mut changed: Vec<String> = Vec::new();
    let now = ctx.now.as_str();
    let method = str_of(call, "method").unwrap_or("");
    let item_id = str_of(call, "item_id").unwrap_or("").to_string();
    let version = call.get("expected_version").and_then(Value::as_u64);
    for (what, value) in [
        ("change_id", change_id.as_str()),
        ("item_id", item_id.as_str()),
        ("step_id", str_of(call, "step_id").unwrap_or("")),
    ] {
        let needed = what == "change_id"
            || (what == "item_id"
                && (method.starts_with("task.")
                    || method.starts_with("bug.")
                    || matches!(method, "checklist.add" | "work.delete")))
            || (what == "step_id"
                && matches!(
                    method,
                    "checklist.check" | "checklist.edit" | "checklist.remove"
                ));
        if needed && (value.is_empty() || value.len() > 128) {
            return invalid(format!("{what} must be a text of 1 to 128 characters."));
        }
    }
    if let Some(p) = call.get("priority").filter(|v| !v.is_null()) {
        if !p
            .as_str()
            .is_some_and(|p| ["urgent", "high", "normal", "low"].contains(&p))
        {
            return invalid("The priority must be urgent, high, normal or low.");
        }
    }
    if let Some(r) = call
        .get("resolution")
        .filter(|v| !v.is_null())
        .filter(|_| method == "task.set_status")
    {
        if !r
            .as_str()
            .is_some_and(|r| ["completed", "cancelled"].contains(&r))
        {
            return invalid("A task resolution is completed or cancelled.");
        }
    }
    if let Some(d) = call.get("done") {
        if !d.is_boolean() {
            return invalid("done must be true or false.");
        }
    }

    match method {
        "task.create" => {
            let project_id = str_of(call, "project_id").unwrap_or("");
            let project = ctx
                .projects
                .iter()
                .find(|p| p.id == project_id)
                .ok_or_else(|| err("not_found", "That project is not here."))?;
            if project.archived {
                return invalid("That project is archived.");
            }
            if next.items.contains_key(&item_id) {
                return Err(err("already_exists", "That item already exists."));
            }
            let title = one_line(str_of(call, "title"), TITLE_MAX, "The title")?;
            if let Some(team) = str_of(call, "team_id") {
                if !project.team_ids.iter().any(|t| t == team) {
                    return invalid("That team isn't one of the project's teams.");
                }
            }
            if let Some(a) = str_of(call, "assignee_id") {
                if !ctx.members.iter().any(|m| m == a) {
                    return Err(err("unknown_recipient", "That member cannot be assigned."));
                }
            }
            let number = next.next.get(project_id).copied().unwrap_or(0) + 1;
            next.next.insert(project_id.to_string(), number);
            let checklist = match call.get("checklist") {
                Some(list) if list.as_array().is_some_and(|l| !l.is_empty()) => {
                    new_steps(list, &[])?
                }
                _ => Vec::new(),
            };
            let mut summary = json!({
                "id": item_id, "kind": "task", "project_id": project_id, "number": number,
                "short_id": short_id(&item_id), "title": title, "status": "todo",
                "priority": str_of(call, "priority").unwrap_or("normal"),
                "created_by": ctx.actor, "created_at": now, "updated_at": now,
                "severity_confirmed": false, "reopen_count": 0, "fixes": [], "fixed_by": [],
                "checklist": counts(&checklist), "done_anyway": false,
                "owner_office_id": "hq", "generation": 1, "version": 1, "deleted": false,
            });
            if let Some(a) = str_of(call, "assignee_id") {
                summary["assignee_id"] = json!(a);
            }
            if let Some(t) = str_of(call, "team_id") {
                summary["team_id"] = json!(t);
            }
            let task = Task {
                summary,
                description: match str_of(call, "description") {
                    Some(d) if !d.is_empty() => {
                        Some(long_text(d, DESCRIPTION_MAX, "The description")?)
                    }
                    _ => None,
                },
                definition_of_done: match str_of(call, "definition_of_done") {
                    Some(d) if !d.is_empty() => {
                        Some(long_text(d, DOD_MAX, "The definition of done")?)
                    }
                    _ => None,
                },
                done_anyway: None,
                what_happened: None,
                expected: None,
                steps: None,
                environment: None,
                closed_reason: None,
                started_at: None,
                done_at: None,
                checklist,
                watchers: vec![ctx.actor.clone()],
                links: Vec::new(),
            };
            next.items.insert(item_id.clone(), task);
            next.seq += 1;
            changed.push(item_id.clone());
            if let Some(fixes) = call.get("fixes").and_then(Value::as_array) {
                for f in fixes {
                    let bug_id = str_of(f, "bug_id").unwrap_or("").to_string();
                    add_fix_link(
                        &mut next,
                        &item_id,
                        str_of(f, "link_id").unwrap_or(""),
                        &bug_id,
                        now,
                        &ctx.actor,
                        &mut changed,
                    )?;
                    follow_fix_tasks(
                        &mut next,
                        &bug_id,
                        now,
                        &mut changed,
                        FixTrigger::Linked {
                            task_status: "todo",
                        },
                    );
                }
            }
        }
        "task.update" => {
            let seq = &mut next.seq;
            let task = next
                .items
                .get_mut(&item_id)
                .filter(|t| t.summary["deleted"] != json!(true))
                .ok_or_else(|| err("not_found", "That item is not here."))?;
            expect_version(task, version)?;
            let mut any = false;
            if has(call, "title") {
                task.summary["title"] =
                    json!(one_line(str_of(call, "title"), TITLE_MAX, "The title")?);
                any = true;
            }
            if has(call, "description") && flag(call, "clear_description") {
                return invalid("Give a description or clear it, not both.");
            }
            if let Some(d) = str_of(call, "description") {
                task.description = Some(long_text(d, DESCRIPTION_MAX, "The description")?);
                any = true;
            }
            if flag(call, "clear_description") {
                task.description = None;
                any = true;
            }
            if has(call, "definition_of_done") && flag(call, "clear_definition_of_done") {
                return invalid("Give a definition of done or clear it, not both.");
            }
            if let Some(d) = str_of(call, "definition_of_done") {
                task.definition_of_done = Some(long_text(d, DOD_MAX, "The definition of done")?);
                any = true;
            }
            if flag(call, "clear_definition_of_done") {
                task.definition_of_done = None;
                any = true;
            }
            if let Some(p) = str_of(call, "priority") {
                task.summary["priority"] = json!(p);
                any = true;
            }
            if has(call, "team_id") && flag(call, "clear_team") {
                return invalid("Give a team or clear it, not both.");
            }
            if let Some(team) = str_of(call, "team_id") {
                let project_id = task.summary["project_id"].as_str().unwrap_or("");
                let ok = ctx
                    .projects
                    .iter()
                    .find(|p| p.id == project_id)
                    .is_some_and(|p| p.team_ids.iter().any(|t| t == team));
                if !ok {
                    return invalid("That team isn't one of the project's teams.");
                }
                task.summary["team_id"] = json!(team);
                any = true;
            }
            if flag(call, "clear_team") {
                task.summary.as_object_mut().map(|o| o.remove("team_id"));
                any = true;
            }
            if !any {
                return invalid("Nothing to change.");
            }
            touch(seq, task, now);
            changed.push(item_id.clone());
        }
        "task.assign" => {
            let seq = &mut next.seq;
            let task = next
                .items
                .get_mut(&item_id)
                .filter(|t| t.summary["deleted"] != json!(true))
                .ok_or_else(|| err("not_found", "That item is not here."))?;
            expect_version(task, version)?;
            match str_of(call, "assignee_id") {
                Some(a) if !ctx.members.iter().any(|m| m == a) => {
                    return Err(err("unknown_recipient", "That member cannot be assigned."));
                }
                Some(a) => task.summary["assignee_id"] = json!(a),
                None => {
                    task.summary
                        .as_object_mut()
                        .map(|o| o.remove("assignee_id"));
                }
            }
            touch(seq, task, now);
            changed.push(item_id.clone());
        }
        "task.set_status" => {
            let seq = &mut next.seq;
            let task = next
                .items
                .get_mut(&item_id)
                .filter(|t| t.summary["deleted"] != json!(true))
                .ok_or_else(|| err("not_found", "That item is not here."))?;
            expect_version(task, version)?;
            let from = task.summary["status"].as_str().unwrap_or("").to_string();
            let to = str_of(call, "status").unwrap_or("").to_string();
            let allowed: &[&str] = match from.as_str() {
                "todo" => &["in_progress", "blocked", "done"],
                "in_progress" => &["todo", "blocked", "done"],
                "blocked" => &["todo", "in_progress", "done"],
                "done" => &["todo", "in_progress"],
                _ => &[],
            };
            if !allowed.contains(&to.as_str()) {
                return invalid(format!(
                    "A task that is {} can't go to {}.",
                    from.replace('_', " "),
                    to.replace('_', " ")
                ));
            }
            if (to == "blocked") != str_of(call, "blocked_reason").is_some() {
                return invalid(if to == "blocked" {
                    "Say what it is blocked on."
                } else {
                    "A reason only goes with blocked."
                });
            }
            if let Some(reason) = str_of(call, "blocked_reason") {
                task.summary["blocked_reason"] =
                    json!(one_line(Some(reason), BLOCKED_MAX, "The reason")?);
            } else {
                task.summary
                    .as_object_mut()
                    .map(|o| o.remove("blocked_reason"));
            }
            if to != "done" && (has(call, "resolution") || has(call, "anyway_reason")) {
                return invalid("A resolution only goes with done.");
            }
            if to == "done" {
                let resolution = str_of(call, "resolution")
                    .unwrap_or("completed")
                    .to_string();
                let left: Vec<&Step> = task
                    .checklist
                    .iter()
                    .filter(|c| c.required && !c.done)
                    .collect();
                if resolution == "completed" && !left.is_empty() {
                    let Some(anyway) = str_of(call, "anyway_reason") else {
                        let names: Vec<&str> = left.iter().map(|c| c.text.as_str()).collect();
                        return Err(err(
                            "checklist_incomplete",
                            format!("Required steps are not done: {}.", names.join("; ")),
                        ));
                    };
                    let anyway = anyway.trim();
                    if anyway.is_empty() {
                        return invalid("Say why it is done with required steps left.");
                    }
                    let reason = long_text(anyway, REASON_MAX, "The reason")?;
                    let steps: Vec<String> = left.iter().map(|c| c.id.clone()).collect();
                    task.done_anyway = Some(json!({"reason": reason, "steps": steps}));
                    task.summary["done_anyway"] = json!(true);
                }
                task.summary["resolution"] = json!(resolution);
                task.done_at = Some(now.to_string());
            } else {
                task.done_at = None;
                if to == "in_progress" && task.started_at.is_none() {
                    task.started_at = Some(now.to_string());
                }
                if from == "done" {
                    let n = task.summary["reopen_count"].as_u64().unwrap_or(0) + 1;
                    task.summary["reopen_count"] = json!(n);
                }
                task.summary.as_object_mut().map(|o| o.remove("resolution"));
                task.done_anyway = None;
                task.summary["done_anyway"] = json!(false);
            }
            task.summary["status"] = json!(to);
            touch(seq, task, now);
            changed.push(item_id.clone());
            let bugs: Vec<String> = task.summary["fixes"]
                .as_array()
                .map(|f| {
                    f.iter()
                        .filter_map(|b| b.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            for bug_id in bugs {
                follow_fix_tasks(
                    &mut next,
                    &bug_id,
                    now,
                    &mut changed,
                    FixTrigger::Task {
                        from: &from,
                        to: &to,
                    },
                );
            }
        }
        "bug.report" => {
            let project_id = str_of(call, "project_id").unwrap_or("");
            let project = ctx
                .projects
                .iter()
                .find(|p| p.id == project_id)
                .ok_or_else(|| err("not_found", "That project is not here."))?;
            if project.archived {
                return invalid("That project is archived.");
            }
            let title = one_line(str_of(call, "title"), TITLE_MAX, "The title")?;
            let happened = bug_text(str_of(call, "what_happened"), "What happened", true)?;
            let severity = str_of(call, "severity").unwrap_or("");
            if !SEVERITIES.contains(&severity) {
                return invalid("Pick a severity: critical, major, minor or trivial.");
            }
            if next.items.contains_key(&item_id) {
                return Err(err("already_exists", "That item already exists."));
            }
            let area = match str_of(call, "area_id") {
                Some(a) if !project.areas.iter().any(|x| x == a) => {
                    return invalid("That area isn't one of the project's areas.");
                }
                Some(a) => Some(a.to_string()),
                None => project.general_area.clone(),
            };
            let key = format!("b:{project_id}");
            let number = next.next.get(&key).copied().unwrap_or(0) + 1;
            next.next.insert(key, number);
            let checklist = match call.get("checklist") {
                Some(list) if list.as_array().is_some_and(|l| !l.is_empty()) => {
                    new_steps(list, &[])?
                }
                _ => Vec::new(),
            };
            let mut summary = json!({
                "id": item_id, "kind": "bug", "project_id": project_id, "number": number,
                "short_id": short_id(&item_id), "title": title, "status": "new", "priority": "normal",
                "created_by": ctx.actor, "created_at": now, "updated_at": now,
                "severity": severity, "severity_confirmed": false, "reopen_count": 0,
                "fixes": [], "fixed_by": [], "checklist": counts(&checklist), "done_anyway": false,
                "owner_office_id": "hq", "generation": 1, "version": 1, "deleted": false,
            });
            if let Some(a) = area {
                summary["area_id"] = json!(a);
            }
            let environment = match str_of(call, "environment") {
                Some(e) if !e.is_empty() => Some(long_text(e, 2000, "The environment")?),
                _ => None,
            };
            let task = Task {
                summary,
                description: None,
                definition_of_done: None,
                done_anyway: None,
                what_happened: happened,
                expected: bug_text(str_of(call, "expected"), "Expected", false)?,
                steps: bug_text(str_of(call, "steps"), "Steps", false)?,
                environment,
                closed_reason: None,
                started_at: None,
                done_at: None,
                checklist,
                watchers: vec![ctx.actor.clone()],
                links: Vec::new(),
            };
            next.items.insert(item_id.clone(), task);
            next.seq += 1;
            changed.push(item_id.clone());
        }
        "bug.update" | "bug.triage" | "bug.assign" | "bug.mark_fixed" | "bug.verify"
        | "bug.close" | "bug.reopen" => {
            let task = next.items.get_mut(&item_id).expect("checked above");
            expect_version(task, version)?;
            let status = task.summary["status"].as_str().unwrap_or("").to_string();
            let project = ctx
                .projects
                .iter()
                .find(|p| task.summary["project_id"] == json!(p.id));
            let area_ok = |a: &str| project.is_some_and(|p| p.areas.iter().any(|x| x == a));
            match method {
                "bug.update" => {
                    let mut any = false;
                    if has(call, "title") {
                        task.summary["title"] =
                            json!(one_line(str_of(call, "title"), TITLE_MAX, "The title")?);
                        any = true;
                    }
                    if has(call, "what_happened") {
                        task.what_happened =
                            bug_text(str_of(call, "what_happened"), "What happened", true)?;
                        any = true;
                    }
                    for (field, clear, label) in [
                        ("expected", "clear_expected", "Expected"),
                        ("steps", "clear_steps", "Steps"),
                        ("environment", "clear_environment", "The environment"),
                    ] {
                        if has(call, field) && flag(call, clear) {
                            return invalid("Give a value or clear it, not both.");
                        }
                        let value = if has(call, field) {
                            Some(if field == "environment" {
                                Some(long_text(str_of(call, field).unwrap_or(""), 2000, label)?)
                            } else {
                                bug_text(str_of(call, field), label, false)?
                            })
                        } else if flag(call, clear) {
                            Some(None)
                        } else {
                            None
                        };
                        if let Some(v) = value {
                            match field {
                                "expected" => task.expected = v,
                                "steps" => task.steps = v,
                                _ => task.environment = v,
                            }
                            any = true;
                        }
                    }
                    if let Some(sv) = call.get("severity").filter(|v| !v.is_null()) {
                        if !sv.as_str().is_some_and(|s| SEVERITIES.contains(&s)) {
                            return invalid("Pick a severity: critical, major, minor or trivial.");
                        }
                        task.summary["severity"] = sv.clone();
                        any = true;
                    }
                    if let Some(p) = str_of(call, "priority") {
                        task.summary["priority"] = json!(p);
                        any = true;
                    }
                    if let Some(a) = str_of(call, "area_id") {
                        if !area_ok(a) {
                            return invalid("That area isn't one of the project's areas.");
                        }
                        task.summary["area_id"] = json!(a);
                        any = true;
                    }
                    if !any {
                        return invalid("Nothing to change.");
                    }
                }
                "bug.triage" => {
                    if status != "new" {
                        return invalid(format!(
                            "A bug that is {} is not waiting for triage.",
                            status.replace('_', " ")
                        ));
                    }
                    let area = str_of(call, "area_id").unwrap_or("");
                    if !area_ok(area) {
                        return invalid("That area isn't one of the project's areas.");
                    }
                    let severity = str_of(call, "severity").unwrap_or("");
                    if !SEVERITIES.contains(&severity) {
                        return invalid("Pick a severity: critical, major, minor or trivial.");
                    }
                    if let Some(a) = str_of(call, "assignee_id") {
                        if !ctx.members.iter().any(|m| m == a) {
                            return Err(err(
                                "unknown_recipient",
                                "That member cannot be assigned.",
                            ));
                        }
                        task.summary["assignee_id"] = json!(a);
                    } else {
                        task.summary
                            .as_object_mut()
                            .map(|o| o.remove("assignee_id"));
                    }
                    task.summary["area_id"] = json!(area);
                    task.summary["severity"] = json!(severity);
                    task.summary["severity_confirmed"] = json!(true);
                    task.summary["priority"] = json!(str_of(call, "priority").unwrap_or("normal"));
                    task.summary["status"] = json!("triaged");
                }
                "bug.assign" => match str_of(call, "assignee_id") {
                    Some(a) if !ctx.members.iter().any(|m| m == a) => {
                        return Err(err("unknown_recipient", "That member cannot be assigned."));
                    }
                    Some(a) => task.summary["assignee_id"] = json!(a),
                    None => {
                        task.summary
                            .as_object_mut()
                            .map(|o| o.remove("assignee_id"));
                    }
                },
                "bug.mark_fixed" => {
                    if status != "triaged" && status != "in_progress" {
                        return invalid(format!(
                            "A bug that is {} can't be marked fixed.",
                            status.replace('_', " ")
                        ));
                    }
                    let note = long_text(
                        str_of(call, "note").unwrap_or("").trim(),
                        REASON_MAX,
                        "The note",
                    )?;
                    if note.is_empty() {
                        return invalid("Say what fixed it.");
                    }
                    let left: Vec<&Step> = task
                        .checklist
                        .iter()
                        .filter(|c| c.required && !c.done)
                        .collect();
                    if !left.is_empty() {
                        let Some(anyway) = str_of(call, "anyway_reason") else {
                            let names: Vec<&str> = left.iter().map(|c| c.text.as_str()).collect();
                            return Err(err(
                                "checklist_incomplete",
                                format!("Required steps are not done: {}.", names.join("; ")),
                            ));
                        };
                        if anyway.trim().is_empty() {
                            return invalid("Say why it is fixed with required steps left.");
                        }
                        let reason = long_text(anyway.trim(), REASON_MAX, "The reason")?;
                        let steps: Vec<String> = left.iter().map(|c| c.id.clone()).collect();
                        task.done_anyway = Some(json!({"reason": reason, "steps": steps}));
                        task.summary["done_anyway"] = json!(true);
                    }
                    task.closed_reason = Some(note);
                    task.summary["status"] = json!("fixed");
                }
                "bug.verify" => {
                    if status != "fixed" {
                        return invalid("Only a fixed bug can be verified.");
                    }
                    match str_of(call, "outcome") {
                        Some("fixed") => {
                            task.summary["status"] = json!("closed");
                            task.summary["resolution"] = json!("verified");
                            task.done_at = Some(now.to_string());
                        }
                        Some("still_happens") => {
                            let seen = str_of(call, "note").unwrap_or("").trim();
                            if seen.is_empty() {
                                return invalid("Say what you still see.");
                            }
                            long_text(seen, REASON_MAX, "The note")?;
                            task.summary["status"] = json!("triaged");
                            let n = task.summary["reopen_count"].as_u64().unwrap_or(0) + 1;
                            task.summary["reopen_count"] = json!(n);
                            task.summary["done_anyway"] = json!(false);
                            task.done_anyway = None;
                        }
                        _ => return invalid("The outcome is fixed or still_happens."),
                    }
                }
                "bug.close" => {
                    if !["new", "triaged", "in_progress", "fixed"].contains(&status.as_str()) {
                        return invalid(format!(
                            "A bug that is {} can't be closed.",
                            status.replace('_', " ")
                        ));
                    }
                    if let Some(original) = str_of(call, "duplicate_of") {
                        let mine = task.summary.clone();
                        let found = next
                            .items
                            .get(original)
                            .filter(|o| {
                                o.summary["deleted"] != json!(true)
                                    && o.summary["kind"] == json!("bug")
                            })
                            .ok_or_else(|| err("not_found", "That original bug is not here."))?;
                        if original == item_id || found.summary["project_id"] != mine["project_id"]
                        {
                            return invalid(
                                "The original must be another bug in the same project.",
                            );
                        }
                        if found.summary["status"] == json!("closed")
                            && found.summary["resolution"] == json!("duplicate")
                        {
                            return invalid("That bug is itself closed as a duplicate.");
                        }
                        let creator = mine["created_by"].as_str().unwrap_or("").to_string();
                        if let Some(o) = next.items.get_mut(original) {
                            if !o.watchers.contains(&creator) {
                                o.watchers.push(creator);
                            }
                        }
                        let task = next.items.get_mut(&item_id).expect("checked above");
                        task.summary["duplicate_of"] = json!(original);
                        task.summary["resolution"] = json!("duplicate");
                    } else {
                        let resolution = str_of(call, "resolution").unwrap_or("");
                        if resolution != "wont_fix" && resolution != "cant_reproduce" {
                            return invalid(
                                "Close it as a duplicate, won't fix or can't reproduce.",
                            );
                        }
                        let reason = long_text(
                            str_of(call, "reason").unwrap_or("").trim(),
                            REASON_MAX,
                            "The reason",
                        )?;
                        if reason.is_empty() {
                            return invalid("Give a reason.");
                        }
                        let task = next.items.get_mut(&item_id).expect("checked above");
                        task.closed_reason = Some(reason);
                        task.summary["resolution"] = json!(resolution);
                    }
                    let task = next.items.get_mut(&item_id).expect("checked above");
                    task.summary["status"] = json!("closed");
                    task.done_at = Some(now.to_string());
                }
                _ => {
                    // bug.reopen
                    if status != "closed" {
                        return invalid("Only a closed bug can be reopened.");
                    }
                    let note = str_of(call, "note").unwrap_or("").trim();
                    if note.is_empty() {
                        return invalid("Say why it is reopened.");
                    }
                    long_text(note, REASON_MAX, "The note")?;
                    let duplicate = task.summary["resolution"] == json!("duplicate");
                    task.summary["status"] = json!(if duplicate { "new" } else { "triaged" });
                    let n = task.summary["reopen_count"].as_u64().unwrap_or(0) + 1;
                    task.summary["reopen_count"] = json!(n);
                    for key in ["resolution", "duplicate_of"] {
                        task.summary.as_object_mut().map(|o| o.remove(key));
                    }
                    task.done_at = None;
                }
            }
            let seq = &mut next.seq;
            let task = next.items.get_mut(&item_id).expect("checked above");
            touch(seq, task, now);
            changed.push(item_id.clone());
            if method == "bug.triage" {
                follow_fix_tasks(&mut next, &item_id, now, &mut changed, FixTrigger::Triaged);
            }
        }
        "work.link" => {
            if str_of(call, "kind") != Some("fixes") {
                return invalid("Only fix links are supported for now.");
            }
            let Some(bug_id) = call
                .get("to")
                .filter(|t| t["type"] == json!("item"))
                .and_then(|t| str_of(t, "item_id"))
                .map(String::from)
            else {
                return invalid("A fix link points at a bug.");
            };
            let from = str_of(call, "from_item_id").unwrap_or("").to_string();
            if from.is_empty() || from.len() > 128 {
                return invalid("from_item_id must be a text of 1 to 128 characters.");
            }
            let task = next
                .items
                .get(&from)
                .filter(|t| t.summary["deleted"] != json!(true))
                .ok_or_else(|| err("not_found", "That item is not here."))?;
            if task.summary["kind"] != json!("task") {
                return invalid("That is a bug, not a task.");
            }
            if from == bug_id {
                return invalid("An item cannot be linked to itself.");
            }
            changed.push(from.clone());
            add_fix_link(
                &mut next,
                &from,
                str_of(call, "link_id").unwrap_or(""),
                &bug_id,
                now,
                &ctx.actor,
                &mut changed,
            )?;
            let seq = &mut next.seq;
            touch(seq, next.items.get_mut(&from).expect("checked above"), now);
            let task_status = next.items[&from].summary["status"]
                .as_str()
                .unwrap_or("")
                .to_string();
            follow_fix_tasks(
                &mut next,
                &bug_id,
                now,
                &mut changed,
                FixTrigger::Linked {
                    task_status: &task_status,
                },
            );
        }
        "work.unlink" => {
            let link_id = str_of(call, "link_id").unwrap_or("").to_string();
            let Some(owner) = next
                .items
                .iter()
                .find(|(_, i)| i.links.iter().any(|l| l.id == link_id))
                .map(|(id, _)| id.clone())
            else {
                return Err(err("not_found", "That link is not here."));
            };
            let bug_id = next.items[&owner]
                .links
                .iter()
                .find(|l| l.id == link_id)
                .map(|l| l.to.clone())
                .expect("found above");
            let task = next.items.get_mut(&owner).expect("found above");
            task.links.retain(|l| l.id != link_id);
            if let Some(f) = task.summary["fixes"].as_array_mut() {
                f.retain(|b| *b != json!(bug_id));
            }
            let seq = &mut next.seq;
            touch(seq, task, now);
            changed.push(owner.clone());
            // Unlinking changes no bug status (design §2.4).
            if let Some(bug) = next.items.get_mut(&bug_id) {
                if let Some(f) = bug.summary["fixed_by"].as_array_mut() {
                    f.retain(|t| *t != json!(owner));
                }
                changed.push(bug_id);
            }
        }
        "checklist.add" => {
            let seq = &mut next.seq;
            let task = next
                .items
                .get_mut(&item_id)
                .filter(|t| t.summary["deleted"] != json!(true))
                .ok_or_else(|| err("not_found", "That item is not here."))?;
            let added = new_steps(call.get("steps").unwrap_or(&Value::Null), &task.checklist)?;
            let at = match str_of(call, "after_step_id") {
                Some(after) => task
                    .checklist
                    .iter()
                    .position(|c| c.id == after)
                    .map(|i| i + 1)
                    .ok_or_else(|| err("not_found", "That step is not here."))?,
                None => task.checklist.len(),
            };
            for (i, s) in added.into_iter().enumerate() {
                task.checklist.insert(at + i, s);
            }
            touch(seq, task, now);
            changed.push(item_id.clone());
        }
        "checklist.check" | "checklist.edit" | "checklist.remove" => {
            let step_id = str_of(call, "step_id").unwrap_or("").to_string();
            let owner = step_owner(&next, &step_id)?;
            let seq = &mut next.seq;
            let task = next.items.get_mut(&owner).expect("found above");
            let idx = task
                .checklist
                .iter()
                .position(|c| c.id == step_id)
                .expect("found above");
            match method {
                "checklist.check" => {
                    let done = flag(call, "done");
                    if task.checklist[idx].task_id.is_some() {
                        return invalid("This step follows its task.");
                    }
                    if task.checklist[idx].done != done {
                        let step = &mut task.checklist[idx];
                        step.done = done;
                        step.version += 1;
                        step.checked_by = done.then(|| ctx.actor.clone());
                        step.checked_at = done.then(|| now.to_string());
                        touch(seq, task, now);
                    }
                }
                "checklist.edit" => {
                    if task.checklist[idx].version != version.unwrap_or(0) {
                        return Err(err(
                            "conflict",
                            "This step changed. Reload to see the latest.",
                        ));
                    }
                    if !has(call, "text") && !has(call, "required") {
                        return invalid("Nothing to change.");
                    }
                    if has(call, "text") {
                        task.checklist[idx].text =
                            one_line(str_of(call, "text"), STEP_MAX, "A step")?;
                    }
                    if has(call, "required") {
                        task.checklist[idx].required = flag(call, "required");
                    }
                    task.checklist[idx].version += 1;
                    touch(seq, task, now);
                }
                _ => {
                    if task.checklist[idx].version != version.unwrap_or(0) {
                        return Err(err(
                            "conflict",
                            "This step changed. Reload to see the latest.",
                        ));
                    }
                    task.checklist.remove(idx);
                    touch(seq, task, now);
                }
            }
            changed.push(owner);
        }
        "work.delete" => {
            let seq = &mut next.seq;
            let task = next
                .items
                .get_mut(&item_id)
                .filter(|t| t.summary["deleted"] != json!(true))
                .ok_or_else(|| err("not_found", "That item is not here."))?;
            expect_version(task, version)?;
            task.summary["deleted"] = json!(true);
            touch(seq, task, now);
            changed.push(item_id.clone());
        }
        _ => {
            return Err(err(
                "unknown_method",
                format!("The office doesn't know {method}."),
            ))
        }
    }

    let accepted = json!({
        "status": "committed",
        "position": {"incarnation": next.incarnation, "seq": next.seq},
        "items": changed.iter().filter_map(|id| next.items.get(id)).map(|t| t.summary.clone()).collect::<Vec<_>>(),
    });
    next.answers
        .insert(change_id.clone(), (call_text, accepted.clone()));
    next.order.push(change_id);
    while next.order.len() > ANSWERS_KEPT {
        let oldest = next.order.remove(0);
        next.answers.remove(&oldest);
    }
    *state = next;
    Ok(accepted)
}

pub fn new_state() -> WorkState {
    WorkState {
        incarnation: 1,
        ..WorkState::default()
    }
}

/// The tasks of a project, not deleted; Done ones only if they went Done at or after `closed_since` (all when absent).
pub fn summaries(
    state: &WorkState,
    project_id: Option<&str>,
    closed_since: Option<&str>,
) -> Vec<Value> {
    let mut list: Vec<&Task> = state
        .items
        .values()
        .filter(|t| t.summary["deleted"] != json!(true))
        .filter(|t| project_id.is_none_or(|p| t.summary["project_id"] == json!(p)))
        .filter(|t| {
            (t.summary["status"] != json!("done") && t.summary["status"] != json!("closed"))
                || closed_since.is_none_or(|since| {
                    t.done_at
                        .as_deref()
                        .or(t.summary["updated_at"].as_str())
                        .unwrap_or("")
                        >= since
                })
        })
        .collect();
    list.sort_by_key(|t| t.summary["number"].as_u64().unwrap_or(0));
    list.into_iter().map(|t| t.summary.clone()).collect()
}

fn ms(a: &str, b: &str) -> u64 {
    fn parse(s: &str) -> i64 {
        // RFC 3339 UTC with milliseconds, as the engine writes it.
        let (date, time) = s
            .trim_end_matches('Z')
            .split_once('T')
            .unwrap_or(("1970-01-01", "00:00:00"));
        let d: Vec<i64> = date.split('-').filter_map(|x| x.parse().ok()).collect();
        let t: Vec<&str> = time.split(':').collect();
        let (y, m, day) = (
            d.first().copied().unwrap_or(1970),
            d.get(1).copied().unwrap_or(1),
            d.get(2).copied().unwrap_or(1),
        );
        // days from civil
        let y2 = if m <= 2 { y - 1 } else { y };
        let era = y2.div_euclid(400);
        let yoe = y2 - era * 400;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146097 + doe - 719468;
        let h: i64 = t.first().and_then(|x| x.parse().ok()).unwrap_or(0);
        let mi: i64 = t.get(1).and_then(|x| x.parse().ok()).unwrap_or(0);
        let sec_part = t.get(2).copied().unwrap_or("0");
        let (whole, frac) = sec_part.split_once('.').unwrap_or((sec_part, ""));
        let sec: i64 = whole.parse().unwrap_or(0);
        let millis: i64 = format!("{:0<3}", frac)
            .chars()
            .take(3)
            .collect::<String>()
            .parse()
            .unwrap_or(0);
        days.saturating_mul(86_400_000)
            .saturating_add(h * 3_600_000 + mi * 60_000 + sec * 1000 + millis)
    }
    (parse(b) - parse(a)).max(0) as u64
}

fn links_of(state: &WorkState, id: &str, back: bool) -> Vec<Value> {
    state
        .items
        .iter()
        .flat_map(|(owner, item)| item.links.iter().map(move |l| (owner, l)))
        .filter(|(owner, l)| if back { l.to == id } else { owner.as_str() == id })
        .map(|(owner, l)| {
            json!({
                "id": l.id, "from_item_id": owner, "kind": "fixes", "to": {"type": "item", "item_id": l.to},
                "created_by": l.created_by, "created_at": l.created_at,
                "target_deleted": state.items.get(&l.to).is_some_and(|t| t.summary["deleted"] == json!(true)),
            })
        })
        .collect()
}

/// One task in full (`work.get`).
pub fn get(state: &WorkState, id: &str, now: &str) -> Result<Value, ApiError> {
    let task = state
        .items
        .get(id)
        .filter(|t| t.summary["deleted"] != json!(true))
        .ok_or_else(|| err("not_found", "That item is not here."))?;
    let created = task.summary["created_at"].as_str().unwrap_or(now);
    let end = task.done_at.as_deref().unwrap_or(now);
    let mut time = json!({
        "lead_ms": ms(created, end), "waiting_on_owner_ms": 0, "blocked_ms": 0, "rest_ms": ms(created, end),
        "open": task.summary["status"] != json!("done") && task.summary["status"] != json!("closed"),
        "rounds": 1 + task.summary["reopen_count"].as_u64().unwrap_or(0), "as_of": now,
    });
    if let Some(started) = &task.started_at {
        time["cycle_ms"] = json!(ms(started, end));
    }
    let mut item = json!({
        "summary": task.summary, "backlinks_more": false, "watchers_more": false,
        "checklist": task.checklist, "links": links_of(state, id, false), "backlinks": links_of(state, id, true), "watchers": task.watchers, "time": time,
    });
    if let Some(d) = &task.description {
        item["description"] = json!(d);
    }
    if let Some(d) = &task.definition_of_done {
        item["definition_of_done"] = json!(d);
    }
    if let Some(d) = &task.done_anyway {
        item["done_anyway_detail"] = d.clone();
    }
    Ok(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every field in `want` is in `have`, recursively (arrays element by element).
    fn subset(have: &Value, want: &Value, path: &str) -> Option<String> {
        match want {
            Value::Object(w) => {
                for (k, v) in w {
                    if let Some(bad) = subset(
                        have.get(k).unwrap_or(&Value::Null),
                        v,
                        &format!("{path}.{k}"),
                    ) {
                        return Some(bad);
                    }
                }
                None
            }
            Value::Array(w) => {
                for (i, v) in w.iter().enumerate() {
                    if let Some(bad) = subset(
                        have.get(i).unwrap_or(&Value::Null),
                        v,
                        &format!("{path}[{i}]"),
                    ) {
                        return Some(bad);
                    }
                }
                None
            }
            _ => (have != want).then(|| format!("{path}: expected {want}, got {have}")),
        }
    }

    fn ctx_of(base: &Value, now: Option<&str>) -> Ctx {
        Ctx {
            now: now.unwrap_or(base["now"].as_str().unwrap()).to_string(),
            actor: base["actor"].as_str().unwrap().to_string(),
            projects: base["projects"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| Project {
                    id: p["id"].as_str().unwrap().to_string(),
                    archived: p["archived"].as_bool().unwrap_or(false),
                    areas: p["areas"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|x| x["id"].as_str().unwrap().to_string())
                                .collect()
                        })
                        .unwrap_or_default(),
                    general_area: p["areas"]
                        .as_array()
                        .and_then(|a| a.iter().find(|x| x["general"] == true))
                        .map(|x| x["id"].as_str().unwrap().to_string()),
                    team_ids: p["teamIds"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|t| t.as_str().unwrap().to_string())
                        .collect(),
                })
                .collect(),
            members: base["members"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m.as_str().unwrap().to_string())
                .collect(),
        }
    }

    /// The cases shared with the TypeScript mock (`src/lib/work/vectors.test.ts`): both engines must agree.
    #[test]
    fn the_shared_cases_agree_with_the_typescript_engine() {
        let vectors: Value = serde_json::from_str(include_str!(
            "../../../../crates/org-work/test-vectors/work-commands.json"
        ))
        .unwrap();
        let cases = vectors["cases"].as_array().unwrap();
        assert_eq!(
            cases.len(),
            29,
            "a case was added or removed: update the Rust runner's count"
        );
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let mut state = new_state();
            for (n, step) in case["steps"].as_array().unwrap().iter().enumerate() {
                let now = step["now"].as_str();
                let ctx = ctx_of(&vectors["context"], now);
                if let Some(id) = step["get"].as_str() {
                    let item = get(&state, id, now.unwrap_or(&ctx.now)).unwrap();
                    assert_eq!(subset(&item, &step["expect"], ""), None, "{name}, step {n}");
                    continue;
                }
                let result = apply(&mut state, &step["call"], &ctx);
                if let Some(code) = step["error"].as_str() {
                    assert_eq!(result.err().map(|e| e.code), Some(code), "{name}, step {n}");
                } else {
                    let accepted = result.unwrap_or_else(|e| panic!("{name}, step {n}: {e:?}"));
                    assert_eq!(
                        subset(&accepted["items"], &step["items"], ""),
                        None,
                        "{name}, step {n}"
                    );
                }
            }
        }
    }

    #[test]
    fn done_tasks_are_listed_only_since_the_cutoff() {
        let vectors: Value = serde_json::from_str(include_str!(
            "../../../../crates/org-work/test-vectors/work-commands.json"
        ))
        .unwrap();
        let mut state = new_state();
        let at = |now: &str| ctx_of(&vectors["context"], Some(now));
        let id = "01JC0000000000000000000001";
        let create = json!({"method": "task.create", "change_id": "a", "item_id": id, "project_id": "p1", "title": "One"});
        apply(&mut state, &create, &at("2026-11-01T00:00:00.000Z")).unwrap();
        let done = json!({"method": "task.set_status", "change_id": "b", "item_id": id, "expected_version": 1, "status": "done"});
        apply(&mut state, &done, &at("2026-11-01T03:00:00.000Z")).unwrap();
        assert_eq!(
            summaries(&state, Some("p1"), Some("2026-11-01T02:00:00.000Z")).len(),
            1
        );
        assert_eq!(
            summaries(&state, Some("p1"), Some("2026-11-01T04:00:00.000Z")).len(),
            0
        );
        assert_eq!(summaries(&state, Some("p1"), None).len(), 1);
    }
}
