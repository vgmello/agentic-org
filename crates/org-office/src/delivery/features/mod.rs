//! Thin `org-office` adapter bridging [`DeliveryRead`] + `project_docs::access::authorize`
//! into `org_docs::delivery::ops_*` pure decisions and rebuild validation.

use std::collections::BTreeSet;

use org_docs::delivery::ops::{
    CallerContext, DeliveryMutation, DeliveryOpError, DeliveryStoreView, TaskSummary,
};
use org_docs::delivery::records::{
    DeploymentRecord, EntityLinkRecord, FeatureFlagRecord, FeatureRecord, LinkEntityKind,
    MilestoneRecord, NumberCounterRecord, ReleaseRecord, ScopedChangelogRecord,
};
use org_proto::tasks::TaskStatus;
use org_proto::{ApiError, ErrorCode};
use org_store::{Ack, Change, DurablePath, EngineError, Id, Refusal, Sha256, Timestamp};

use crate::delivery::ports::DeliveryRead;
use crate::organization::records::time_text;
use crate::project_docs::access::{authorize, Denied, DocsAccess};

/// Adapts any [`DeliveryRead`] snapshot/tx to `org_docs::delivery::ops::DeliveryStoreView`.
pub struct StoreViewAdapter<'a, R: DeliveryRead>(pub &'a R);

fn parse_id(raw: &str) -> Result<Id, String> {
    Id::parse(raw).map_err(|_| format!("invalid id `{raw}`"))
}

fn map_err(e: EngineError) -> String {
    e.to_string()
}

impl<R: DeliveryRead> DeliveryStoreView for StoreViewAdapter<'_, R> {
    fn project_features(&self, project_id: &str) -> Result<Vec<FeatureRecord>, String> {
        self.0
            .project_features(&parse_id(project_id)?)
            .map_err(map_err)
    }
    fn feature(&self, id: &str) -> Result<Option<FeatureRecord>, String> {
        self.0.feature(&parse_id(id)?).map_err(map_err)
    }
    fn project_releases(&self, project_id: &str) -> Result<Vec<ReleaseRecord>, String> {
        self.0
            .project_releases(&parse_id(project_id)?)
            .map_err(map_err)
    }
    fn release(&self, id: &str) -> Result<Option<ReleaseRecord>, String> {
        self.0.release(&parse_id(id)?).map_err(map_err)
    }
    fn release_milestones(&self, release_id: &str) -> Result<Vec<MilestoneRecord>, String> {
        self.0
            .release_milestones(&parse_id(release_id)?)
            .map_err(map_err)
    }
    fn milestone(&self, id: &str) -> Result<Option<MilestoneRecord>, String> {
        self.0.milestone(&parse_id(id)?).map_err(map_err)
    }
    fn project_deployments(&self, project_id: &str) -> Result<Vec<DeploymentRecord>, String> {
        self.0
            .project_deployments(&parse_id(project_id)?)
            .map_err(map_err)
    }
    fn deployment(&self, id: &str) -> Result<Option<DeploymentRecord>, String> {
        self.0.deployment(&parse_id(id)?).map_err(map_err)
    }
    fn project_flags(&self, project_id: &str) -> Result<Vec<FeatureFlagRecord>, String> {
        self.0
            .project_flags(&parse_id(project_id)?)
            .map_err(map_err)
    }
    fn used_flag_ids(&self, project_id: &str) -> Result<BTreeSet<String>, String> {
        self.0
            .used_flag_ids(&parse_id(project_id)?)
            .map_err(map_err)
    }
    fn release_changelog(&self, release_id: &str) -> Result<Option<ScopedChangelogRecord>, String> {
        self.0
            .release_changelog(&parse_id(release_id)?)
            .map_err(map_err)
    }
    fn deployment_changelog(
        &self,
        deployment_id: &str,
    ) -> Result<Option<ScopedChangelogRecord>, String> {
        self.0
            .deployment_changelog(&parse_id(deployment_id)?)
            .map_err(map_err)
    }
    fn project_links(&self, project_id: &str) -> Result<Vec<EntityLinkRecord>, String> {
        self.0
            .project_links(&parse_id(project_id)?)
            .map_err(map_err)
    }
    fn counter(
        &self,
        scope_kind: &str,
        scope_id: &str,
        entity_kind: &str,
    ) -> Result<Option<NumberCounterRecord>, String> {
        self.0
            .delivery_counter(scope_kind, &parse_id(scope_id)?, entity_kind)
            .map_err(map_err)
    }
    fn employee_exists(&self, id: &str) -> Result<bool, String> {
        Ok(self.0.employee(&parse_id(id)?).map_err(map_err)?.is_some())
    }
    fn repository_exists(&self, id: &str) -> Result<bool, String> {
        let rid = parse_id(id)?;
        Ok(self
            .0
            .repositories()
            .map_err(map_err)?
            .into_iter()
            .any(|r| r.id == rid))
    }
    fn project_tasks(&self, project_id: &str) -> Result<Vec<TaskSummary>, String> {
        let pid = parse_id(project_id)?;
        Ok(self
            .0
            .tasks()
            .map_err(map_err)?
            .into_iter()
            .filter(|t| t.project_id.as_ref() == Some(&pid))
            .map(|t| TaskSummary {
                id: t.id.to_string(),
                project_id: t.project_id.map(|id| id.to_string()),
                feature_id: t.feature_id.map(|id| id.to_string()),
                milestone_id: t.milestone_id.map(|id| id.to_string()),
                affects_feature_id: t
                    .details
                    .and_then(|d| d.affects_feature_id)
                    .map(|id| id.to_string()),
                is_done: matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled),
            })
            .collect())
    }
    fn entity_exists_in_project(
        &self,
        project_id: &str,
        kind: LinkEntityKind,
        id: &str,
    ) -> Result<bool, String> {
        let pid = parse_id(project_id)?;
        let eid = parse_id(id)?;
        Ok(match kind {
            LinkEntityKind::Project => eid == pid,
            LinkEntityKind::Feature => self
                .0
                .feature(&eid)
                .map_err(map_err)?
                .is_some_and(|f| f.project_id == project_id),
            LinkEntityKind::Release => self
                .0
                .release(&eid)
                .map_err(map_err)?
                .is_some_and(|r| r.project_id == project_id),
            LinkEntityKind::Milestone => self
                .0
                .milestone(&eid)
                .map_err(map_err)?
                .is_some_and(|m| m.project_id == project_id),
            LinkEntityKind::Deployment => self
                .0
                .deployment(&eid)
                .map_err(map_err)?
                .is_some_and(|d| d.project_id == project_id),
            LinkEntityKind::Task => self
                .0
                .task(&eid)
                .map_err(map_err)?
                .is_some_and(|t| t.project_id.as_ref() == Some(&pid)),
            LinkEntityKind::Document => {
                if self
                    .0
                    .project_doc(&eid)
                    .map_err(map_err)?
                    .is_some_and(|d| d.project_id == pid)
                {
                    true
                } else {
                    let rel_hit = self
                        .0
                        .project_releases(&pid)
                        .map_err(map_err)?
                        .into_iter()
                        .any(|r| {
                            Id::parse(&r.id)
                                .ok()
                                .and_then(|rid| self.0.release_changelog(&rid).ok().flatten())
                                .is_some_and(|c| c.id == id)
                        });
                    rel_hit
                        || self
                            .0
                            .project_deployments(&pid)
                            .map_err(map_err)?
                            .into_iter()
                            .any(|d| {
                                Id::parse(&d.id)
                                    .ok()
                                    .and_then(|did| {
                                        self.0.deployment_changelog(&did).ok().flatten()
                                    })
                                    .is_some_and(|c| c.id == id)
                            })
                }
            }
            LinkEntityKind::DevelopmentProcess => self
                .0
                .structure_document(&eid)
                .map_err(map_err)?
                .is_some_and(|d| d.project_id.is_none() || d.project_id.as_ref() == Some(&pid)),
            LinkEntityKind::Media => self
                .0
                .project_doc_media(&pid)
                .map_err(map_err)?
                .iter()
                .any(|m| m.id == eid),
        })
    }
    fn derive_uuid_v7(&self, domain: &str, command_id: &str, now_ms: u64) -> String {
        let digest = Sha256::of_parts(&[domain.as_bytes(), b"\0", command_id.as_bytes()]);
        let mut b = [0u8; 16];
        b[..6].copy_from_slice(&now_ms.to_be_bytes()[2..8]);
        b[6..16].copy_from_slice(&digest.as_bytes()[..10]);
        b[6] = (b[6] & 0x0f) | 0x70;
        b[8] = (b[8] & 0x3f) | 0x80;
        let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
        format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        )
    }
}

fn map_op_err(err: DeliveryOpError) -> Denied {
    match err {
        DeliveryOpError::Refused(api) => Denied::Refused(api),
        DeliveryOpError::Storage(msg) => Denied::Storage(EngineError::new(msg)),
    }
}

/// Runs one authorized delivery read operation.
pub fn run_read<R: DeliveryRead, T>(
    view: &R,
    caller: &str,
    project: &str,
    f: impl FnOnce(&StoreViewAdapter<'_, R>, &CallerContext<'_>) -> Result<T, DeliveryOpError>,
) -> Result<T, Denied> {
    let cleared = authorize(view, caller, project, DocsAccess::Read)?;
    let ctx = CallerContext {
        caller_id: cleared.caller.id.as_str(),
        project_id: cleared.project.id.as_str(),
        project_slug: &cleared.project.slug,
        now_rfc3339: "",
        now_ms: 0,
    };
    f(&StoreViewAdapter(view), &ctx).map_err(map_op_err)
}

/// Runs one authorized delivery write decision and converts [`DeliveryMutation`] into a [`Change`].
pub fn run_write<R: DeliveryRead>(
    view: &R,
    now: Timestamp,
    caller: &str,
    project: &str,
    f: impl FnOnce(
        &StoreViewAdapter<'_, R>,
        &CallerContext<'_>,
    ) -> Result<DeliveryMutation, DeliveryOpError>,
) -> Result<Change, Denied> {
    let cleared = authorize(view, caller, project, DocsAccess::Write)?;
    let now_rfc3339 = time_text(now);
    let ctx = CallerContext {
        caller_id: cleared.caller.id.as_str(),
        project_id: cleared.project.id.as_str(),
        project_slug: &cleared.project.slug,
        now_rfc3339: &now_rfc3339,
        now_ms: now.0,
    };
    let mutation = f(&StoreViewAdapter(view), &ctx).map_err(map_op_err)?;
    let mut change = Change::new(Ack::Durable);
    for (p, bytes) in mutation.puts {
        let dp = DurablePath::parse(&p).expect("validated delivery durable path");
        change = change.put(dp, bytes);
    }
    for p in mutation.removes {
        let dp = DurablePath::parse(&p).expect("validated delivery durable path");
        change = change.remove(dp);
    }
    Ok(change.reply(mutation.reply))
}

/// Converts a [`Denied`] into an `org_store::Refusal`.
pub fn refusal(denied: Denied) -> Refusal {
    match denied {
        Denied::Refused(error) => Refusal::rejected(serde_json::to_vec(&error).unwrap_or_default()),
        Denied::Storage(error) => Refusal::from(error),
    }
}

/// Decodes a rejected delivery receipt reply back into [`ApiError`].
pub fn decode_rejection(bytes: &[u8]) -> ApiError {
    serde_json::from_slice(bytes).unwrap_or_else(|_| {
        ApiError::new(
            ErrorCode::StorageError,
            "delivery write was rejected without detail",
        )
    })
}
