//! Owner-bound opaque source transfer. Staging never establishes acceptance.
use super::{object, DecodeError, RawPrincipal};
use crate::gateway::OperationId;
use serde::{Deserialize, Serialize};

pub const METHOD: &str = <Stage as crate::Call>::METHOD;
pub const CAPABILITY: &str = "selected_task_staging_v1";
pub struct Stage;
impl crate::Call for Stage {
    const METHOD: &'static str = "task.selected.stage.v1";
    type Params = Params;
    type Result = Status;
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Params {
    pub stage_id: OperationId,
    pub manifest_sha256: String,
    #[serde(deserialize_with = "object")]
    pub blob: Blob,
    /// None inspects existing data; Some publishes exactly this chunk.
    #[serde(deserialize_with = "optional_chunk")]
    pub chunk: Option<Chunk>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Blob {
    pub sha256: String,
    pub byte_length: u64,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chunk {
    pub offset: u64,
    pub hex: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub stage_id: OperationId,
    pub manifest_sha256: String,
    pub blob: Blob,
    pub chunk_offsets: Vec<u64>,
    /// Whole-blob digest verified, rather than merely a complete chunk count.
    pub verified: bool,
}
pub struct Request {
    pub id: u64,
    pub principal: RawPrincipal,
    pub params: Params,
}
fn optional_chunk<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Chunk>, D::Error> {
    let raw = Option::<Box<serde_json::value::RawValue>>::deserialize(d)?;
    raw.map(|raw| {
        let mut decoder = serde_json::Deserializer::from_str(raw.get());
        let chunk = object(&mut decoder).map_err(serde::de::Error::custom)?;
        decoder.end().map_err(serde::de::Error::custom)?;
        Ok(chunk)
    })
    .transpose()
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
        params: Params,
    }
    if bytes.len() > crate::MAX_FRAME_BYTES {
        return Err(DecodeError);
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let raw: Raw = object(&mut decoder).map_err(|_| DecodeError)?;
    decoder.end().map_err(|_| DecodeError)?;
    if raw.method != METHOD || raw.id == 0 || raw.id > 9_007_199_254_740_991 {
        return Err(DecodeError);
    }
    Ok(Request {
        id: raw.id,
        principal: raw.principal,
        params: raw.params,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_raw_stage_wrapper_and_nested_duplicates_are_rejected() {
        let p = r#"{"stage_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","manifest_sha256":"aaa","blob":{"sha256":"bbb","byte_length":1},"chunk":{"offset":0,"hex":"ff"}}"#;
        let valid = format!("{{\"id\":1,\"method\":\"{METHOD}\",\"params\":{p}}}");
        assert!(decode(valid.as_bytes()).is_ok());
        for bad in [
            valid.replace("\"id\":1", "\"id\":2,\"id\":1"),
            valid.replace("\"hex\":\"ff\"", "\"hex\":\"ff\",\"hex\":\"00\""),
            valid.replace("\"byte_length\":1", "\"byte_length\":1,\"byte_length\":2"),
            valid.replace("\"offset\":0", "\"offset\":0,\"unexpected\":true"),
            valid.replace("{\"offset\":0,\"hex\":\"ff\"}", "[0,\"ff\"]"),
            valid.replace(p, "[]"),
            format!("{valid} true"),
        ] {
            assert!(decode(bad.as_bytes()).is_err());
        }
    }
}
