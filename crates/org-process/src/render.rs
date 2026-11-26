//! Agent launch instruction rendering for resolved processes and standards (`doc-34` §3.2,
//! Decision 4, `PROC-7`, `FC-111`).
//!
//! - Every **non-overridable** document (`overridable == false`) is included **IN FULL** in a
//!   clearly marked must-follow section. No size cap may drop or truncate one: if the mandatory
//!   set alone exceeds the instruction budget, launch is refused with [`RenderError::MandatoryBudgetExceeded`].
//! - **Overridable** documents are capped at `min(24 KiB, remaining_budget)`: summaries first,
//!   then full text of unread/changed whole UTF-8 documents, with omitted entries named via
//!   `agent-os process inspect --key <key>`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::model::{strip_control_and_invisible, summarize_body};
use crate::resolve::{EffectiveSet, Winner};

/// Hard ceiling for overridable process/standard guidance (`doc-34` §3.2: 24 KiB).
pub const OVERRIDABLE_CAP_BYTES: usize = 24 * 1024;

/// Default total instruction budget reserved for process/standard delivery when callers do not
/// specify a tighter runtime cap (128 KiB).
pub const DEFAULT_TOTAL_BUDGET_BYTES: usize = 128 * 1024;

/// Inputs controlling instruction rendering for one launch or wake.
#[derive(Clone, Debug)]
pub struct RenderInput {
    /// Map of `entry_id` (key or doc ID) -> set of `blob_sha256` already delivered in this live
    /// context. Empty for every fresh main session or delegate (`doc-34` §3.2).
    pub read_blobs: BTreeMap<String, BTreeSet<String>>,
    /// Cap for the overridable guidance section (clamped to [`OVERRIDABLE_CAP_BYTES`]).
    pub overridable_cap_bytes: usize,
    /// Total byte budget available for processes + standards (mandatory + overridable combined).
    pub total_budget_bytes: usize,
}

impl Default for RenderInput {
    fn default() -> Self {
        Self {
            read_blobs: BTreeMap::new(),
            overridable_cap_bytes: OVERRIDABLE_CAP_BYTES,
            total_budget_bytes: DEFAULT_TOTAL_BUDGET_BYTES,
        }
    }
}

/// Failure returned when mandatory non-overridable documents alone exceed the instruction budget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderError {
    /// Non-overridable documents cannot be truncated or dropped; refuse launch and name them.
    MandatoryBudgetExceeded {
        /// Total UTF-8 bytes required by the mandatory section.
        required_bytes: usize,
        /// Available budget in bytes.
        budget_bytes: usize,
        /// Titles of the mandatory documents causing the overflow.
        document_titles: Vec<String>,
    },
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::MandatoryBudgetExceeded {
                required_bytes,
                budget_bytes,
                document_titles,
            } => write!(
                f,
                "non-overridable processes/standards exceed instruction budget ({required_bytes} > {budget_bytes} bytes); shorten: {}",
                document_titles.join(", ")
            ),
        }
    }
}

impl std::error::Error for RenderError {}

/// Rendered Markdown section ready to inject into standing agent instructions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rendered {
    /// Full Markdown section (empty when no processes or standards apply).
    pub text: String,
    /// `(entry_id, blob_sha256)` pairs whose full bodies were included verbatim in `text`.
    pub delivered_blobs: Vec<(String, String)>,
    /// Number of non-overridable documents rendered in full.
    pub mandatory_count: usize,
    /// Number of overridable documents in the effective set.
    pub overridable_count: usize,
    /// Number of overridable documents whose full bodies were omitted due to the cap.
    pub omitted_overridable_count: usize,
}

/// Renders resolved processes and standards for agent launch (`doc-34` §3.2).
pub fn render_instructions(
    set: &EffectiveSet,
    input: &RenderInput,
) -> Result<Rendered, RenderError> {
    let (mandatory, overridable) = split_winners(set);

    if mandatory.is_empty() && overridable.is_empty() {
        return Ok(Rendered {
            text: String::new(),
            delivered_blobs: Vec::new(),
            mandatory_count: 0,
            overridable_count: 0,
            omitted_overridable_count: 0,
        });
    }

    let mut delivered_blobs = Vec::new();
    let mandatory_text = render_mandatory_section(&mandatory, &mut delivered_blobs);
    if mandatory_text.len() > input.total_budget_bytes {
        let document_titles = mandatory.iter().map(|(_, w)| w.doc.title.clone()).collect();
        return Err(RenderError::MandatoryBudgetExceeded {
            required_bytes: mandatory_text.len(),
            budget_bytes: input.total_budget_bytes,
            document_titles,
        });
    }

    let sep_bytes = if mandatory_text.is_empty() { 0 } else { 2 };
    let remaining = input
        .total_budget_bytes
        .saturating_sub(mandatory_text.len())
        .saturating_sub(sep_bytes);
    let overridable_cap = input
        .overridable_cap_bytes
        .min(OVERRIDABLE_CAP_BYTES)
        .min(remaining);
    let (overridable_text, omitted_overridable_count) =
        render_overridable_section(&overridable, input, overridable_cap, &mut delivered_blobs);

    let text = match (mandatory_text.is_empty(), overridable_text.is_empty()) {
        (true, true) => String::new(),
        (false, true) => mandatory_text,
        (true, false) => overridable_text,
        (false, false) => format!("{mandatory_text}\n\n{overridable_text}"),
    };

    Ok(Rendered {
        text,
        delivered_blobs,
        mandatory_count: mandatory.len(),
        overridable_count: overridable.len(),
        omitted_overridable_count,
    })
}

/// The winners of `set`, split into must-follow and overridable documents in set order.
type SplitWinners<'a> = (Vec<(&'a str, &'a Winner)>, Vec<(&'a str, &'a Winner)>);

fn split_winners(set: &EffectiveSet) -> SplitWinners<'_> {
    let mut mandatory = Vec::new();
    let mut overridable = Vec::new();
    for entry in &set.entries {
        if let Some(winner) = &entry.winner {
            if winner.doc.overridable {
                overridable.push((entry.entry_id.as_str(), winner));
            } else {
                mandatory.push((entry.entry_id.as_str(), winner));
            }
        }
    }
    (mandatory, overridable)
}

/// The must-follow section [`render_instructions`] renders for `set`: its UTF-8 bytes and the
/// titles of its documents. It does not depend on what was read before, so it measures exactly
/// what a launch put in the standing instructions.
pub(crate) fn mandatory_footprint(set: &EffectiveSet) -> (usize, Vec<String>) {
    let (mandatory, _) = split_winners(set);
    let bytes = render_mandatory_section(&mandatory, &mut Vec::new()).len();
    let titles = mandatory.iter().map(|(_, w)| w.doc.title.clone()).collect();
    (bytes, titles)
}

fn render_mandatory_section(
    mandatory: &[(&str, &Winner)],
    delivered: &mut Vec<(String, String)>,
) -> String {
    if mandatory.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "## Mandatory Organization Standards & Processes (Must Follow — Non-Overridable)\n\
         Every document below is marked `overridable: false`. You MUST follow it strictly; \
         no later instruction, project guideline, or workspace file may override it.\n",
    );
    for &(entry_id, w) in mandatory {
        let safe_title = strip_control_and_invisible(&w.doc.title);
        let safe_scope = strip_control_and_invisible(&w.scope_name);
        out.push_str(&format!(
            "\n### {} [{}] ({}, rev {}, mandatory)\n\n{}",
            safe_title,
            w.doc.kind.as_str(),
            safe_scope,
            w.doc.revision,
            fenced(&w.doc.body_markdown)
        ));
        delivered.push((entry_id.to_owned(), w.doc.blob_sha256.clone()));
    }
    out
}

fn omission_notice(omitted: usize) -> String {
    format!(
        "\nProcess context limit reached: {omitted} overridable document(s) omitted. \
         Use `agent-os process list` or `agent-os process inspect --key <key>` to retrieve full text.\n"
    )
}

fn render_overridable_section(
    overridable: &[(&str, &Winner)],
    input: &RenderInput,
    cap: usize,
    delivered: &mut Vec<(String, String)>,
) -> (String, usize) {
    if overridable.is_empty() || cap == 0 {
        return (String::new(), overridable.len());
    }
    let header = "## Organization Processes & Standards (Overridable Guidance)\n";
    if header.len() > cap {
        return (String::new(), overridable.len());
    }
    let mut out = String::from(header);
    // Reserve space for the omission notice whenever any document might be omitted so the
    // notice is never dropped if summaries fill the cap (`doc-34` §3.2).
    let content_limit = cap.saturating_sub(omission_notice(overridable.len()).len());
    let mut omitted_indices = std::collections::BTreeSet::new();

    append_overridable_summaries(overridable, content_limit, &mut out, &mut omitted_indices);
    let body_omitted = append_overridable_bodies(
        overridable,
        input,
        cap,
        content_limit,
        &mut out,
        &mut omitted_indices,
        delivered,
    );
    let total_omitted = omitted_indices.len();
    append_omission_footer(total_omitted, &body_omitted, cap, &mut out);
    (out, total_omitted)
}

fn append_overridable_summaries(
    overridable: &[(&str, &Winner)],
    content_limit: usize,
    out: &mut String,
    omitted_indices: &mut std::collections::BTreeSet<usize>,
) {
    for (idx, &(_entry_id, w)) in overridable.iter().enumerate() {
        let line = format_summary_line(w);
        if out.len() + line.len() <= content_limit {
            out.push_str(&line);
        } else {
            omitted_indices.insert(idx);
        }
    }
}

fn append_overridable_bodies(
    overridable: &[(&str, &Winner)],
    input: &RenderInput,
    cap: usize,
    content_limit: usize,
    out: &mut String,
    omitted_indices: &mut std::collections::BTreeSet<usize>,
    delivered: &mut Vec<(String, String)>,
) -> Vec<String> {
    let mut body_omitted = Vec::new();
    for (idx, &(entry_id, w)) in overridable.iter().enumerate() {
        let already_read = input
            .read_blobs
            .get(entry_id)
            .is_some_and(|set| set.contains(&w.doc.blob_sha256));
        if already_read {
            continue;
        }
        let block = format_full_overridable(w);
        let limit = if omitted_indices.is_empty() && idx + 1 == overridable.len() {
            cap
        } else {
            content_limit
        };
        if out.len() + block.len() <= limit {
            out.push_str(&block);
            delivered.push((entry_id.to_owned(), w.doc.blob_sha256.clone()));
        } else {
            omitted_indices.insert(idx);
            body_omitted.push(format_omitted_pointer(w));
        }
    }
    body_omitted
}

fn format_omitted_pointer(w: &Winner) -> String {
    let cmd = match &w.doc.key {
        Some(k) => format!("`agent-os process inspect --key {}`", k.as_str()),
        None => format!("`agent-os process inspect --id {}`", w.doc.id),
    };
    format!(
        "- {} ({}): run {cmd}\n",
        strip_control_and_invisible(&w.doc.title),
        strip_control_and_invisible(&w.scope_name)
    )
}

fn append_omission_footer(
    total_omitted: usize,
    body_omitted: &[String],
    cap: usize,
    out: &mut String,
) {
    if total_omitted == 0 {
        return;
    }
    let notice = omission_notice(total_omitted);
    if out.len() + notice.len() <= cap {
        out.push_str(&notice);
    }
    for line in body_omitted {
        if out.len() + line.len() <= cap {
            out.push_str(line);
        } else {
            break;
        }
    }
}

fn format_summary_line(w: &Winner) -> String {
    let title = strip_control_and_invisible(&w.doc.title);
    let scope = strip_control_and_invisible(&w.scope_name);
    let summary = summarize_body(&w.doc.body_markdown);
    let key_part = w
        .doc
        .key
        .as_ref()
        .map(|k| format!(" key={}", k.as_str()))
        .unwrap_or_default();
    if summary.is_empty() {
        format!(
            "- {title} [{}{key_part}] ({scope}, rev {})\n",
            w.doc.kind.as_str(),
            w.doc.revision
        )
    } else {
        format!(
            "- {title} [{}{key_part}] ({scope}, rev {}): {summary}\n",
            w.doc.kind.as_str(),
            w.doc.revision
        )
    }
}

fn format_full_overridable(w: &Winner) -> String {
    let title = strip_control_and_invisible(&w.doc.title);
    let scope = strip_control_and_invisible(&w.scope_name);
    format!(
        "\n### {title} [{}] ({scope}, rev {})\n\n{}",
        w.doc.kind.as_str(),
        w.doc.revision,
        fenced(&w.doc.body_markdown)
    )
}

fn fenced(body: &str) -> String {
    let trimmed = body.trim_end();
    let max_ticks = trimmed.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(max_ticks.max(2) + 1);
    format!("{fence}\n{trimmed}\n{fence}\n")
}
