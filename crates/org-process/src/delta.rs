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
use crate::render::{render_instructions, RenderError, RenderInput};
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

/// Renders `delta` for the turn that names the task `task_label`: an empty string when the
/// task's chain changes nothing. Must-follow documents that apply are rendered in full, as at
/// launch; when they alone exceed `input.total_budget_bytes` the turn is refused like a launch.
pub fn render_task_delta(
    delta: &TaskDelta,
    task_label: &str,
    input: &RenderInput,
) -> Result<String, RenderError> {
    if delta.is_empty() {
        return Ok(String::new());
    }
    let label = strip_control_and_invisible(task_label);
    let mut out = format!(
        "## Processes for task {label}\nThis task resolves against its own chain. For work on it, the documents below apply in addition to, or in place of, your standing processes and standards."
    );
    let rendered = render_instructions(&delta.applies, input)?;
    if !rendered.text.is_empty() {
        out.push_str("\n\n");
        out.push_str(&rendered.text);
    }
    if !delta.not_applicable.is_empty() {
        out.push_str("\n\nThese standing documents do not apply to this task:\n");
        for entry in &delta.not_applicable {
            if let Some(winner) = &entry.winner {
                out.push_str(&format!(
                    "- {} ({})\n",
                    strip_control_and_invisible(&winner.doc.title),
                    strip_control_and_invisible(&winner.scope_name)
                ));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "delta_tests.rs"]
mod tests;
