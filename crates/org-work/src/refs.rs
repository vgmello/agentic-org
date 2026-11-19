//! Item references (§1.1): an id, a qualified number (`chk-T-12`, `REQ-14`), an unqualified one
//! (`T-12`, against a project the caller names) or a short id (`3k9f2x`).

use org_proto::{ApiError, ErrorCode};

use crate::ids::{is_ulid, SHORT_ID_LEN};
use crate::model::StoredItem;
use crate::ports::{NumberScope, OrgView, StoreError, WorkReads};

/// A parsed item reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemRef {
    /// A full id.
    Id(String),
    /// `<key>-T-<n>` or `<key>-B-<n>`.
    Qualified {
        project_key: String,
        bug: bool,
        number: u64,
    },
    /// `T-<n>` or `B-<n>`: resolved in a project the caller names.
    Unqualified { bug: bool, number: u64 },
    /// `REQ-<n>`.
    Request(u64),
    /// The last 6 characters of an id, lower case.
    Short(String),
}

impl ItemRef {
    /// Parses `text`, or `None` if it is no reference at all.
    pub fn parse(text: &str) -> Option<Self> {
        if is_ulid(text) {
            return Some(Self::Id(text.to_owned()));
        }
        if let Some(n) = text.strip_prefix("REQ-").and_then(number) {
            return Some(Self::Request(n));
        }
        let parts: Vec<&str> = text.split('-').collect();
        match parts.as_slice() {
            [kind, n] => {
                let bug = kind_letter(kind)?;
                Some(Self::Unqualified {
                    bug,
                    number: number(n)?,
                })
            }
            [key, kind, n] if is_key(key) => Some(Self::Qualified {
                project_key: (*key).to_owned(),
                bug: kind_letter(kind)?,
                number: number(n)?,
            }),
            _ if text.len() == SHORT_ID_LEN
                && text
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase()) =>
            {
                Some(Self::Short(text.to_owned()))
            }
            _ => None,
        }
    }
}

fn kind_letter(s: &str) -> Option<bool> {
    match s {
        "T" => Some(false),
        "B" => Some(true),
        _ => None,
    }
}

/// `[1-9][0-9]*` as a number.
fn number(s: &str) -> Option<u64> {
    if s.is_empty() || s.starts_with('0') || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// A project key: a lower-case letter, then 1 to 9 lower-case letters or digits.
fn is_key(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=10).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// Resolves a reference to an item: `not_found` when nothing matches, `invalid_request` for
/// text that is no reference, an unqualified number without a project, or a short id that
/// matches more than one item ("ambiguous").
pub fn resolve_item(
    org: &dyn OrgView,
    reads: &dyn WorkReads,
    text: &str,
    project_id: Option<&str>,
) -> Result<Result<StoredItem, ApiError>, StoreError> {
    let not_found = || Err(ApiError::new(ErrorCode::NotFound, "no such work item"));
    let Some(r) = ItemRef::parse(text) else {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "not an item reference",
        )));
    };
    let scope = |project_id: String, bug: bool| {
        if bug {
            NumberScope::Bugs { project_id }
        } else {
            NumberScope::Tasks { project_id }
        }
    };
    let found = match r {
        ItemRef::Id(id) => reads.get(&id)?,
        ItemRef::Request(n) => reads.by_number(&NumberScope::Requests, n)?,
        ItemRef::Qualified {
            project_key,
            bug,
            number,
        } => match org.project_by_key(&project_key)? {
            Some(p) => reads.by_number(&scope(p.id, bug), number)?,
            None => None,
        },
        ItemRef::Unqualified { bug, number } => match project_id {
            Some(p) => reads.by_number(&scope(p.to_owned(), bug), number)?,
            None => {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "name the project for an unqualified number",
                )))
            }
        },
        ItemRef::Short(short) => {
            let mut matches = reads.by_short_id(&short)?;
            if matches.len() > 1 {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "the short id is ambiguous",
                )));
            }
            matches.pop()
        }
    };
    Ok(found.map_or_else(not_found, Ok))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_form() {
        assert_eq!(
            ItemRef::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            Some(ItemRef::Id("01ARZ3NDEKTSV4RRFFQ69G5FAV".into()))
        );
        assert_eq!(
            ItemRef::parse("chk-T-12"),
            Some(ItemRef::Qualified {
                project_key: "chk".into(),
                bug: false,
                number: 12
            })
        );
        assert_eq!(
            ItemRef::parse("chk-B-7"),
            Some(ItemRef::Qualified {
                project_key: "chk".into(),
                bug: true,
                number: 7
            })
        );
        assert_eq!(
            ItemRef::parse("T-3"),
            Some(ItemRef::Unqualified {
                bug: false,
                number: 3
            })
        );
        assert_eq!(ItemRef::parse("REQ-14"), Some(ItemRef::Request(14)));
        assert_eq!(
            ItemRef::parse("3k9f2x"),
            Some(ItemRef::Short("3k9f2x".into()))
        );
        for bad in [
            "chk-T-0",
            "chk-T-012",
            "Chk-T-1",
            "c-T-1",
            "chk-X-1",
            "REQ-0",
            "T-",
            "3K9F2X",
            "abcdefg",
        ] {
            assert_eq!(ItemRef::parse(bad), None, "{bad}");
        }
    }
}
