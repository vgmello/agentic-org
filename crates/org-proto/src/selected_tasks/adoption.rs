//! Explicit owner adoption on a host-approved channel. No path or authority input.
use super::{object, staging::Blob, DecodeError, RawPrincipal, Revision};
use crate::gateway::OperationId;
use serde::{Deserialize, Serialize};
pub const METHOD: &str = <Adopt as crate::Call>::METHOD;
pub const CAPABILITY: &str = "selected_task_adoption_v1";
pub struct Adopt;
impl crate::Call for Adopt {
    const METHOD: &'static str = "task.selected.adopt.v1";
    type Params = Action;
    type Result = Outcome;
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Commit {
        operation_id: OperationId,
        stage_id: OperationId,
        manifest_sha256: String,
        expected_directory_generation: Revision,
    },
    DirectoryCommit {
        operation_id: OperationId,
        stage_id: OperationId,
        manifest_sha256: String,
        expected_directory_generation: Option<Revision>,
    },
    Lookup {
        operation_id: OperationId,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    Completed {
        task_id: String,
        revision: i64,
        history_coverage: String,
    },
    DirectoryCompleted {
        generation: i64,
    },
    Staged {
        reason_code: String,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    pub resource_profile: String,
    pub task_id: String,
    pub canonical_path: String,
    pub public_snapshot: Blob,
    pub history_origin: HistoryOrigin,
    pub events: Vec<Event>,
    pub public_blobs: Vec<Blob>,
    pub number_counter: Option<Blob>,
    pub number_allocation: Option<Blob>,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HistoryOrigin {
    Creation,
    SnapshotOnly,
    Baseline { revision: Revision },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub revision: Revision,
    pub blob: Blob,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryManifest {
    pub version: u32,
    pub resource_profile: String,
    pub source_position: DirectoryPosition,
    pub files: Vec<DirectoryFile>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryPosition {
    pub epoch: u64,
    pub seq: u64,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryKind {
    Organization,
    Repositories,
    Team,
    Employee,
    Project,
    Role,
    Counter,
    Allocation,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryFile {
    pub kind: DirectoryKind,
    pub canonical_path: String,
    pub blob: Blob,
}
pub struct Request {
    pub id: u64,
    pub principal: RawPrincipal,
    pub action: Action,
}
pub fn decode(bytes: &[u8]) -> Result<Request, DecodeError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Raw {
        id: u64,
        method: String,
        #[serde(default)]
        principal: RawPrincipal,
        #[serde(deserialize_with = "object")]
        params: Action,
    }
    if bytes.len() > crate::MAX_FRAME_BYTES {
        return Err(DecodeError);
    }
    let mut d = serde_json::Deserializer::from_slice(bytes);
    let r: Raw = object(&mut d).map_err(|_| DecodeError)?;
    d.end().map_err(|_| DecodeError)?;
    if r.method != METHOD || r.id == 0 || r.id > 9_007_199_254_740_991 {
        return Err(DecodeError);
    }
    Ok(Request {
        id: r.id,
        principal: r.principal,
        action: r.params,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adoption_wrapper_and_action_refuse_duplicates_positional_records_and_unknown_fields() {
        let action = r#"{"kind":"commit","operation_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","stage_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","manifest_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","expected_directory_generation":9223372036854775807}"#;
        let valid = format!("{{\"id\":1,\"method\":\"{METHOD}\",\"params\":{action}}}");
        assert!(decode(valid.as_bytes()).is_ok());
        for bad in [
            valid.replace("\"id\":1", "\"id\":2,\"id\":1"),
            valid.replace(
                "\"kind\":\"commit\"",
                "\"kind\":\"commit\",\"kind\":\"commit\"",
            ),
            valid.replace("\"stage_id\":", "\"extra\":true,\"stage_id\":"),
            valid.replace("9223372036854775807", "9223372036854775808"),
            valid.replace("9223372036854775807", "1.0"),
            valid.replace("9223372036854775807", "0"),
            valid.replace(action, "[]"),
            format!("{valid} null"),
        ] {
            assert!(decode(bad.as_bytes()).is_err());
        }
    }
}
