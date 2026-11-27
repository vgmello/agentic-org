//! Engine-independent read and write ports for the delivery family (`ENG-1`, `ADR-0018`).

use std::collections::BTreeSet;

use org_docs::delivery::records::{
    DeploymentRecord, EntityLinkRecord, FeatureFlagRecord, FeatureRecord, LinkEntityKind,
    MilestoneRecord, NumberCounterRecord, ReleaseRecord, ScopedChangelogRecord,
};
use org_store::{Engine, EngineError, Id};

use crate::project_docs::ports::{ProjectDocsRead, ProjectDocsWrite};
use crate::tasks::ports::{TasksRead, TasksWrite};

/// Read queries over projected features, releases, milestones, deployments, flags, changelogs,
/// Why links, and scoped reference counters.
pub trait DeliveryRead: TasksRead + ProjectDocsRead {
    fn project_features(&self, project: &Id) -> Result<Vec<FeatureRecord>, EngineError>;
    fn feature(&self, id: &Id) -> Result<Option<FeatureRecord>, EngineError>;
    fn all_features(&self) -> Result<Vec<FeatureRecord>, EngineError>;
    fn project_releases(&self, project: &Id) -> Result<Vec<ReleaseRecord>, EngineError>;
    fn release(&self, id: &Id) -> Result<Option<ReleaseRecord>, EngineError>;
    fn release_milestones(&self, release: &Id) -> Result<Vec<MilestoneRecord>, EngineError>;
    fn milestone(&self, id: &Id) -> Result<Option<MilestoneRecord>, EngineError>;
    fn project_deployments(&self, project: &Id) -> Result<Vec<DeploymentRecord>, EngineError>;
    fn deployment(&self, id: &Id) -> Result<Option<DeploymentRecord>, EngineError>;
    fn project_flags(&self, project: &Id) -> Result<Vec<FeatureFlagRecord>, EngineError>;
    fn flag(&self, id: &Id) -> Result<Option<FeatureFlagRecord>, EngineError>;
    /// Flag IDs referenced by any release or deployment flag snapshot in `project`.
    fn used_flag_ids(&self, project: &Id) -> Result<BTreeSet<String>, EngineError>;
    fn release_changelog(&self, release: &Id)
        -> Result<Option<ScopedChangelogRecord>, EngineError>;
    fn deployment_changelog(
        &self,
        deployment: &Id,
    ) -> Result<Option<ScopedChangelogRecord>, EngineError>;
    fn project_links(&self, project: &Id) -> Result<Vec<EntityLinkRecord>, EngineError>;
    fn delivery_counter(
        &self,
        scope_kind: &str,
        scope_id: &Id,
        entity_kind: &str,
    ) -> Result<Option<NumberCounterRecord>, EngineError>;
    fn projects_with_delivery(&self) -> Result<Vec<Id>, EngineError>;
}

/// Write operations on the delivery projection inside a store transaction.
pub trait DeliveryWrite: DeliveryRead + TasksWrite + ProjectDocsWrite {
    fn put_feature(&mut self, record: &FeatureRecord) -> Result<(), EngineError>;
    fn remove_feature(&mut self, id: &Id) -> Result<(), EngineError>;
    fn put_release(&mut self, record: &ReleaseRecord) -> Result<(), EngineError>;
    fn remove_release(&mut self, id: &Id) -> Result<(), EngineError>;
    fn put_milestone(&mut self, record: &MilestoneRecord) -> Result<(), EngineError>;
    fn remove_milestone(&mut self, id: &Id) -> Result<(), EngineError>;
    fn put_deployment(&mut self, record: &DeploymentRecord) -> Result<(), EngineError>;
    fn remove_deployment(&mut self, id: &Id) -> Result<(), EngineError>;
    fn put_flag(&mut self, record: &FeatureFlagRecord) -> Result<(), EngineError>;
    fn remove_flag(&mut self, id: &Id) -> Result<(), EngineError>;
    fn put_scoped_changelog(&mut self, record: &ScopedChangelogRecord) -> Result<(), EngineError>;
    fn remove_release_changelog(&mut self, release: &Id) -> Result<(), EngineError>;
    fn remove_deployment_changelog(&mut self, deployment: &Id) -> Result<(), EngineError>;
    fn put_entity_link(&mut self, record: &EntityLinkRecord) -> Result<(), EngineError>;
    fn remove_entity_link(
        &mut self,
        source_kind: LinkEntityKind,
        source_id: &Id,
        link_id: &Id,
    ) -> Result<(), EngineError>;
    fn put_delivery_counter(
        &mut self,
        scope_kind: &str,
        scope_id: &Id,
        entity_kind: &str,
        counter: &NumberCounterRecord,
    ) -> Result<(), EngineError>;
    fn remove_delivery_counter(
        &mut self,
        scope_kind: &str,
        scope_id: &Id,
        entity_kind: &str,
    ) -> Result<(), EngineError>;
}

/// An engine hosting the delivery family.
pub trait DeliveryEngine: Engine<Tx: DeliveryWrite, Snapshot: DeliveryRead> {}

impl<E> DeliveryEngine for E where E: Engine<Tx: DeliveryWrite, Snapshot: DeliveryRead> {}
