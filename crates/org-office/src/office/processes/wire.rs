//! Process documents, layers and effective entries as wire views, and UTF-8 safe chunking
//! of shown document bodies.

use std::collections::BTreeMap;

use org_process::{
    summarize_body, DocumentKind, DocumentStatus, EffectiveEntry, Layer, LayerOutcome, Scope,
    WinnerKind,
};
use org_proto::processes::{
    DocumentKindWire, DocumentStatusWire, EffectiveProcessEntryWire, ProcessDocumentWire,
    ProcessLayerWire,
};
use org_proto::{ApiError, ErrorCode};
use org_store::Id;

use crate::organization::records::By;
use crate::processes::records::{structure_document_path, StructureDocument};

/// Maximum content chunk returned by `process.show` (`doc-34` §6: 512 KiB).
const SHOW_CHUNK_MAX_BYTES: usize = 512 * 1024;

fn kind_to_wire(k: DocumentKind) -> DocumentKindWire {
    match k {
        DocumentKind::Process => DocumentKindWire::Process,
        DocumentKind::Standard => DocumentKindWire::Standard,
    }
}

fn status_to_wire(s: DocumentStatus) -> DocumentStatusWire {
    match s {
        DocumentStatus::Draft => DocumentStatusWire::Draft,
        DocumentStatus::Current => DocumentStatusWire::Current,
        DocumentStatus::Deprecated => DocumentStatusWire::Deprecated,
    }
}

fn by_parts(by: &By) -> (String, Option<String>) {
    match by {
        By::Employee(id) => ("employee".to_owned(), Some(id.as_str().to_owned())),
        By::System => ("system".to_owned(), None),
        By::Unknown => ("unknown".to_owned(), None),
    }
}

pub(super) fn doc_path_str(doc: &StructureDocument, team_chains: &BTreeMap<Id, Vec<Id>>) -> String {
    let chain = doc
        .team_id
        .as_ref()
        .and_then(|tid| team_chains.get(tid))
        .map(Vec::as_slice);
    structure_document_path(doc.kind, &doc.id, chain, doc.project_id.as_ref())
        .map(|p| p.as_str().to_owned())
        .unwrap_or_else(|_| format!("state/{}.md", doc.id.as_str()))
}

pub(super) fn doc_to_wire(
    doc: &StructureDocument,
    team_chains: &BTreeMap<Id, Vec<Id>>,
    scope_names: &BTreeMap<Scope, String>,
    emp_names: &BTreeMap<String, String>,
) -> ProcessDocumentWire {
    let scope = doc.scope();
    let (cb_kind, cb_id) = by_parts(&doc.meta.created_by);
    let (ub_kind, ub_id) = by_parts(&doc.meta.updated_by);
    let owner_id_str = doc
        .owner_employee_id
        .as_ref()
        .map(|i| i.as_str().to_owned());
    let owner_name = owner_id_str
        .as_deref()
        .and_then(|id| emp_names.get(id))
        .cloned();
    let scope_name = match &scope {
        Scope::Organization => None,
        other => scope_names.get(other).cloned(),
    };
    ProcessDocumentWire {
        id: doc.id.as_str().to_owned(),
        key: doc.key.clone(),
        revision: doc.meta.revision,
        kind: kind_to_wire(doc.kind),
        title: doc.title.clone(),
        status: status_to_wire(doc.status),
        owner_employee_id: owner_id_str,
        owner_name,
        level: scope.level_str().to_owned(),
        team_id: doc.team_id.as_ref().map(|i| i.as_str().to_owned()),
        project_id: doc.project_id.as_ref().map(|i| i.as_str().to_owned()),
        scope_name,
        overridable: doc.overridable,
        summary: summarize_body(&doc.body_markdown),
        body_markdown: doc.body_markdown.clone(),
        path: doc_path_str(doc, team_chains),
        created_at: doc.meta.created_at.clone(),
        created_by_kind: cb_kind,
        created_by_id: cb_id,
        updated_at: doc.meta.updated_at.clone(),
        updated_by_kind: ub_kind,
        updated_by_id: ub_id,
    }
}

pub(super) fn layer_to_wire(
    layer: &Layer,
    by_id: &BTreeMap<String, ProcessDocumentWire>,
) -> Option<ProcessLayerWire> {
    let document = by_id.get(&layer.doc.id)?.clone();
    let (outcome, detail) = match &layer.outcome {
        LayerOutcome::Applies => ("applies".to_owned(), None),
        LayerOutcome::Replaced => ("replaced".to_owned(), None),
        LayerOutcome::Refused {
            sealed_by_name,
            sealed_by_title,
            ..
        } => (
            "refused".to_owned(),
            Some(format!(
                "sealed (non-overridable) by {sealed_by_name}: {sealed_by_title}"
            )),
        ),
        LayerOutcome::Invalid { field } => (
            "invalid".to_owned(),
            Some(format!("invalid field `{field}`")),
        ),
        LayerOutcome::Clash { scope_name } => (
            "clash".to_owned(),
            Some(format!("duplicate key clash in {scope_name}")),
        ),
    };
    Some(ProcessLayerWire {
        document,
        outcome,
        detail,
    })
}

pub(super) fn entry_to_wire(
    entry: &EffectiveEntry,
    by_id: &BTreeMap<String, ProcessDocumentWire>,
) -> Option<EffectiveProcessEntryWire> {
    let chain: Vec<ProcessLayerWire> = entry
        .layers
        .iter()
        .filter_map(|l| layer_to_wire(l, by_id))
        .collect();
    let (applies, mandatory, winner_kind, document, replaces) = match &entry.winner {
        Some(w) => {
            let doc = by_id.get(&w.doc.id)?.clone();
            let rep = w.replaces.as_ref().and_then(|r| by_id.get(&r.id)).cloned();
            let wk = match w.kind {
                WinnerKind::Inherited => "inherited",
                WinnerKind::Override => "override",
                WinnerKind::Addition => "addition",
            };
            (true, !w.doc.overridable, Some(wk.to_owned()), doc, rep)
        }
        None => {
            let first = chain.first()?.document.clone();
            (false, false, None, first, None)
        }
    };
    let problem = chain.iter().find_map(|l| {
        (!matches!(l.outcome.as_str(), "applies" | "replaced"))
            .then(|| l.detail.clone())
            .flatten()
    });
    Some(EffectiveProcessEntryWire {
        id: entry.entry_id.clone(),
        key: entry.key.as_ref().map(|k| k.as_str().to_owned()),
        kind: kind_to_wire(entry.kind),
        applies,
        mandatory,
        winner_kind,
        document,
        replaces,
        problem,
        chain,
    })
}

pub(super) fn metadata_only(mut doc: ProcessDocumentWire) -> ProcessDocumentWire {
    doc.body_markdown.clear();
    doc
}

fn json_char_encoded_len(ch: char) -> usize {
    match ch {
        '"' | '\\' | '\x08' | '\x0c' | '\n' | '\r' | '\t' => 2,
        c if (c as u32) < 0x20 => 6,
        c => c.len_utf8(),
    }
}

pub(super) fn slice_utf8_chunk(
    text: &str,
    offset: usize,
    max_json_str_bytes: usize,
) -> Result<(String, Option<usize>), ApiError> {
    let total = text.len();
    if offset > total {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("offset {offset} exceeds document body length ({total} bytes)"),
        ));
    }
    if offset == total {
        return Ok((String::new(), None));
    }
    if !text.is_char_boundary(offset) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("offset {offset} does not fall on a UTF-8 character boundary"),
        ));
    }
    let start = offset;
    let raw_cap = (start + SHOW_CHUNK_MAX_BYTES).min(total);
    let mut end = start;
    let mut json_len = 2usize; // opening and closing JSON quotes
    for ch in text[start..].chars() {
        let ch_raw = ch.len_utf8();
        let ch_json = json_char_encoded_len(ch);
        if end > start && (end + ch_raw > raw_cap || json_len + ch_json > max_json_str_bytes) {
            break;
        }
        end += ch_raw;
        json_len += ch_json;
    }
    let next = (end < total).then_some(end);
    Ok((text[start..end].to_owned(), next))
}
