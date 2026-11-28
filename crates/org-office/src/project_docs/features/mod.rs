//! Thin `org-office` adapter bridging [`ProjectDocsRead`] + `project_docs::access::authorize`
//! into `org_docs::project_tree::ops`, plus `put_media` image sanitization.

pub mod put_media;

use org_docs::project_tree::codecs::{RawDocEntry, RawDocMedia, RawProjectDoc};
use org_docs::project_tree::ops::{ProjectTreeView, TreeCaller, TreeMutation, TreeOpError};
use org_store::{Ack, Change, DurablePath, EngineError, Id, Sha256, Timestamp};

use super::access::{authorize, Denied, DocsAccess};
use super::ports::ProjectDocsRead;
use super::records::{doc_to_raw, entry_to_raw, media_to_raw};
use crate::organization::records::{time_text, EmployeeStatus};

pub struct TreeViewAdapter<'a, R: ProjectDocsRead>(pub &'a R);

fn parse_id(raw: &str) -> Result<Id, String> {
    Id::parse(raw).map_err(|_| format!("invalid id `{raw}`"))
}

fn map_err(e: EngineError) -> String {
    e.to_string()
}

impl<R: ProjectDocsRead> ProjectTreeView for TreeViewAdapter<'_, R> {
    fn project_doc_entries(&self, project_id: &str) -> Result<Vec<RawDocEntry>, String> {
        Ok(self
            .0
            .project_doc_entries(&parse_id(project_id)?)
            .map_err(map_err)?
            .iter()
            .map(entry_to_raw)
            .collect())
    }
    fn project_doc(&self, doc_id: &str) -> Result<Option<RawProjectDoc>, String> {
        Ok(self
            .0
            .project_doc(&parse_id(doc_id)?)
            .map_err(map_err)?
            .as_ref()
            .map(doc_to_raw))
    }
    fn project_doc_media(&self, project_id: &str) -> Result<Vec<RawDocMedia>, String> {
        Ok(self
            .0
            .project_doc_media(&parse_id(project_id)?)
            .map_err(map_err)?
            .iter()
            .map(media_to_raw)
            .collect())
    }
    fn active_employee_exists(&self, emp_id: &str) -> Result<bool, String> {
        Ok(self
            .0
            .employee(&parse_id(emp_id)?)
            .map_err(map_err)?
            .is_some_and(|e| e.status == EmployeeStatus::Active))
    }
    fn document_link_ids(&self, doc_id: &str) -> Result<Vec<String>, String> {
        self.0
            .document_link_ids(&parse_id(doc_id)?)
            .map_err(map_err)
    }
}

pub(crate) fn map_tree_err(err: TreeOpError) -> Denied {
    match err {
        TreeOpError::Refused(api) => Denied::Refused(api),
        TreeOpError::Storage(msg) => Denied::Storage(EngineError::new(msg)),
    }
}

pub(crate) fn to_change(mutation: TreeMutation) -> Result<Change, Denied> {
    let mut change = Change::new(Ack::Durable);
    for (p, bytes) in mutation.puts {
        let dp = DurablePath::parse(&p).map_err(|e| {
            Denied::Storage(EngineError::new(format!("invalid docs path `{p}`: {e}")))
        })?;
        change = change.put(dp, bytes);
    }
    for p in mutation.removes {
        let dp = DurablePath::parse(&p).map_err(|e| {
            Denied::Storage(EngineError::new(format!("invalid docs path `{p}`: {e}")))
        })?;
        change = change.remove(dp);
    }
    Ok(change.reply(mutation.reply))
}

pub fn run_read<R: ProjectDocsRead, T>(
    view: &R,
    caller: &str,
    project: &str,
    f: impl FnOnce(&TreeViewAdapter<'_, R>, &str) -> Result<T, TreeOpError>,
) -> Result<T, Denied> {
    let project = authorize(view, caller, project, DocsAccess::Read)?.project;
    f(&TreeViewAdapter(view), project.id.as_str()).map_err(map_tree_err)
}

pub fn run_write<R: ProjectDocsRead>(
    view: &R,
    now: Timestamp,
    caller: &str,
    project: &str,
    f: impl FnOnce(&TreeViewAdapter<'_, R>, &TreeCaller<'_>) -> Result<TreeMutation, TreeOpError>,
) -> Result<Change, Denied> {
    let cleared = authorize(view, caller, project, DocsAccess::Write)?;
    let ts = time_text(now);
    let ctx = TreeCaller {
        caller_id: cleared.caller.id.as_str(),
        project_id: cleared.project.id.as_str(),
        now_rfc3339: &ts,
    };
    to_change(f(&TreeViewAdapter(view), &ctx).map_err(map_tree_err)?)
}

pub fn derive_id(namespace: &str, command_id: &str, now_ms: u64) -> Id {
    let digest = Sha256::of_parts(&[namespace.as_bytes(), b"\0", command_id.as_bytes()]);
    let hash = digest.as_bytes();
    let mut b = [0u8; 16];
    b[0..6].copy_from_slice(&now_ms.to_be_bytes()[2..8]);
    b[6] = (hash[6] & 0x0f) | 0x70;
    b[7] = hash[7];
    b[8] = (hash[8] & 0x3f) | 0x80;
    b[9..16].copy_from_slice(&hash[9..16]);
    let hex: String = b.iter().map(|byte| format!("{byte:02x}")).collect();
    let text = format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    );
    Id::parse(&text).expect("a derived UUIDv7 parses")
}
