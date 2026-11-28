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
use org_store::{Ack, Change, DurablePath, EngineError, Id, Sha256, Timestamp};

use crate::delivery::ports::DeliveryRead;
use crate::organization::records::{time_text, EmployeeStatus};
use crate::project_docs::access::{authorize, Denied, DocsAccess};

/// Adapts any [`DeliveryRead`] snapshot/tx to `org_docs::delivery::ops::DeliveryStoreView`.
pub struct StoreViewAdapter<'a, R: DeliveryRead>(pub &'a R);

fn parse_id(raw: &str) -> Result<Id, String> {
    Id::parse(raw).map_err(|_| format!("invalid id `{raw}`"))
}

fn map_err(e: EngineError) -> String {
    e.to_string()
}

macro_rules! fwd_id_read {
    ($($method:ident -> $ret:ty;)*) => {$(
        fn $method(&self, id: &str) -> Result<$ret, String> {
            self.0.$method(&parse_id(id)?).map_err(map_err)
        }
    )*};
}

impl<R: DeliveryRead> DeliveryStoreView for StoreViewAdapter<'_, R> {
    fwd_id_read! {
        project_features -> Vec<FeatureRecord>;
        feature -> Option<FeatureRecord>;
        project_releases -> Vec<ReleaseRecord>;
        release -> Option<ReleaseRecord>;
        release_milestones -> Vec<MilestoneRecord>;
        project_milestones -> Vec<MilestoneRecord>;
        milestone -> Option<MilestoneRecord>;
        project_deployments -> Vec<DeploymentRecord>;
        deployment -> Option<DeploymentRecord>;
        project_flags -> Vec<FeatureFlagRecord>;
        used_flag_ids -> BTreeSet<String>;
        release_changelog -> Option<ScopedChangelogRecord>;
        deployment_changelog -> Option<ScopedChangelogRecord>;
        project_changelogs -> Vec<ScopedChangelogRecord>;
        project_links -> Vec<EntityLinkRecord>;
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
    fn employee_is_active(&self, id: &str) -> Result<bool, String> {
        Ok(self
            .0
            .employee(&parse_id(id)?)
            .map_err(map_err)?
            .is_some_and(|e| e.status == EmployeeStatus::Active))
    }
    fn repository_exists(&self, id: &str) -> Result<bool, String> {
        let rid = parse_id(id)?;
        Ok(self
            .0
            .repositories()
            .map_err(map_err)?
            .iter()
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
        let (pid, eid) = (parse_id(project_id)?, parse_id(id)?);
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
                self.0
                    .project_doc(&eid)
                    .map_err(map_err)?
                    .is_some_and(|d| d.project_id == pid)
                    || self
                        .0
                        .project_changelogs(&pid)
                        .map_err(map_err)?
                        .iter()
                        .any(|c| c.id == id)
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
        let dp = DurablePath::parse(&p).map_err(|e| {
            Denied::Storage(EngineError::new(format!(
                "invalid delivery durable path `{p}`: {e}"
            )))
        })?;
        change = change.put(dp, bytes);
    }
    for p in mutation.removes {
        let dp = DurablePath::parse(&p).map_err(|e| {
            Denied::Storage(EngineError::new(format!(
                "invalid delivery durable path `{p}`: {e}"
            )))
        })?;
        change = change.remove(dp);
    }
    Ok(change.reply(mutation.reply))
}
