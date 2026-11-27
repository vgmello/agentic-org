//! Records of the tree as the wire shows them.

use org_proto::docs::{DocMediaKindWire, DocMediaWire, DocStatusWire, DocSummaryWire};
use org_store::Id;

use crate::project_docs::records::{DocEntry, DocMedia, DocStatus, MediaKind};

fn status(status: DocStatus) -> DocStatusWire {
    match status {
        DocStatus::Draft => DocStatusWire::Draft,
        DocStatus::Current => DocStatusWire::Current,
        DocStatus::Deprecated => DocStatusWire::Deprecated,
    }
}

pub fn doc_summary(doc: &DocEntry) -> DocSummaryWire {
    DocSummaryWire {
        id: doc.id.to_string(),
        project_id: doc.project_id.to_string(),
        path: doc.path.as_str().to_owned(),
        section: doc.path.section().as_str().to_owned(),
        kind: doc.kind.clone(),
        title: doc.title.clone(),
        status: status(doc.status),
        summary: doc.summary.clone(),
        owner_employee_id: doc.owner_employee_id.as_ref().map(Id::to_string),
        revision: doc.meta.revision,
        created_at: doc.meta.created_at.clone(),
        updated_at: doc.meta.updated_at.clone(),
        updated_by_id: doc.meta.updated_by.employee().map(Id::to_string),
    }
}

pub fn media(media: &DocMedia) -> DocMediaWire {
    DocMediaWire {
        id: media.id.to_string(),
        project_id: media.project_id.to_string(),
        path: media.file.rel_path(),
        kind: match media.kind {
            MediaKind::Image => DocMediaKindWire::Image,
            MediaKind::Diagram => DocMediaKindWire::Diagram,
        },
        media_type: media.media_type.clone(),
        bytes: media.bytes,
        sha256: media.sha256.clone(),
        title: media.title.clone(),
        summary: media.summary.clone(),
        revision: media.meta.revision,
        created_at: media.meta.created_at.clone(),
        updated_at: media.meta.updated_at.clone(),
    }
}
