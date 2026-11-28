//! Memory engine projection tables for the delivery family (`MemoryTx` / `MemorySnapshot`).

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};

use org_docs::delivery::records::{
    DeploymentRecord, EntityLinkRecord, FeatureFlagRecord, FeatureRecord, LinkEntityKind,
    MilestoneRecord, NumberCounterRecord, ReleaseRecord, ScopedChangelogRecord,
};
use org_store::testkit::{FamilyTable, MemorySnapshot, MemoryTx};
use org_store::{EngineError, Id};

use super::ports::{DeliveryRead, DeliveryWrite};

#[derive(Clone, Default, Debug)]
pub struct DeliveryTables {
    pub features: BTreeMap<String, FeatureRecord>,
    pub releases: BTreeMap<String, ReleaseRecord>,
    pub milestones: BTreeMap<String, MilestoneRecord>,
    pub deployments: BTreeMap<String, DeploymentRecord>,
    pub flags: BTreeMap<String, FeatureFlagRecord>,
    pub release_changelogs: BTreeMap<String, ScopedChangelogRecord>,
    pub deployment_changelogs: BTreeMap<String, ScopedChangelogRecord>,
    pub links: BTreeMap<(LinkEntityKind, String, String), EntityLinkRecord>,
    pub counters: BTreeMap<(String, String, String), NumberCounterRecord>,
}

impl FamilyTable for DeliveryTables {
    fn name(&self) -> &'static str {
        "delivery"
    }

    fn clone_box(&self) -> Box<dyn FamilyTable> {
        Box::new(self.clone())
    }

    fn dump(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for f in self.features.values() {
            lines.push(format!("features {} {f:?}", f.id));
        }
        for r in self.releases.values() {
            lines.push(format!("releases {} {r:?}", r.id));
        }
        for m in self.milestones.values() {
            lines.push(format!("milestones {} {m:?}", m.id));
        }
        for d in self.deployments.values() {
            lines.push(format!("deployments {} {d:?}", d.id));
        }
        for fl in self.flags.values() {
            lines.push(format!("feature_flags {} {fl:?}", fl.id));
        }
        for c in self
            .release_changelogs
            .values()
            .chain(self.deployment_changelogs.values())
        {
            lines.push(format!("scoped_changelogs {} {c:?}", c.id));
        }
        for l in self.links.values() {
            lines.push(format!("delivery_links {} {l:?}", l.id));
        }
        for ((sk, sid, ek), c) in &self.counters {
            lines.push(format!("delivery_counters {sk}:{sid}:{ek} {c:?}"));
        }
        lines.sort();
        lines
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn read<R: Default>(
    tables: Option<&DeliveryTables>,
    f: impl FnOnce(&DeliveryTables) -> R,
) -> Result<R, EngineError> {
    Ok(tables.map(f).unwrap_or_default())
}

macro_rules! memory_delivery_read {
    ($($view:ty),+) => {$(
        impl DeliveryRead for $view {
            fn project_features(&self, project: &Id) -> Result<Vec<FeatureRecord>, EngineError> {
                read(self.table(), |t| {
                    let mut out: Vec<_> = t.features.values()
                        .filter(|f| f.project_id == project.as_str())
                        .cloned().collect();
                    out.sort_by(|a, b| a.number.cmp(&b.number).then(a.id.cmp(&b.id)));
                    out
                })
            }
            fn feature(&self, id: &Id) -> Result<Option<FeatureRecord>, EngineError> {
                read(self.table(), |t| t.features.get(id.as_str()).cloned())
            }
            fn all_features(&self) -> Result<Vec<FeatureRecord>, EngineError> {
                read(self.table(), |t| {
                    let mut out: Vec<_> = t.features.values().cloned().collect();
                    out.sort_by(|a, b| a.project_id.cmp(&b.project_id).then(a.number.cmp(&b.number)));
                    out
                })
            }
            fn project_releases(&self, project: &Id) -> Result<Vec<ReleaseRecord>, EngineError> {
                read(self.table(), |t| {
                    let mut out: Vec<_> = t.releases.values()
                        .filter(|r| r.project_id == project.as_str())
                        .cloned().collect();
                    out.sort_by(|a, b| a.number.cmp(&b.number).then(a.id.cmp(&b.id)));
                    out
                })
            }
            fn release(&self, id: &Id) -> Result<Option<ReleaseRecord>, EngineError> {
                read(self.table(), |t| t.releases.get(id.as_str()).cloned())
            }
            fn release_milestones(&self, release: &Id) -> Result<Vec<MilestoneRecord>, EngineError> {
                read(self.table(), |t| {
                    let mut out: Vec<_> = t.milestones.values()
                        .filter(|m| m.release_id == release.as_str())
                        .cloned().collect();
                    out.sort_by(|a, b| a.number.cmp(&b.number).then(a.id.cmp(&b.id)));
                    out
                })
            }
            fn project_milestones(&self, project: &Id) -> Result<Vec<MilestoneRecord>, EngineError> {
                read(self.table(), |t| {
                    let mut out: Vec<_> = t.milestones.values()
                        .filter(|m| m.project_id == project.as_str())
                        .cloned().collect();
                    out.sort_by(|a, b| {
                        a.release_id
                            .cmp(&b.release_id)
                            .then(a.number.cmp(&b.number))
                            .then(a.id.cmp(&b.id))
                    });
                    out
                })
            }
            fn milestone(&self, id: &Id) -> Result<Option<MilestoneRecord>, EngineError> {
                read(self.table(), |t| t.milestones.get(id.as_str()).cloned())
            }
            fn project_deployments(&self, project: &Id) -> Result<Vec<DeploymentRecord>, EngineError> {
                read(self.table(), |t| {
                    let mut out: Vec<_> = t.deployments.values()
                        .filter(|d| d.project_id == project.as_str())
                        .cloned().collect();
                    out.sort_by(|a, b| a.id.cmp(&b.id));
                    out
                })
            }
            fn deployment(&self, id: &Id) -> Result<Option<DeploymentRecord>, EngineError> {
                read(self.table(), |t| t.deployments.get(id.as_str()).cloned())
            }
            fn project_flags(&self, project: &Id) -> Result<Vec<FeatureFlagRecord>, EngineError> {
                read(self.table(), |t| {
                    let mut out: Vec<_> = t.flags.values()
                        .filter(|f| f.project_id == project.as_str())
                        .cloned().collect();
                    out.sort_by(|a, b| a.key.cmp(&b.key).then(a.id.cmp(&b.id)));
                    out
                })
            }
            fn flag(&self, id: &Id) -> Result<Option<FeatureFlagRecord>, EngineError> {
                read(self.table(), |t| t.flags.get(id.as_str()).cloned())
            }
            fn used_flag_ids(&self, project: &Id) -> Result<BTreeSet<String>, EngineError> {
                read(self.table(), |t| {
                    let mut used = BTreeSet::new();
                    for r in t.releases.values().filter(|r| r.project_id == project.as_str()) {
                        for s in &r.flag_settings {
                            used.insert(s.flag_id.clone());
                        }
                    }
                    for d in t.deployments.values().filter(|d| d.project_id == project.as_str()) {
                        for s in &d.flag_settings {
                            used.insert(s.flag_id.clone());
                        }
                    }
                    used
                })
            }
            fn release_changelog(&self, release: &Id) -> Result<Option<ScopedChangelogRecord>, EngineError> {
                read(self.table(), |t| t.release_changelogs.get(release.as_str()).cloned())
            }
            fn deployment_changelog(&self, deployment: &Id) -> Result<Option<ScopedChangelogRecord>, EngineError> {
                read(self.table(), |t| t.deployment_changelogs.get(deployment.as_str()).cloned())
            }
            fn project_changelogs(&self, project: &Id) -> Result<Vec<ScopedChangelogRecord>, EngineError> {
                read(self.table(), |t| {
                    let mut out: Vec<_> = t.release_changelogs.values()
                        .chain(t.deployment_changelogs.values())
                        .filter(|c| c.project_id == project.as_str())
                        .cloned()
                        .collect();
                    out.sort_by(|a, b| a.id.cmp(&b.id));
                    out
                })
            }
            fn project_links(&self, project: &Id) -> Result<Vec<EntityLinkRecord>, EngineError> {
                read(self.table(), |t| {
                    let mut out: Vec<_> = t.links.values()
                        .filter(|l| l.project_id == project.as_str())
                        .cloned().collect();
                    out.sort();
                    out
                })
            }
            fn delivery_counter(
                &self,
                scope_kind: &str,
                scope_id: &Id,
                entity_kind: &str,
            ) -> Result<Option<NumberCounterRecord>, EngineError> {
                read(self.table(), |t| {
                    t.counters.get(&(scope_kind.to_owned(), scope_id.to_string(), entity_kind.to_owned())).cloned()
                })
            }
            fn projects_with_delivery(&self) -> Result<Vec<Id>, EngineError> {
                read(self.table(), |t: &DeliveryTables| {
                    let mut ids = BTreeSet::new();
                    for p in t.features.values().map(|x| &x.project_id)
                        .chain(t.releases.values().map(|x| &x.project_id))
                        .chain(t.milestones.values().map(|x| &x.project_id))
                        .chain(t.deployments.values().map(|x| &x.project_id))
                        .chain(t.flags.values().map(|x| &x.project_id))
                        .chain(t.release_changelogs.values().map(|x| &x.project_id))
                        .chain(t.deployment_changelogs.values().map(|x| &x.project_id))
                        .chain(t.links.values().map(|x| &x.project_id))
                    {
                        if let Ok(id) = Id::parse(p) {
                            ids.insert(id);
                        }
                    }
                    ids.into_iter().collect()
                })
            }
        }
    )+};
}

memory_delivery_read!(MemoryTx, MemorySnapshot);

impl DeliveryWrite for MemoryTx {
    fn put_feature(&mut self, record: &FeatureRecord) -> Result<(), EngineError> {
        self.table_mut::<DeliveryTables>()?
            .features
            .insert(record.id.clone(), record.clone());
        Ok(())
    }
    fn remove_feature(&mut self, id: &Id) -> Result<(), EngineError> {
        self.table_mut::<DeliveryTables>()?
            .features
            .remove(id.as_str());
        Ok(())
    }
    fn put_release(&mut self, record: &ReleaseRecord) -> Result<(), EngineError> {
        self.table_mut::<DeliveryTables>()?
            .releases
            .insert(record.id.clone(), record.clone());
        Ok(())
    }
    fn remove_release(&mut self, id: &Id) -> Result<(), EngineError> {
        self.table_mut::<DeliveryTables>()?
            .releases
            .remove(id.as_str());
        Ok(())
    }
    fn put_milestone(&mut self, record: &MilestoneRecord) -> Result<(), EngineError> {
        self.table_mut::<DeliveryTables>()?
            .milestones
            .insert(record.id.clone(), record.clone());
        Ok(())
    }
    fn remove_milestone(&mut self, id: &Id) -> Result<(), EngineError> {
        self.table_mut::<DeliveryTables>()?
            .milestones
            .remove(id.as_str());
        Ok(())
    }
    fn put_deployment(&mut self, record: &DeploymentRecord) -> Result<(), EngineError> {
        self.table_mut::<DeliveryTables>()?
            .deployments
            .insert(record.id.clone(), record.clone());
        Ok(())
    }
    fn remove_deployment(&mut self, id: &Id) -> Result<(), EngineError> {
        self.table_mut::<DeliveryTables>()?
            .deployments
            .remove(id.as_str());
        Ok(())
    }
    fn put_flag(&mut self, record: &FeatureFlagRecord) -> Result<(), EngineError> {
        self.table_mut::<DeliveryTables>()?
            .flags
            .insert(record.id.clone(), record.clone());
        Ok(())
    }
    fn remove_flag(&mut self, id: &Id) -> Result<(), EngineError> {
        self.table_mut::<DeliveryTables>()?
            .flags
            .remove(id.as_str());
        Ok(())
    }
    fn put_scoped_changelog(&mut self, record: &ScopedChangelogRecord) -> Result<(), EngineError> {
        let t = self.table_mut::<DeliveryTables>()?;
        if let Some(ref rid) = record.release_id {
            t.release_changelogs.insert(rid.clone(), record.clone());
        } else if let Some(ref did) = record.deployment_id {
            t.deployment_changelogs.insert(did.clone(), record.clone());
        }
        Ok(())
    }
    fn remove_release_changelog(&mut self, release: &Id) -> Result<(), EngineError> {
        self.table_mut::<DeliveryTables>()?
            .release_changelogs
            .remove(release.as_str());
        Ok(())
    }
    fn remove_deployment_changelog(&mut self, deployment: &Id) -> Result<(), EngineError> {
        self.table_mut::<DeliveryTables>()?
            .deployment_changelogs
            .remove(deployment.as_str());
        Ok(())
    }
    fn put_entity_link(&mut self, record: &EntityLinkRecord) -> Result<(), EngineError> {
        self.table_mut::<DeliveryTables>()?.links.insert(
            (
                record.source_kind,
                record.source_id.clone(),
                record.id.clone(),
            ),
            record.clone(),
        );
        Ok(())
    }
    fn remove_entity_link(
        &mut self,
        source_kind: LinkEntityKind,
        source_id: &Id,
        link_id: &Id,
    ) -> Result<(), EngineError> {
        self.table_mut::<DeliveryTables>()?.links.remove(&(
            source_kind,
            source_id.to_string(),
            link_id.to_string(),
        ));
        Ok(())
    }
    fn put_delivery_counter(
        &mut self,
        scope_kind: &str,
        scope_id: &Id,
        entity_kind: &str,
        counter: &NumberCounterRecord,
    ) -> Result<(), EngineError> {
        self.table_mut::<DeliveryTables>()?.counters.insert(
            (
                scope_kind.to_owned(),
                scope_id.to_string(),
                entity_kind.to_owned(),
            ),
            counter.clone(),
        );
        Ok(())
    }
    fn remove_delivery_counter(
        &mut self,
        scope_kind: &str,
        scope_id: &Id,
        entity_kind: &str,
    ) -> Result<(), EngineError> {
        self.table_mut::<DeliveryTables>()?.counters.remove(&(
            scope_kind.to_owned(),
            scope_id.to_string(),
            entity_kind.to_owned(),
        ));
        Ok(())
    }
}
