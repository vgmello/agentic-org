//! The effective processes and standards of a target, and why each entry won.

use std::collections::BTreeMap;

use org_process::{resolve_single_chain, Scope, SingleChainInputs};
use org_proto::processes::{
    ProcessDocumentWire, ProcessEffectiveParams, ProcessEffectiveResult, ProcessForWire,
    ProcessWhyParams, ProcessWhyResult,
};
use org_proto::{ApiError, ErrorCode};

use crate::error::OfficeError;
use crate::office::{storage, Office};
use crate::store::open::process_ops::{ProcessSnapshot, ProcessSnapshotRead};
use crate::time::format_rfc3339_ms;

use super::chain::{build_scope_names, build_team_chains, kind_matches, resolve_chain_target};
use super::wire::{doc_path_str, doc_to_wire, entry_to_wire, layer_to_wire, metadata_only};

impl Office {
    pub(super) fn compute_effective_from_snapshot(
        snap: &ProcessSnapshot,
        team_chain: Vec<String>,
        project_id: Option<String>,
    ) -> (
        org_process::EffectiveSet,
        BTreeMap<String, ProcessDocumentWire>,
        BTreeMap<Scope, String>,
    ) {
        let team_chains = build_team_chains(&snap.teams);
        let scope_names = build_scope_names(&snap.teams, &snap.projects);
        let emp_names: BTreeMap<String, String> = snap
            .employees
            .iter()
            .map(|e| (e.id.as_str().to_owned(), e.name.clone()))
            .collect();
        let wire_by_id: BTreeMap<String, ProcessDocumentWire> = snap
            .documents
            .iter()
            .map(|d| {
                (
                    d.id.as_str().to_owned(),
                    doc_to_wire(d, &team_chains, &scope_names, &emp_names),
                )
            })
            .collect();
        let inputs = SingleChainInputs {
            team_chain,
            project_id,
            documents: snap
                .documents
                .iter()
                .map(|d| d.to_process_input(doc_path_str(d, &team_chains)))
                .collect(),
            scope_names: scope_names.clone(),
        };
        let set = resolve_single_chain(&inputs);
        (set, wire_by_id, scope_names)
    }

    /// `process.effective` (`doc-34` §6): resolves single-chain processes & standards.
    pub fn process_effective(
        &self,
        caller: &str,
        params: ProcessEffectiveParams,
    ) -> Result<Result<ProcessEffectiveResult, ApiError>, OfficeError> {
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
        let (team_chain, project_id) = match resolve_chain_target(
            &snap,
            &team_chains,
            params.employee_id.as_deref(),
            params.r#for.as_ref(),
        ) {
            Ok(pair) => pair,
            Err(e) => return Ok(Err(e)),
        };
        let (set, wire_by_id, scope_names) =
            Self::compute_effective_from_snapshot(&snap, team_chain, project_id);
        let scope_chain = set
            .chain
            .iter()
            .map(|s| {
                scope_names
                    .get(s)
                    .cloned()
                    .unwrap_or_else(|| s.level_str().to_owned())
            })
            .collect();
        let entries = set
            .entries
            .iter()
            .filter(|e| kind_matches(params.kind, e.kind))
            .filter_map(|e| entry_to_wire(e, &wire_by_id))
            .collect();
        let problems = set
            .problems
            .iter()
            .filter_map(|l| layer_to_wire(l, &wire_by_id))
            .collect();
        Ok(Ok(ProcessEffectiveResult {
            scope_chain,
            entries,
            problems,
            computed_at: format_rfc3339_ms(self.clock.now().0),
            stale_since: None,
        }))
    }

    /// `process.why` (`doc-34` §6, `PROC-8`): explains single-chain resolution for one key.
    pub fn process_why(
        &self,
        caller: &str,
        params: ProcessWhyParams,
    ) -> Result<Result<ProcessWhyResult, ApiError>, OfficeError> {
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
        let (team_chain, project_id) = match resolve_chain_target(
            &snap,
            &team_chains,
            params.employee_id.as_deref(),
            params.r#for.as_ref(),
        ) {
            Ok(pair) => pair,
            Err(e) => return Ok(Err(e)),
        };
        let (set, wire_by_id, _) =
            Self::compute_effective_from_snapshot(&snap, team_chain, project_id);
        let Some(entry) = set.find_by_key(&params.key) else {
            return Ok(Err(ApiError::new(
                ErrorCode::NotFound,
                format!("no process or standard found for key `{}`", params.key),
            )));
        };
        let chain = entry
            .layers
            .iter()
            .filter_map(|l| {
                let mut wire_layer = layer_to_wire(l, &wire_by_id)?;
                wire_layer.document = metadata_only(wire_layer.document);
                Some(wire_layer)
            })
            .collect();
        let winner = entry
            .winner
            .as_ref()
            .and_then(|w| wire_by_id.get(&w.doc.id))
            .cloned()
            .map(metadata_only);
        Ok(Ok(ProcessWhyResult {
            key: params.key,
            chain,
            winner,
        }))
    }
}
