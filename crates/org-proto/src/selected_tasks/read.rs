//! Bounded selected reads. Content identity is task-scoped, never a file path.
use super::{object, RawPrincipal, Revision};
use serde::{Deserialize, Serialize};

pub const METHOD: &str = <Read as crate::Call>::METHOD;
pub const PAGE_MAX: u16 = 50;
pub struct Read;
impl crate::Call for Read {
    const METHOD: &'static str = "task.selected.read";
    type Params = Query;
    type Result = Result<i64>;
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Query {
    Status,
    List {
        after: Option<String>,
        queue: Queue,
        limit: u16,
    },
    Detail {
        task_id: String,
    },
    History {
        task_id: String,
        after: Option<Revision>,
        limit: u16,
    },
    Targets {
        task_id: String,
        target_kind: TargetKind,
        after: Option<String>,
        limit: u16,
    },
    Chunk {
        task_id: String,
        #[serde(deserialize_with = "object")]
        reference: ContentRef<i64>,
        offset: u64,
        limit: u32,
    },
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Queue {
    All,
    Owned { team_id: String },
    AssignedTeam { team_id: String },
    AssignedEmployee { employee_id: String },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Team,
    Employee,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Snapshot,
    Event,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentRef<R> {
    pub task_id: String,
    pub revision: R,
    pub kind: ContentKind,
    /// SHA-256 of the complete returned source bytes, including any embedded
    /// codec checksum property. This is distinct from an event payload checksum.
    pub sha256: String,
    pub byte_length: u64,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card<R> {
    pub task_id: String,
    pub revision: R,
    pub accountable_team_id: String,
    pub project_id: Option<String>,
    pub number: Option<String>,
    pub title_preview: String,
    pub status: String,
    pub assigned_team_id: Option<String>,
    pub assignee_id: Option<String>,
    pub snapshot: ContentRef<R>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub id: String,
    pub name_preview: String,
    pub active: bool,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Result<R> {
    Status {
        directory_generation: R,
    },
    List {
        items: Vec<Card<R>>,
        next: Option<String>,
    },
    Detail {
        task: Card<R>,
        history_coverage: String,
    },
    History {
        events: Vec<ContentRef<R>>,
        next: Option<R>,
        coverage: String,
    },
    Targets {
        directory_generation: R,
        items: Vec<Target>,
        next: Option<String>,
    },
    Chunk {
        reference: ContentRef<R>,
        offset: u64,
        hex: String,
        next: Option<u64>,
    },
}

pub struct Request {
    pub id: u64,
    pub principal: RawPrincipal,
    pub query: Query,
}
pub fn decode(bytes: &[u8]) -> std::result::Result<Request, super::DecodeError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Raw {
        id: u64,
        method: String,
        #[serde(default)]
        principal: RawPrincipal,
        #[serde(deserialize_with = "object")]
        params: Query,
    }
    if bytes.len() > crate::MAX_FRAME_BYTES {
        return Err(super::DecodeError);
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let raw: Raw = object(&mut decoder).map_err(|_| super::DecodeError)?;
    decoder.end().map_err(|_| super::DecodeError)?;
    if raw.method != METHOD || raw.id == 0 || raw.id > 9_007_199_254_740_991 {
        return Err(super::DecodeError);
    }
    Ok(Request {
        id: raw.id,
        principal: raw.principal,
        query: raw.params,
    })
}

impl Result<i64> {
    pub fn into_browser(
        self,
    ) -> std::result::Result<Result<super::BrowserRevision>, super::DecodeError> {
        use super::{BrowserRevision, Revision};
        let rev = |n| Revision::new(n).map(BrowserRevision);
        fn reference(
            r: ContentRef<i64>,
        ) -> std::result::Result<ContentRef<super::BrowserRevision>, super::DecodeError> {
            Ok(ContentRef {
                task_id: r.task_id,
                revision: super::BrowserRevision(Revision::new(r.revision)?),
                kind: r.kind,
                sha256: r.sha256,
                byte_length: r.byte_length,
            })
        }
        fn card(
            c: Card<i64>,
        ) -> std::result::Result<Card<super::BrowserRevision>, super::DecodeError> {
            Ok(Card {
                task_id: c.task_id,
                revision: super::BrowserRevision(Revision::new(c.revision)?),
                accountable_team_id: c.accountable_team_id,
                project_id: c.project_id,
                number: c.number,
                title_preview: c.title_preview,
                status: c.status,
                assigned_team_id: c.assigned_team_id,
                assignee_id: c.assignee_id,
                snapshot: reference(c.snapshot)?,
            })
        }
        Ok(match self {
            Self::Status {
                directory_generation,
            } => Result::Status {
                directory_generation: rev(directory_generation)?,
            },
            Self::List { items, next } => Result::List {
                items: items
                    .into_iter()
                    .map(card)
                    .collect::<std::result::Result<_, _>>()?,
                next,
            },
            Self::Detail {
                task,
                history_coverage,
            } => Result::Detail {
                task: card(task)?,
                history_coverage,
            },
            Self::History {
                events,
                next,
                coverage,
            } => Result::History {
                events: events
                    .into_iter()
                    .map(reference)
                    .collect::<std::result::Result<_, _>>()?,
                next: next.map(rev).transpose()?,
                coverage,
            },
            Self::Targets {
                directory_generation,
                items,
                next,
            } => Result::Targets {
                directory_generation: rev(directory_generation)?,
                items,
                next,
            },
            Self::Chunk {
                reference: r,
                offset,
                hex,
                next,
            } => Result::Chunk {
                reference: reference(r)?,
                offset,
                hex,
                next,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_reads_reject_duplicate_wrappers_and_preserve_full_i64_browser_references() {
        for raw in [
            r#"{"id":1,"id":2,"method":"task.selected.read","params":{"kind":"status"}}"#,
            r#"{"id":1,"method":"task.selected.read","params":{"kind":"list","after":null,"queue":{"kind":"all"},"limit":1,"limit":2}}"#,
        ] {
            assert!(decode(raw.as_bytes()).is_err());
        }
        for raw in [
            r#"[1,"task.selected.read",{"kind":"status"}]"#,
            r#"{"id":1,"method":"task.selected.read","params":["status"]}"#,
            r#"{"id":1,"method":"task.selected.read","params":{"kind":"chunk","task_id":"task","reference":["task",1,"snapshot","hash",2],"offset":0,"limit":1}}"#,
            r#"{"id":1,"method":"task.selected.read","params":{"kind":"list","after":null,"queue":{"kind":"all","kind":"all"},"limit":1}}"#,
        ] {
            assert!(decode(raw.as_bytes()).is_err());
        }
        let result = Result::History {
            events: vec![ContentRef {
                task_id: "opaque/ID".into(),
                revision: i64::MAX,
                kind: ContentKind::Event,
                sha256: "a".repeat(64),
                byte_length: 16 * 1024 * 1024,
            }],
            next: Some(i64::MAX),
            coverage: "unknown".into(),
        }
        .into_browser()
        .unwrap();
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["events"][0]["revision"], "9223372036854775807");
        assert_eq!(value["next"], "9223372036854775807");
    }
}
