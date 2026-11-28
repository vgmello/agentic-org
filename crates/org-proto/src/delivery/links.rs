//! Why links wire contract (`TASK-55`, `FC-061`, `schema.sql` `links` table).

use serde::{Deserialize, Serialize};

use crate::Call;

/// Record kind participating in a Why link (`record_refs.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkEntityKindWire {
    Project,
    Feature,
    Release,
    Milestone,
    Deployment,
    Task,
    Document,
    DevelopmentProcess,
    Media,
    #[serde(other)]
    Unknown,
}

/// Directed link kind (`links.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKindWire {
    DependsOn,
    RelatesTo,
    Fixes,
    #[serde(other)]
    Unknown,
}

/// One directed link connecting two records in a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityLinkWire {
    pub id: String,
    pub project_id: String,
    pub source_kind: LinkEntityKindWire,
    pub source_id: String,
    pub kind: LinkKindWire,
    pub target_kind: LinkEntityKindWire,
    pub target_id: String,
}

/// `links.list`: Why links scoped to one project (optionally filtered by entity or link kind).
pub struct LinksList;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinksListParams {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_kind: Option<LinkEntityKindWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<LinkKindWire>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinksListResult {
    pub project_id: String,
    pub links: Vec<EntityLinkWire>,
}

impl Call for LinksList {
    const METHOD: &'static str = "links.list";
    type Params = LinksListParams;
    type Result = LinksListResult;
}

/// `links.add`: add one directed Why link between two records inside a project.
pub struct LinksAdd;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinksAddParams {
    pub command_id: String,
    #[serde(default)]
    pub project: String,
    pub source_kind: LinkEntityKindWire,
    pub source_id: String,
    pub kind: LinkKindWire,
    pub target_kind: LinkEntityKindWire,
    pub target_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkWriteResult {
    pub link: EntityLinkWire,
}

impl Call for LinksAdd {
    const METHOD: &'static str = "links.add";
    type Params = LinksAddParams;
    type Result = LinkWriteResult;
}

/// `links.remove`: remove one directed Why link.
pub struct LinksRemove;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinksRemoveParams {
    pub command_id: String,
    #[serde(default)]
    pub project: String,
    pub source_kind: LinkEntityKindWire,
    pub source_id: String,
    pub link_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkRemoveResult {
    pub removed_id: String,
}

impl Call for LinksRemove {
    const METHOD: &'static str = "links.remove";
    type Params = LinksRemoveParams;
    type Result = LinkRemoveResult;
}
