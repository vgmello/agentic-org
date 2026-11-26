//! Showing and viewing one process document.

use std::collections::BTreeMap;

use org_process::{DocumentStatus, Scope};
use org_proto::processes::{
    ProcessDocumentWire, ProcessForWire, ProcessScopeWire, ProcessShowParams, ProcessShowResult,
    ProcessViewParams, ProcessViewResult,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{Id, Sha256};

use crate::error::OfficeError;
use crate::office::{storage, Office};
use crate::processes::records::{encode_structure_document, StructureDocument};
use crate::store::open::process_ops::{ProcessSnapshot, ProcessSnapshotRead};
use crate::time::format_rfc3339_ms;

use super::chain::{
    build_team_chains, document_own_chain, find_parent_for_doc, find_team_id, kind_matches,
    resolve_chain_target,
};
use super::wire::{entry_to_wire, layer_to_wire, metadata_only, slice_utf8_chunk};

impl Office {
    /// `process.show` (`doc-34` §6): returns a document's full text (chunked <= 512 KiB) and
    fn show_document_by_id<'a>(
        snap: &'a ProcessSnapshot,
        team_chains: &BTreeMap<Id, Vec<Id>>,
        params: &ProcessShowParams,
        doc_id: &str,
    ) -> Result<
        (
            &'a StructureDocument,
            ProcessDocumentWire,
            Option<ProcessDocumentWire>,
        ),
        ApiError,
    > {
        let rec = snap
            .documents
            .iter()
            .find(|d| d.id.as_str() == doc_id)
            .ok_or_else(|| {
                ApiError::new(
                    ErrorCode::NotFound,
                    format!("structure document `{doc_id}` not found"),
                )
            })?;
        let (doc_chain, doc_proj) = if params.r#for.is_some() {
            resolve_chain_target(
                snap,
                team_chains,
                params.employee_id.as_deref(),
                params.r#for.as_ref(),
            )?
        } else {
            document_own_chain(snap, team_chains, rec)
        };
        let (set, wire_by_id, _) = Self::compute_effective_from_snapshot(snap, doc_chain, doc_proj);
        let parent_doc = find_parent_for_doc(&set, &wire_by_id, rec);
        let wire = wire_by_id
            .get(rec.id.as_str())
            .expect("wire exists")
            .clone();
        Ok((rec, wire, parent_doc))
    }

    fn show_document_by_key<'a>(
        snap: &'a ProcessSnapshot,
        team_chains: &BTreeMap<Id, Vec<Id>>,
        params: &ProcessShowParams,
    ) -> Result<
        (
            &'a StructureDocument,
            ProcessDocumentWire,
            Option<ProcessDocumentWire>,
        ),
        ApiError,
    > {
        let (team_chain, project_id) = resolve_chain_target(
            snap,
            team_chains,
            params.employee_id.as_deref(),
            params.r#for.as_ref(),
        )?;
        let (set, wire_by_id, _) =
            Self::compute_effective_from_snapshot(snap, team_chain, project_id);
        let key = params.key.as_deref().unwrap_or("");
        let winner = set.winner_by_key_or_id(key).ok_or_else(|| {
            ApiError::new(
                ErrorCode::NotFound,
                format!("no active process or standard matches `{key}`"),
            )
        })?;
        let rec = snap
            .documents
            .iter()
            .find(|d| d.id.as_str() == winner.doc.id)
            .expect("winner exists in snapshot");
        let parent_doc = winner
            .replaces
            .as_ref()
            .and_then(|p| wire_by_id.get(&p.id))
            .cloned();
        let wire = wire_by_id
            .get(rec.id.as_str())
            .expect("wire exists")
            .clone();
        Ok((rec, wire, parent_doc))
    }

    /// `process.show` (`doc-34` §6, `PROC-6`): returns the document, body chunk, and
    /// optional parent document when overriding a higher scope.
    pub fn process_show(
        &self,
        caller: &str,
        params: ProcessShowParams,
    ) -> Result<Result<ProcessShowResult, ApiError>, OfficeError> {
        if params.key.is_none() && params.document_id.is_none() {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "provide either `key` or `document_id`",
            )));
        }
        if params.revision.is_some() && params.document_id.is_none() {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "`revision` requires `document_id` so continuation reads stay pinned to one document",
            )));
        }
        let task_ref = match &params.r#for {
            Some(ProcessForWire::Task { task_id }) => Some(task_id.clone()),
            _ => None,
        };
        let snap = match self
            .store
            .read(ProcessSnapshotRead {
                caller_id: caller.to_owned(),
                task_ref,
            })
            .map_err(storage)?
            .0
        {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        let team_chains = build_team_chains(&snap.teams);
        let resolved = match params.document_id.as_deref() {
            Some(doc_id) => Self::show_document_by_id(&snap, &team_chains, &params, doc_id),
            None => Self::show_document_by_key(&snap, &team_chains, &params),
        };
        let (doc_rec, wire, parent) = match resolved {
            Ok(v) => v,
            Err(e) => return Ok(Err(e)),
        };
        if let Some(expected_rev) = params.revision {
            if doc_rec.meta.revision != expected_rev {
                return Ok(Err(ApiError::new(
                    ErrorCode::Conflict,
                    format!(
                        "document `{}` changed from revision {expected_rev} to {}; restart reading from offset 0",
                        doc_rec.id.as_str(),
                        doc_rec.meta.revision
                    ),
                )));
            }
        }

        let encoded = encode_structure_document(doc_rec);
        let blob = Sha256::of(&encoded).to_hex();
        let total_bytes = doc_rec.body_markdown.len();
        let wire = metadata_only(wire);
        let parent = parent.map(metadata_only);
        let probe = ProcessShowResult {
            document: wire,
            blob,
            content: String::new(),
            total_bytes,
            next_offset: Some(total_bytes),
            parent,
        };
        let overhead = serde_json::to_vec(&probe)
            .map(|v| v.len())
            .unwrap_or(16 * 1024);
        // Reserve 4 KiB for JSON-RPC Response framing (`id`, `result`, newline) and safety margin.
        let max_json_str_bytes = org_proto::MAX_FRAME_BYTES
            .saturating_sub(overhead)
            .saturating_sub(4096)
            .max(64);
        let (content, next_offset) = match slice_utf8_chunk(
            &doc_rec.body_markdown,
            params.offset.unwrap_or(0),
            max_json_str_bytes,
        ) {
            Ok(pair) => pair,
            Err(e) => return Ok(Err(e)),
        };
        Ok(Ok(ProcessShowResult {
            content,
            next_offset,
            ..probe
        }))
    }

    /// `process.view` (`doc-34` §6, `PROC-9`, `PROC-10`): scoped catalog view for Settings > Processes.
    pub fn process_view(
        &self,
        caller: &str,
        params: ProcessViewParams,
    ) -> Result<Result<ProcessViewResult, ApiError>, OfficeError> {
        let snap = match self
            .store
            .read(ProcessSnapshotRead {
                caller_id: caller.to_owned(),
                task_ref: None,
            })
            .map_err(storage)?
            .0
        {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        let team_chains = build_team_chains(&snap.teams);
        let (target_scope, team_chain, project_id) = match &params.scope {
            ProcessScopeWire::Organization => (Scope::Organization, Vec::new(), None),
            ProcessScopeWire::Team { team_id } => {
                let tid = match find_team_id(&snap.teams, team_id) {
                    Ok(id) => id,
                    Err(e) => return Ok(Err(e)),
                };
                let Some(chain) = team_chains.get(&tid) else {
                    return Ok(Err(ApiError::new(
                        ErrorCode::NotFound,
                        format!("team `{team_id}` not found"),
                    )));
                };
                (
                    Scope::Team(tid.as_str().to_owned()),
                    chain.iter().map(|i| i.as_str().to_owned()).collect(),
                    None,
                )
            }
            ProcessScopeWire::Project { project_id } => {
                let Some(proj) = snap.projects.iter().find(|p| {
                    p.id.as_str() == project_id
                        || p.key().eq_ignore_ascii_case(project_id)
                        || p.slug.eq_ignore_ascii_case(project_id)
                }) else {
                    return Ok(Err(ApiError::new(
                        ErrorCode::NotFound,
                        format!("project `{project_id}` not found"),
                    )));
                };
                let chain = team_chains
                    .get(&proj.owner_team_id)
                    .map(|c| c.iter().map(|i| i.as_str().to_owned()).collect())
                    .unwrap_or_else(|| vec![proj.owner_team_id.as_str().to_owned()]);
                (
                    Scope::Project(proj.id.as_str().to_owned()),
                    chain,
                    Some(proj.id.as_str().to_owned()),
                )
            }
        };

        let (set, wire_by_id, scope_names) =
            Self::compute_effective_from_snapshot(&snap, team_chain, project_id);
        let scope_name = scope_names
            .get(&target_scope)
            .cloned()
            .unwrap_or_else(|| target_scope.level_str().to_owned());
        let rows = set
            .entries
            .iter()
            .filter(|e| kind_matches(params.kind, e.kind))
            .filter_map(|e| entry_to_wire(e, &wire_by_id))
            .collect();
        let inactive = snap
            .documents
            .iter()
            .filter(|d| {
                d.scope() == target_scope
                    && d.status != DocumentStatus::Current
                    && kind_matches(params.kind, d.kind)
            })
            .filter_map(|d| wire_by_id.get(d.id.as_str()).cloned())
            .collect();
        let problems = set
            .problems
            .iter()
            .filter_map(|l| layer_to_wire(l, &wire_by_id))
            .collect();

        Ok(Ok(ProcessViewResult {
            scope: params.scope,
            scope_name,
            rows,
            inactive,
            problems,
            computed_at: format_rfc3339_ms(self.clock.now().0),
            stale_since: None,
        }))
    }
}
