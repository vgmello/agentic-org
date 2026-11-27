//! High-level Office facade for organization processes, standards, single-chain resolution,
//! launch instruction rendering, and built-in product skills (`TASK-44`, `C5`, `doc-34`).

use std::collections::{BTreeMap, BTreeSet};

use org_process::{
    find_builtin_skill, list_builtin_skills_by_category, render_instructions, render_task_deltas,
    resolve_single_chain, EffectiveSet, RenderInput, Rendered, Scope, SingleChainInputs,
    SkillCategory, OVERRIDABLE_CAP_BYTES,
};
use org_proto::processes::{
    ProcessWriteParams, ProcessWriteResult, SkillCategoryWire, SkillListResult, SkillReadResult,
    SkillSummaryWire,
};
use org_proto::{ApiError, ErrorCode};
use org_store::Id;

use crate::error::OfficeError;
use crate::organization::records::Team;
use crate::processes::records::StructureDocument;
use crate::projects::records::Project;
use crate::store::open::process_ops::{
    LaunchProcessSnapshotRead, ProcessSnapshotRead, TaskProcessSnapshotRead,
    WriteStructureDocumentOp,
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
        let set = match self.resolve_agent_process_set(employee_id, project_ref, task_ref)? {
            Ok(set) => set,
            Err(e) => return Ok(Err(e)),
        };
        Ok(render_instructions(
            &set,
            &RenderInput {
                read_blobs,
                overridable_cap_bytes: OVERRIDABLE_CAP_BYTES,
                total_budget_bytes,
            },
        )
        .map_err(|err| ApiError::new(ErrorCode::InvalidRequest, err.to_string())))
    }

    /// What the tasks `task_refs` change about the agent's standing set (its home team chain,
    /// [`Self::render_agent_process_instructions`] without a task), rendered for the turn that
    /// names them (`doc-27` §1, `doc-34` §3.1). The standing set and every task's set come from
    /// one snapshot. `Ok(None)` when no task's chain changes anything; a task that no longer
    /// exists is skipped, but one whose project cannot be loaded refuses the turn (`NotFound`).
    /// The standing must-follow section and everything the turn adds share
    /// `total_budget_bytes`; over it the turn is refused like a launch (`doc-34` Decision 4).
    pub fn render_agent_task_process_delta(
        &self,
        employee_id: &str,
        task_refs: &[&str],
        total_budget_bytes: usize,
    ) -> Result<Result<Option<String>, ApiError>, OfficeError> {
        if task_refs.is_empty() {
            return Ok(Ok(None));
        }
        let snap = match self
            .store
            .read(TaskProcessSnapshotRead {
                employee_id: employee_id.to_owned(),
                task_refs: task_refs.iter().map(|t| (*t).to_owned()).collect(),
            })
            .map_err(storage)?
            .0
        {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        let chains = ChainSources::new(&snap.teams, &snap.projects, &snap.documents);
        let standing = chains.resolve(&snap.employee.home_team_id, None);
        let task_sets: Vec<(&str, EffectiveSet)> = snap
            .tasks
            .iter()
            .map(|(task_ref, task, project)| {
                let set = match project {
                    Some(proj) => chains.resolve(&proj.owner_team_id, Some(proj.id.as_str())),
                    None => chains.resolve(&task.team_id, None),
                };
                (task_ref.as_str(), set)
            })
            .collect();
        let tasks: Vec<(&str, &EffectiveSet)> =
            task_sets.iter().map(|(label, set)| (*label, set)).collect();
        let input = RenderInput {
            read_blobs: BTreeMap::new(),
            overridable_cap_bytes: OVERRIDABLE_CAP_BYTES,
            total_budget_bytes,
        };
        Ok(render_task_deltas(&standing, &tasks, &input)
            .map(|text| (!text.is_empty()).then_some(text))
            .map_err(|err| ApiError::new(ErrorCode::InvalidRequest, err.to_string())))
    }

    /// The single chain for an agent's work: the project's owning chain, else the task's
    /// accountable team chain, else the agent's home team chain (`doc-34` §3.1).
    fn resolve_agent_process_set(
        &self,
        employee_id: &str,
        project_ref: Option<&str>,
        task_ref: Option<&str>,
    ) -> Result<Result<EffectiveSet, ApiError>, OfficeError> {
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
        let chains = ChainSources::new(&snap.teams, &snap.projects, &snap.documents);
        Ok(Ok(if let Some(proj) = &snap.project {
            chains.resolve(&proj.owner_team_id, Some(proj.id.as_str()))
        } else if let Some(task) = &snap.task {
            // Projectless work resolves against the task's accountable `team_id` (`doc-34` §3.1).
            chains.resolve(&task.team_id, None)
        } else {
            chains.resolve(&snap.employee.home_team_id, None)
        }))
    }

    /// `skill.list` (`doc-28` §3.5): lists all built-in product skills.
    pub fn skill_list(&self) -> SkillListResult {
        self.skill_list_by_category(None)
    }

    /// `skill.list` filtered by optional wire [`SkillCategoryWire`] (`TASK-80.4`).
    pub fn skill_list_filtered(
        &self,
        category: Option<SkillCategoryWire>,
    ) -> Result<SkillListResult, ApiError> {
        let parsed = match category {
            None => None,
            Some(SkillCategoryWire::Workflow) => Some(SkillCategory::Workflow),
            Some(SkillCategoryWire::ProductGuide) => Some(SkillCategory::ProductGuide),
            Some(SkillCategoryWire::ChiefOfStaff) => Some(SkillCategory::ChiefOfStaff),
            Some(_) => {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "unknown skill category; expected `workflow`, `product_guide`, or `chief_of_staff`",
                ));
            }
        };
        Ok(self.skill_list_by_category(parsed))
    }

    /// `skill.list` filtered by optional [`SkillCategory`] (`TASK-80.4`).
    pub fn skill_list_by_category(&self, category: Option<SkillCategory>) -> SkillListResult {
        SkillListResult {
            skills: list_builtin_skills_by_category(category)
                .into_iter()
                .map(|s| SkillSummaryWire {
                    key: s.key.to_owned(),
                    title: s.title.to_owned(),
                    description: s.description.to_owned(),
                    source: s.source.to_owned(),
                    category: Some(skill_category_to_wire(s.category)),
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
            category: Some(skill_category_to_wire(skill.category)),
            body_markdown: skill.body_markdown.to_owned(),
        })
    }
}

fn skill_category_to_wire(category: SkillCategory) -> SkillCategoryWire {
    match category {
        SkillCategory::Workflow => SkillCategoryWire::Workflow,
        SkillCategory::ProductGuide => SkillCategoryWire::ProductGuide,
        SkillCategory::ChiefOfStaff => SkillCategoryWire::ChiefOfStaff,
    }
}

/// The teams, projects and documents of one snapshot, ready to resolve any single chain.
struct ChainSources<'a> {
    team_chains: BTreeMap<Id, Vec<Id>>,
    scope_names: BTreeMap<Scope, String>,
    documents: &'a [StructureDocument],
}

impl<'a> ChainSources<'a> {
    fn new(teams: &[Team], projects: &[Project], documents: &'a [StructureDocument]) -> Self {
        Self {
            team_chains: build_team_chains(teams),
            scope_names: build_scope_names(teams, projects),
            documents,
        }
    }

    /// The effective set for `team_id`'s chain (organization first), plus `project_id`'s layer.
    fn resolve(&self, team_id: &Id, project_id: Option<&str>) -> EffectiveSet {
        let team_chain = self
            .team_chains
            .get(team_id)
            .map(|c| c.iter().map(|i| i.as_str().to_owned()).collect())
            .unwrap_or_else(|| vec![team_id.as_str().to_owned()]);
        resolve_single_chain(&SingleChainInputs {
            team_chain,
            project_id: project_id.map(ToOwned::to_owned),
            documents: self
                .documents
                .iter()
                .map(|d| d.to_process_input(doc_path_str(d, &self.team_chains)))
                .collect(),
            scope_names: self.scope_names.clone(),
        })
    }
}

#[cfg(test)]
#[path = "processes_tests.rs"]
mod tests;
