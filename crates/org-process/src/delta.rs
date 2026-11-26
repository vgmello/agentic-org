//! The difference between an agent's standing process set and the set for one task
//! (`doc-27` §1, `doc-34` §3.1).
//!
//! An agent's standing instructions carry the set resolved for its home team chain, so they stay
//! the same from turn to turn and across tasks. A task in a project resolves against the
//! project's own chain instead. The turn that names the task carries only the difference: the
//! documents that apply to the task and are not already standing, and the standing ones that do
//! not apply to it.

use std::collections::BTreeMap;

use crate::model::strip_control_and_invisible;
use crate::render::{mandatory_footprint, render_instructions, RenderError, RenderInput};
use crate::resolve::{EffectiveEntry, EffectiveSet, Winner};

/// What a task's chain changes about an agent's standing process set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskDelta {
    /// Entries of the task's set whose winning document is not the standing one (or that the
    /// standing set lacks), in the task set's order.
    pub applies: EffectiveSet,
    /// Standing entries with a winner that the task's chain does not apply.
    pub not_applicable: Vec<EffectiveEntry>,
}

impl TaskDelta {
    /// Whether the task's chain changes nothing.
    pub fn is_empty(&self) -> bool {
        self.applies.entries.is_empty() && self.not_applicable.is_empty()
    }
}

/// Whether two winners are the same document at the same revision and content.
fn same_winner(a: Option<&Winner>, b: Option<&Winner>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            a.doc.id == b.doc.id
                && a.doc.revision == b.doc.revision
                && a.doc.blob_sha256 == b.doc.blob_sha256
        }
        (None, None) => true,
        _ => false,
    }
}

/// Compares the standing set with the task's set, entry by entry (`entry_id`).
pub fn task_delta(standing: &EffectiveSet, task: &EffectiveSet) -> TaskDelta {
    let standing_by_id: BTreeMap<&str, &EffectiveEntry> = standing
        .entries
        .iter()
        .map(|entry| (entry.entry_id.as_str(), entry))
        .collect();
    let task_by_id: BTreeMap<&str, &EffectiveEntry> = task
        .entries
        .iter()
        .map(|entry| (entry.entry_id.as_str(), entry))
        .collect();
    let applies = task
        .entries
        .iter()
        .filter(|entry| entry.winner.is_some())
        .filter(|entry| {
            let standing_winner = standing_by_id
                .get(entry.entry_id.as_str())
                .and_then(|s| s.winner.as_ref());
            !same_winner(standing_winner, entry.winner.as_ref())
        })
        .cloned()
        .collect();
    let not_applicable = standing
        .entries
        .iter()
        .filter(|entry| entry.winner.is_some())
        .filter(|entry| {
            task_by_id
                .get(entry.entry_id.as_str())
                .is_none_or(|t| t.winner.is_none())
        })
        .cloned()
        .collect();
    TaskDelta {
        applies: EffectiveSet {
            chain: task.chain.clone(),
            entries: applies,
            problems: Vec::new(),
        },
        not_applicable,
    }
}

/// Separator between the parts of the task process context.
const PART_SEP: &str = "\n\n";

/// One entry by what it delivers: its ID and its winner's document, revision and content.
type EntryIdentity<'a> = (&'a str, Option<(&'a str, u64, &'a str)>);

fn entry_identities(entries: &[EffectiveEntry]) -> Vec<EntryIdentity<'_>> {
    entries
        .iter()
        .map(|e| {
            let winner = e.winner.as_ref().map(|w| {
                (
                    w.doc.id.as_str(),
                    w.doc.revision,
                    w.doc.blob_sha256.as_str(),
                )
            });
            (e.entry_id.as_str(), winner)
        })
        .collect()
}

/// Whether two deltas change the same documents (same winners, same documents dropped), so
/// one section can serve both tasks.
fn same_documents(a: &TaskDelta, b: &TaskDelta) -> bool {
    entry_identities(&a.applies.entries) == entry_identities(&b.applies.entries)
        && entry_identities(&a.not_applicable) == entry_identities(&b.not_applicable)
}

/// `## Processes for task <label>` (or `tasks <a>, <b>`) and the line that frames the section.
fn section_heading(labels: &[&str]) -> String {
    let labels: Vec<String> = labels
        .iter()
        .map(|l| strip_control_and_invisible(l))
        .collect();
    if labels.len() == 1 {
        format!(
            "## Processes for task {}\nThis task resolves against its own chain. For work on it, the documents below apply in addition to, or in place of, your standing processes and standards.",
            labels[0]
        )
    } else {
        format!(
            "## Processes for tasks {}\nThese tasks resolve against their own chain. For work on them, the documents below apply in addition to, or in place of, your standing processes and standards.",
            labels.join(", ")
        )
    }
}

/// The standing documents a section's tasks do not apply, by title and scope (empty when none).
fn not_applicable_list(not_applicable: &[EffectiveEntry], several: bool) -> String {
    if not_applicable.is_empty() {
        return String::new();
    }
    let these = if several { "these tasks" } else { "this task" };
    let mut out = format!("{PART_SEP}These standing documents do not apply to {these}:\n");
    for winner in not_applicable.iter().filter_map(|e| e.winner.as_ref()) {
        out.push_str(&format!(
            "- {} ({})\n",
            strip_control_and_invisible(&winner.doc.title),
            strip_control_and_invisible(&winner.scope_name)
        ));
    }
    out
}

/// Renders, for the turn that names the tasks in `tasks` (`(label, task set)` pairs), what each
/// task's chain changes about the agent's `standing` set: an empty string when nothing changes.
/// Tasks whose chains change the same documents share one section.
///
/// Must-follow documents that apply are rendered in full, as at launch. Everything the model then
/// holds counts against `input.total_budget_bytes`: the standing must-follow section already in
/// its instructions, each section's heading and not-applicable list, and the must-follow
/// documents each section adds. When they exceed the budget the turn is refused like a launch
/// ([`RenderError::MandatoryBudgetExceeded`], naming every must-follow document involved);
/// overridable guidance gets only what is left.
pub fn render_task_deltas(
    standing: &EffectiveSet,
    tasks: &[(&str, &EffectiveSet)],
    input: &RenderInput,
) -> Result<String, RenderError> {
    let mut sections: Vec<(TaskDelta, Vec<&str>)> = Vec::new();
    for &(label, task) in tasks {
        let delta = task_delta(standing, task);
        if delta.is_empty() {
            continue;
        }
        match sections.iter_mut().find(|(d, _)| same_documents(d, &delta)) {
            Some((_, labels)) if labels.contains(&label) => {}
            Some((_, labels)) => labels.push(label),
            None => sections.push((delta, vec![label])),
        }
    }
    if sections.is_empty() {
        return Ok(String::new());
    }
    let (mut used, mut titles) = mandatory_footprint(standing);
    let mut out = String::new();
    for (delta, labels) in &sections {
        let sep = if out.is_empty() { 0 } else { PART_SEP.len() };
        let heading = section_heading(labels);
        let dropped = not_applicable_list(&delta.not_applicable, labels.len() > 1);
        let framing = sep + heading.len() + PART_SEP.len() + dropped.len();
        let (adds_bytes, adds_titles) = mandatory_footprint(&delta.applies);
        titles.extend(adds_titles);
        let overflow = |required_bytes: usize| RenderError::MandatoryBudgetExceeded {
            required_bytes,
            budget_bytes: input.total_budget_bytes,
            document_titles: titles.clone(),
        };
        let Some(available) = input.total_budget_bytes.checked_sub(used + framing) else {
            return Err(overflow(used + framing + adds_bytes));
        };
        let rendered = render_instructions(
            &delta.applies,
            &RenderInput {
                read_blobs: input.read_blobs.clone(),
                overridable_cap_bytes: input.overridable_cap_bytes,
                total_budget_bytes: available,
            },
        )
        .map_err(|err| match err {
            RenderError::MandatoryBudgetExceeded { required_bytes, .. } => {
                overflow(used + framing + required_bytes)
            }
        })?;
        if sep > 0 {
            out.push_str(PART_SEP);
        }
        out.push_str(&heading);
        if !rendered.text.is_empty() {
            out.push_str(PART_SEP);
            out.push_str(&rendered.text);
        }
        out.push_str(&dropped);
        used = used + framing + rendered.text.len();
    }
    Ok(out)
}

#[cfg(test)]
#[path = "delta_tests.rs"]
mod tests;
