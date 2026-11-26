//! High-level Office facade for organization processes, standards, single-chain resolution,
//! launch instruction rendering, and built-in product skills (`TASK-44`, `C5`, `doc-34`).

use std::collections::{BTreeMap, BTreeSet};

use org_process::{
    find_builtin_skill, list_builtin_skills, render_instructions, resolve_single_chain,
    RenderInput, Rendered, SingleChainInputs, OVERRIDABLE_CAP_BYTES,
};
use org_proto::processes::{
    ProcessWriteParams, ProcessWriteResult, SkillListResult, SkillReadResult, SkillSummaryWire,
};
use org_proto::{ApiError, ErrorCode};

use crate::error::OfficeError;
use crate::store::open::process_ops::{
    LaunchProcessSnapshotRead, ProcessSnapshotRead, WriteStructureDocumentOp,
};

use super::{storage, Office};

mod chain;
mod effective;
mod show;
mod wire;

use chain::{build_scope_names, build_team_chains};
use wire::{doc_path_str, doc_to_wire};

impl Office {
    /// `process.write` (`doc-34` §5 & §6, `FC-112`, `FC-184`): creates or updates a process/standard.
    pub fn process_write(
        &self,
        caller: &str,
        params: ProcessWriteParams,
    ) -> Result<Result<ProcessWriteResult, ApiError>, OfficeError> {
        let (doc_id, revision) = match self
            .store
            .run(WriteStructureDocumentOp {
                actor: caller.to_owned(),
                owner_id: self.owner.as_str().to_owned(),
                params,
            })
            .map_err(storage)?
        {
            Ok(pair) => pair,
            Err(e) => return Ok(Err(e)),
        };

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
        let scope_names = build_scope_names(&snap.teams, &snap.projects);
        let emp_names: BTreeMap<String, String> = snap
            .employees
            .iter()
            .map(|e| (e.id.as_str().to_owned(), e.name.clone()))
            .collect();
        let rec = snap
            .documents
            .iter()
            .find(|d| d.id.as_str() == doc_id)
            .ok_or_else(|| {
                OfficeError::Invalid("written structure document missing from projection")
            })?;
        let document = doc_to_wire(rec, &team_chains, &scope_names, &emp_names);
        Ok(Ok(ProcessWriteResult {
            document_id: doc_id,
            revision,
            document,
        }))
    }

    /// Renders effective single-chain processes and standards for agent session launch (`doc-34` §3.2).
    /// Returns `Err(ApiError::InvalidRequest)` if non-overridable documents alone exceed `total_budget_bytes`.
    pub fn render_agent_process_instructions(
        &self,
        employee_id: &str,
        project_ref: Option<&str>,
        task_ref: Option<&str>,
        read_blobs: BTreeMap<String, BTreeSet<String>>,
        total_budget_bytes: usize,
    ) -> Result<Result<Rendered, ApiError>, OfficeError> {
        let snap = match self
            .store
            .read(LaunchProcessSnapshotRead {
                employee_id: employee_id.to_owned(),
                project_ref: project_ref.map(ToOwned::to_owned),
                task_ref: task_ref.map(ToOwned::to_owned),
            })
            .map_err(storage)?
            .0
        {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        let team_chains = build_team_chains(&snap.teams);
        let scope_names = build_scope_names(&snap.teams, &snap.projects);

        let (team_chain, project_id) = if let Some(proj) = &snap.project {
            let chain = team_chains
                .get(&proj.owner_team_id)
                .map(|c| c.iter().map(|i| i.as_str().to_owned()).collect())
                .unwrap_or_else(|| vec![proj.owner_team_id.as_str().to_owned()]);
            (chain, Some(proj.id.as_str().to_owned()))
        } else if let Some(task) = &snap.task {
            // Projectless work resolves against the task's accountable `team_id` (`doc-34` §3.1).
            let acct = &task.team_id;
            let chain = team_chains
                .get(acct)
                .map(|c| c.iter().map(|i| i.as_str().to_owned()).collect())
                .unwrap_or_else(|| vec![acct.as_str().to_owned()]);
            (chain, None)
        } else {
            let chain = team_chains
                .get(&snap.employee.home_team_id)
                .map(|c| c.iter().map(|i| i.as_str().to_owned()).collect())
                .unwrap_or_else(|| vec![snap.employee.home_team_id.as_str().to_owned()]);
            (chain, None)
        };

        let set = resolve_single_chain(&SingleChainInputs {
            team_chain,
            project_id,
            documents: snap
                .documents
                .iter()
                .map(|d| d.to_process_input(doc_path_str(d, &team_chains)))
                .collect(),
            scope_names,
        });

        match render_instructions(
            &set,
            &RenderInput {
                read_blobs,
                overridable_cap_bytes: OVERRIDABLE_CAP_BYTES,
                total_budget_bytes,
            },
        ) {
            Ok(rendered) => Ok(Ok(rendered)),
            Err(err) => Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                err.to_string(),
            ))),
        }
    }

    /// `skill.list` (`doc-28` §3.5): lists built-in product skills.
    pub fn skill_list(&self) -> SkillListResult {
        SkillListResult {
            skills: list_builtin_skills()
                .iter()
                .map(|s| SkillSummaryWire {
                    key: s.key.to_owned(),
                    title: s.title.to_owned(),
                    description: s.description.to_owned(),
                    source: s.source.to_owned(),
                })
                .collect(),
        }
    }

    /// `skill.read` (`doc-28` §3.5): reads one built-in product skill by key.
    pub fn skill_read(&self, name: &str) -> Result<SkillReadResult, ApiError> {
        let Some(skill) = find_builtin_skill(name) else {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                format!("skill `{name}` not found"),
            ));
        };
        Ok(SkillReadResult {
            key: skill.key.to_owned(),
            title: skill.title.to_owned(),
            description: skill.description.to_owned(),
            source: skill.source.to_owned(),
            body_markdown: skill.body_markdown.to_owned(),
        })
    }
}

#[cfg(test)]
#[path = "processes_tests.rs"]
mod tests;
