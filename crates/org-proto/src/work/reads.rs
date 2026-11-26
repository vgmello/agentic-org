//! Work reads: `work.snapshot`, `work.get`, `work.list`, `work.times` and `work.activity`.

use serde::{Deserialize, Serialize};

use super::events::WorkEvent;
use super::items::{Feature, TimeFigures, WorkItem, WorkItemSummary, WorkPosition};
use crate::call::Call;
use crate::{ItemKind, ItemStatus, LinkKind};

/// `work.snapshot`: the office's items (and, phase 3, features), one page at a time, each page
/// at its own position. Subscribe from the first page's position. Open items always; closed
/// and done ones whose last terminal event is at or after `closed_since`, or all of them with
/// `all_closed`. Deleted items are left out. Errors: `not_found` (the project),
/// `invalid_request`.
pub struct WorkSnapshot;

/// Params of `work.snapshot`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkSnapshotParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_since: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all_closed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// At most [`WORK_PAGE_MAX`] (the default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Result of `work.snapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkSnapshotResult {
    /// The position this page was read at.
    pub position: WorkPosition,
    pub items: Vec<WorkItemSummary>,
    /// Phase 3.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<Feature>,
    /// Absent: this was the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl Call for WorkSnapshot {
    const METHOD: &'static str = "work.snapshot";
    type Params = WorkSnapshotParams;
    type Result = WorkSnapshotResult;
}

/// `work.get`: one item in full. `item` is an id, a qualified number or a short id. Errors:
/// `not_found`, `invalid_request` (an ambiguous short id).
pub struct WorkGet;

/// Params of `work.get`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkGetParams {
    pub item: String,
}

/// Result of `work.get`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkGetResult {
    pub item: WorkItem,
}

impl Call for WorkGet {
    const METHOD: &'static str = "work.get";
    type Params = WorkGetParams;
    type Result = WorkGetResult;
}

/// `work.list`: items matching every filter given. Open items only unless `include_closed`.
/// Errors: `not_found`, `invalid_request`.
pub struct WorkList;

/// Params of `work.list`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<ItemKind>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub statuses: Vec<ItemStatus>,
    /// A member id, a handle, or `none` (unassigned).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_id: Option<String>,
    /// Items with a link to this item id (their backlinks to it), e.g. past
    /// [`WORK_GET_MAX_LISTED`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_to: Option<String>,
    /// With `linked_to`: only links of this kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_kind: Option<LinkKind>,
    /// Words in the title, at most 200 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub include_closed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// At most [`WORK_PAGE_MAX`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Result of `work.list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkListResult {
    pub items: Vec<WorkItemSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl Call for WorkList {
    const METHOD: &'static str = "work.list";
    type Params = WorkListParams;
    type Result = WorkListResult;
}

/// `work.times`: the time figures of up to [`WORK_PAGE_MAX`] items (list columns). Errors:
/// `invalid_request`.
pub struct WorkTimes;

/// Params of `work.times`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkTimesParams {
    pub item_ids: Vec<String>,
}

/// One item's time figures, in [`WorkTimesResult`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemTime {
    pub item_id: String,
    pub time: TimeFigures,
}

/// Result of `work.times`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkTimesResult {
    pub times: Vec<ItemTime>,
}

impl Call for WorkTimes {
    const METHOD: &'static str = "work.times";
    type Params = WorkTimesParams;
    type Result = WorkTimesResult;
}

/// `work.activity`: an item's (or feature's) events after `after_version`, oldest first.
/// Errors: `not_found`.
pub struct WorkActivity;

/// Params of `work.activity`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkActivityParams {
    pub target_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_version: Option<u64>,
    /// At most [`WORK_PAGE_MAX`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Result of `work.activity`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkActivityResult {
    pub events: Vec<WorkEvent>,
    /// Absent: no more events now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_after_version: Option<u64>,
}

impl Call for WorkActivity {
    const METHOD: &'static str = "work.activity";
    type Params = WorkActivityParams;
    type Result = WorkActivityResult;
}
