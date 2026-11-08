use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::calls::Call;
use crate::error::{ApiError, CallError};
use crate::events::EventKind;

/// The `id` of an error response to a line the server couldn't read as a [`Request`] and
/// couldn't recover an id from (see [`salvage_id`]). Clients number their requests from 1
/// and keep ids below 2^53 (the TypeScript mirror uses JS numbers), so this id never
/// matches a pending request.
pub const UNREADABLE_REQUEST_ID: u64 = 0;

/// Recover the `id` from a line that didn't decode as a whole, e.g. `{"id":42,"method":5}`,
/// so the reply still reaches the right pending request. A server answers with
/// `invalid_request` under this id (or [`UNREADABLE_REQUEST_ID`] if there is none); a client
/// fails just that pending request. Read lines with [`crate::frame::read_line`] to keep them.
pub fn salvage_id(line: &[u8]) -> Option<u64> {
    serde_json::from_slice::<Value>(line)
        .ok()?
        .get("id")?
        .as_u64()
}

/// A client's request. `method` stays a string on the wire, so a server can answer an
/// unknown method with `unknown_method` instead of failing to parse the line.
/// Clients number requests from 1 (see [`UNREADABLE_REQUEST_ID`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl Request {
    /// Build a request for call `C`.
    pub fn new<C: Call>(id: u64, params: &C::Params) -> Result<Self, serde_json::Error> {
        Ok(Self {
            id,
            method: C::METHOD.to_owned(),
            params: serde_json::to_value(params)?,
        })
    }

    /// Whether this request is for call `C`.
    pub fn is<C: Call>(&self) -> bool {
        self.method == C::METHOD
    }

    /// Read the params as call `C`'s params. Params must be an object; missing params
    /// (`null`) read as `{}`. A server answers a failure here with `invalid_request`.
    pub fn params<C: Call>(&self) -> Result<C::Params, serde_json::Error> {
        match &self.params {
            Value::Null => serde_json::from_value(Value::Object(Map::new())),
            v @ Value::Object(_) => C::Params::deserialize(v),
            _ => Err(serde_json::Error::custom("params must be a JSON object")),
        }
    }
}

/// The server's answer to the request with the same `id`: either `result` or `error`.
/// A line with both, or neither, is rejected. A `null` result or a `null` error counts as
/// missing, so results must not serialize to `null` ([`Response::ok`] refuses them).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawResponse")]
pub struct Response {
    pub id: u64,
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// A response's outcome. On the wire it is a `result` or an `error` key, never both.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Result(Value),
    Error(ApiError),
}

/// What a response line holds before the "exactly one of result or error" check.
#[derive(Deserialize)]
struct RawResponse {
    id: u64,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<ApiError>,
}

impl TryFrom<RawResponse> for Response {
    type Error = &'static str;

    fn try_from(raw: RawResponse) -> Result<Self, Self::Error> {
        let outcome = match (raw.result, raw.error) {
            (Some(v), None) => Outcome::Result(v),
            (None, Some(e)) => Outcome::Error(e),
            (Some(_), Some(_)) => return Err("a response has both `result` and `error`"),
            (None, None) => return Err("a response has neither `result` nor `error`"),
        };
        Ok(Self {
            id: raw.id,
            outcome,
        })
    }
}

impl Response {
    /// A successful response to call `C`.
    /// Fails if the result serializes to `null`, which readers couldn't tell from no result.
    pub fn ok<C: Call>(id: u64, result: &C::Result) -> Result<Self, serde_json::Error> {
        let value = serde_json::to_value(result)?;
        if value.is_null() {
            return Err(serde_json::Error::custom("a result must not be null"));
        }
        Ok(Self {
            id,
            outcome: Outcome::Result(value),
        })
    }

    /// An error response.
    pub fn error(id: u64, error: ApiError) -> Self {
        Self {
            id,
            outcome: Outcome::Error(error),
        }
    }

    /// Read the outcome as call `C`'s result.
    pub fn into_result<C: Call>(self) -> Result<C::Result, CallError> {
        match self.outcome {
            Outcome::Result(v) => serde_json::from_value(v).map_err(CallError::Decode),
            Outcome::Error(e) => Err(CallError::Api(e)),
        }
    }
}

/// A server-pushed event, sent only after `subscribe`. Has no `id`. `seq` counts per
/// connection from 1. Clients ignore events they don't know. Missing `data` reads as `null`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub event: String,
    pub seq: u64,
    #[serde(default)]
    pub data: Value,
}

impl Event {
    /// Build event `E`.
    pub fn new<E: EventKind>(seq: u64, data: &E::Data) -> Result<Self, serde_json::Error> {
        Ok(Self {
            event: E::NAME.to_owned(),
            seq,
            data: serde_json::to_value(data)?,
        })
    }

    /// Whether this is event `E`.
    pub fn is<E: EventKind>(&self) -> bool {
        self.event == E::NAME
    }

    /// Read the data as event `E`'s data, or `None` if this is a different event.
    pub fn data<E: EventKind>(&self) -> Option<Result<E::Data, serde_json::Error>> {
        self.is::<E>().then(|| E::Data::deserialize(&self.data))
    }
}

/// Anything a server sends, told apart by its keys: `event` means an [`Event`]; `id` with
/// `result` or `error` means a [`Response`]. Any other object is [`ServerMessage::Unknown`]
/// (a shape from a newer server), which readers ignore. A known shape that fails to decode
/// is an error; use [`salvage_id`] on the line to fail just that pending request. Duplicate
/// keys are not detected on this path (the last one wins).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ServerMessage {
    Response(Response),
    Event(Event),
    Unknown(Value),
}

impl<'de> Deserialize<'de> for ServerMessage {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let map = Map::<String, Value>::deserialize(d)?;
        let is_event = map.contains_key("event");
        let is_response =
            map.contains_key("id") && (map.contains_key("result") || map.contains_key("error"));
        let value = Value::Object(map);
        if is_event {
            Event::deserialize(value)
                .map(ServerMessage::Event)
                .map_err(D::Error::custom)
        } else if is_response {
            Response::deserialize(value)
                .map(ServerMessage::Response)
                .map_err(D::Error::custom)
        } else {
            Ok(ServerMessage::Unknown(value))
        }
    }
}
