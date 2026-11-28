//! Server-side `--fields` pruning for `review.inspect` responses (`TASK-92.1`).

use org_proto::reviews::ReviewInspectSelection;
use serde_json::Value;

/// Removes unrequested sections (and `review.body_markdown`) from a serialized
/// [`org_proto::reviews::ReviewInspectResult`] value before sending it on the wire.
pub(super) fn prune_review_inspect(mut val: Value, sel: ReviewInspectSelection) -> Value {
    let Some(obj) = val.as_object_mut() else {
        return val;
    };
    if !sel.body {
        if let Some(review) = obj.get_mut("review").and_then(Value::as_object_mut) {
            review.remove("body_markdown");
        }
    }
    let sections = [
        ("rounds", sel.rounds),
        ("reviewers", sel.reviewers),
        ("findings", sel.findings),
    ];
    for (key, keep) in sections {
        if !keep {
            obj.remove(key);
        }
    }
    val
}
