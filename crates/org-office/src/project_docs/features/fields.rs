//! The request field rules of the documentation tree: each raw field a caller sends is checked
//! here and turned into the value a record holds, or refused as `<field>: <rule>`.

use org_proto::docs::{DocStatusWire, DOC_BODY_MAX_BYTES, DOC_REF_MAX_BYTES};
use org_proto::ErrorCode;
use org_store::Id;

use crate::organization::records::{is_line, is_stable_id, EmployeeStatus};
use crate::project_docs::access::{refused, Denied};
use crate::project_docs::paths::{file_stem_from_title, DocPath, PathError, Section};
use crate::project_docs::ports::ProjectDocsRead;
use crate::project_docs::records::{is_body, is_doc_kind, is_summary, DocStatus, ProjectDoc};

pub(crate) fn invalid(field: &str, rule: impl std::fmt::Display) -> Denied {
    refused(ErrorCode::InvalidRequest, format!("{field}: {rule}"))
}

pub(crate) fn path_error(field: &str, error: &PathError) -> Denied {
    invalid(field, error)
}

pub(crate) fn kind(raw: &str) -> Result<String, Denied> {
    if is_doc_kind(raw) {
        Ok(raw.to_owned())
    } else {
        Err(invalid(
            "kind",
            "a lowercase slug of letters, digits and `-` (at most 64 bytes), not brief or changelog",
        ))
    }
}

pub(crate) fn title(raw: &str) -> Result<String, Denied> {
    let title = raw.trim();
    if is_line(title) {
        Ok(title.to_owned())
    } else {
        Err(invalid(
            "title",
            "one line of visible text, 1 to 200 characters",
        ))
    }
}

/// `None` for an absent or empty summary.
pub(crate) fn summary(raw: Option<&str>) -> Result<Option<String>, Denied> {
    let Some(text) = raw.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    if is_summary(text) {
        Ok(Some(text.to_owned()))
    } else {
        Err(invalid("summary", "visible text of at most 2 KiB"))
    }
}

pub(crate) fn body(raw: &str) -> Result<String, Denied> {
    if raw.len() > DOC_BODY_MAX_BYTES {
        return Err(refused(
            ErrorCode::TooLarge,
            "body_markdown: at most 256 KiB",
        ));
    }
    if !is_body(raw) {
        return Err(invalid("body_markdown", "holds no NUL character"));
    }
    Ok(raw.to_owned())
}

pub(crate) fn status(raw: DocStatusWire) -> Result<DocStatus, Denied> {
    match raw {
        DocStatusWire::Draft => Ok(DocStatus::Draft),
        DocStatusWire::Current => Ok(DocStatus::Current),
        DocStatusWire::Deprecated => Ok(DocStatus::Deprecated),
        DocStatusWire::Unknown => Err(invalid("status", "one of: draft current deprecated")),
    }
}

pub(crate) fn uuid(field: &str, raw: &str) -> Result<Id, Denied> {
    if !is_stable_id(raw) {
        return Err(invalid(field, "a lowercase UUIDv7"));
    }
    Id::parse(raw).map_err(|_| invalid(field, "a lowercase UUIDv7"))
}

/// An owner: absent or empty is none; otherwise an active employee of the organization.
pub(crate) fn owner<R: ProjectDocsRead>(view: &R, raw: Option<&str>) -> Result<Option<Id>, Denied> {
    let Some(text) = raw.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    let id = uuid("owner_employee_id", text)?;
    match view.employee(&id)? {
        Some(employee) if employee.status == EmployeeStatus::Active => Ok(Some(id)),
        _ => Err(invalid(
            "owner_employee_id",
            "an active employee of the organization",
        )),
    }
}

pub(crate) fn section(raw: &str) -> Result<Section, Denied> {
    Section::parse(raw).map_err(|e| path_error("section", &e))
}

/// The path of a new document: `path` as given (in `section` when both are), or
/// `<section>/<title-slug>.md`.
pub(crate) fn new_path(
    path: Option<&str>,
    in_section: Option<&str>,
    title: &str,
) -> Result<DocPath, Denied> {
    let wanted = in_section.map(section).transpose()?;
    match path {
        Some(raw) => {
            let parsed = DocPath::parse(raw).map_err(|e| path_error("path", &e))?;
            if wanted.is_some_and(|s| s != parsed.section()) {
                return Err(invalid("path", "not in the given section"));
            }
            Ok(parsed)
        }
        None => {
            let file = format!("{}.md", file_stem_from_title(title));
            DocPath::join(&wanted.unwrap_or_default(), &file).map_err(|e| path_error("path", &e))
        }
    }
}

/// The document `raw` names in `project`: its UUIDv7 or its path. A document of another
/// project is not found.
pub(crate) fn find_doc<R: ProjectDocsRead>(
    view: &R,
    project: &Id,
    raw: &str,
) -> Result<ProjectDoc, Denied> {
    let not_found = || refused(ErrorCode::NotFound, "document not found");
    if raw.is_empty() || raw.len() > DOC_REF_MAX_BYTES {
        return Err(invalid("document", "required"));
    }
    if is_stable_id(raw) {
        let id = uuid("document", raw)?;
        return match view.project_doc(&id)? {
            Some(doc) if doc.project_id == *project => Ok(doc),
            _ => Err(not_found()),
        };
    }
    let path = DocPath::parse(raw).map_err(|e| path_error("document", &e))?;
    let entry = view
        .project_doc_entries(project)?
        .into_iter()
        .find(|d| d.path == path)
        .ok_or_else(not_found)?;
    view.project_doc(&entry.id)?.ok_or_else(not_found)
}

/// Refuses a stale `expected_revision`.
pub(crate) fn revision(doc_revision: u64, expected: u64) -> Result<(), Denied> {
    if doc_revision == expected {
        Ok(())
    } else {
        Err(refused(
            ErrorCode::Conflict,
            format!("expected revision {expected}, but the document is at revision {doc_revision}"),
        ))
    }
}
