//! Antigravity `SUB-6` sub-agent reference extraction and nesting.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::Value;

use org_agent_runtime::transcript::{
    merge_usage, LoadedSubagentTranscript, ParsedTranscript, SubagentTranscript,
    SubagentTranscriptRef, TranscriptEntry, TranscriptEntryKind,
};

/// Maximum deduplicated sub-agent references extracted from a single Agy transcript.
///
/// Bounded above the office's 32-subagent load limit (`32 * 2 = 64`) so office callers
/// reliably observe overflow past 32 and set `truncated = true`, even when some referenced
/// sibling conversation directories are missing on disk.
pub(super) const MAX_EXTRACTED_SUBAGENT_REFS: usize = 64;

fn is_safe_agy_conversation_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.contains("..")
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && !id.starts_with('.')
}

/// Extracts deduplicated sub-agent references launched via `invoke_subagent`, skipping self-refs
/// and unsafe conversation IDs.
pub(super) fn extract_agy_subagent_refs(
    raw: &str,
    parent_session_id: &str,
) -> Vec<SubagentTranscriptRef> {
    let mut seen = BTreeSet::new();
    let mut refs = Vec::new();
    for line in raw.lines() {
        if refs.len() >= MAX_EXTRACTED_SUBAGENT_REFS {
            break;
        }
        let Ok(val) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let Some(conv_id) = val
            .pointer("/result/conversationId")
            .or_else(|| val.get("conversationId"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        if conv_id == parent_session_id
            || !is_safe_agy_conversation_id(conv_id)
            || !seen.insert(conv_id.to_owned())
        {
            continue;
        }
        let role = val
            .pointer("/spec/role")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let tool_call_id = val
            .get("tool_call_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        refs.push(SubagentTranscriptRef {
            agent_id: conv_id.to_owned(),
            label: role,
            parent_tool_use_id: tool_call_id,
        });
    }
    refs
}

/// Attaches parsed Agy sub-agent transcripts onto their launching `invoke_subagent` `ToolCall`
/// (matched by `tool_call_id` first, then FIFO for unlinked refs; `SUB-6`).
pub(super) fn attach_agy_subagents(
    parent: &mut ParsedTranscript,
    subagents: Vec<LoadedSubagentTranscript>,
) {
    let mut by_call_id: BTreeMap<String, Vec<SubagentTranscript>> = BTreeMap::new();
    let mut fifo_unlinked: VecDeque<SubagentTranscript> = VecDeque::new();
    let mut seen_ids = BTreeSet::new();
    for sub in subagents {
        let LoadedSubagentTranscript {
            agent_id,
            label,
            parent_tool_use_id,
            parsed,
        } = sub;
        if !seen_ids.insert(agent_id.clone()) {
            continue;
        }
        merge_usage(&mut parent.usage, &parsed.usage);
        let view = SubagentTranscript {
            agent_id,
            label,
            parent_tool_use_id: parent_tool_use_id.clone(),
            format_id: parsed.format_id,
            entry_count: parsed.entries.len() as u32,
            entries: parsed.entries,
            files: parsed.files,
            usage: parsed.usage,
        };
        if let Some(cid) = parent_tool_use_id {
            by_call_id.entry(cid).or_default().push(view);
        } else {
            fifo_unlinked.push_back(view);
        }
    }
    let mut rebuilt = Vec::with_capacity(parent.entries.len() + fifo_unlinked.len());
    for mut entry in std::mem::take(&mut parent.entries) {
        let extras = attach_matching_agy(&mut entry, &mut by_call_id, &mut fifo_unlinked);
        let turn = entry.turn_index;
        rebuilt.push(entry);
        for sub in extras {
            rebuilt.push(make_agy_subagent_entry(turn, sub));
        }
    }
    let last_turn = rebuilt.last().map_or(1, |e| e.turn_index);
    for sub in by_call_id.into_values().flatten().chain(fifo_unlinked) {
        rebuilt.push(make_agy_subagent_entry(last_turn, sub));
    }
    parent.entries = rebuilt;
}

fn attach_matching_agy(
    entry: &mut TranscriptEntry,
    by_call_id: &mut BTreeMap<String, Vec<SubagentTranscript>>,
    fifo_unlinked: &mut VecDeque<SubagentTranscript>,
) -> Vec<SubagentTranscript> {
    if entry.kind != TranscriptEntryKind::ToolCall
        || entry.tool_name.as_deref() != Some("invoke_subagent")
        || entry.subagent.is_some()
    {
        return Vec::new();
    }
    if let Some(mut list) = entry
        .tool_use_id
        .as_deref()
        .and_then(|cid| by_call_id.remove(cid))
    {
        if !list.is_empty() {
            entry.subagent = Some(list.remove(0));
        }
        return list;
    }
    if let Some(sub) = fifo_unlinked.pop_front() {
        entry.subagent = Some(sub);
    }
    Vec::new()
}

fn make_agy_subagent_entry(turn_index: u32, sub: SubagentTranscript) -> TranscriptEntry {
    let agent_id = sub.agent_id.clone();
    TranscriptEntry {
        id: format!("subagent-{agent_id}"),
        turn_index,
        kind: TranscriptEntryKind::SubagentRun,
        timestamp: None,
        role: Some("assistant".to_owned()),
        text: Some(
            sub.label
                .clone()
                .unwrap_or_else(|| format!("Sub-agent {agent_id}")),
        ),
        tool_name: Some("invoke_subagent".to_owned()),
        tool_use_id: sub.parent_tool_use_id.clone(),
        tool_input: None,
        tool_output: None,
        is_error: false,
        file_path: None,
        file_action: None,
        diff_preview: None,
        usage: Some(sub.usage.clone()),
        raw_type: None,
        subagent: Some(sub),
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_agy_subagent_refs, MAX_EXTRACTED_SUBAGENT_REFS};

    #[test]
    fn extract_agy_subagent_refs_caps_and_filters_unsafe_ids() {
        let mut lines = vec![
            r#"{"type":"TOOL_RESPONSE","tool_call_id":"bad_0","result":{"conversationId":"../escape"}}"#.to_owned(),
            r#"{"type":"TOOL_RESPONSE","tool_call_id":"bad_1","result":{"conversationId":".hidden"}}"#.to_owned(),
        ];
        for idx in 0..(MAX_EXTRACTED_SUBAGENT_REFS + 15) {
            lines.push(format!(
                r#"{{"type":"TOOL_RESPONSE","tool_call_id":"call_{idx}","result":{{"conversationId":"sub-{idx:03}"}},"spec":{{"role":"Worker {idx}"}}}}"#
            ));
        }
        let raw = lines.join("\n");
        let refs = extract_agy_subagent_refs(&raw, "parent-conv");
        assert_eq!(refs.len(), MAX_EXTRACTED_SUBAGENT_REFS);
        assert_eq!(refs[0].agent_id, "sub-000");
        assert_eq!(
            refs[MAX_EXTRACTED_SUBAGENT_REFS - 1].agent_id,
            format!("sub-{:03}", MAX_EXTRACTED_SUBAGENT_REFS - 1)
        );
    }
}
