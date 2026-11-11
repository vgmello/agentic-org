use std::fmt;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::calls::Call;
use crate::error::{ApiError, CallError};
use crate::events::EventKind;
use crate::principal::{MalformedPrincipal, Principal};

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
///
/// Its `params` and `principal` can carry a secret in raw JSON, so don't log a request or keep it
/// longer than handling it takes. Its `Debug` shows only the id and method and elides the params
/// and the principal (`<elided N bytes>`); `Clone` copies the secret along with it.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    /// The grant a request on a host-made channel acts under (X-02 §4.1), as raw JSON: read it
    /// with [`Request::principal`]. `None` when the key is absent; a present key is `Some`, even
    /// when its value is `null`, so a server can refuse it rather than mistake it for no
    /// principal. It stays raw so that a malformed principal never fails the envelope: the
    /// server answers it with `not_authorized` (on an office socket, any principal is
    /// `not_authorized`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub principal: Option<Value>,
    #[serde(default)]
    pub params: Value,
}

/// Decodes a present field as `Some`, even when its value is `null`.
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

impl Request {
    /// Build a request for call `C`, with no principal.
    pub fn new<C: Call>(id: u64, params: &C::Params) -> Result<Self, serde_json::Error> {
        Ok(Self {
            id,
            method: C::METHOD.to_owned(),
            principal: None,
            params: serde_json::to_value(params)?,
        })
    }

    /// The same request, acting under `principal`. The grant's MAC is copied into the request's
    /// raw JSON, like any secret in params: see the type's docs.
    pub fn with_principal(mut self, principal: &Principal) -> Result<Self, serde_json::Error> {
        self.principal = Some(serde_json::to_value(principal)?);
        Ok(self)
    }

    /// Read the principal: `Ok(None)` when the request has none, [`MalformedPrincipal`] when it
    /// is present but isn't a [`Principal`] (`null`, a MAC that isn't 64 lowercase hex digits, a
    /// missing field, an array). A server answers that with `not_authorized`. The error carries
    /// nothing from the request.
    pub fn principal(&self) -> Result<Option<Principal>, MalformedPrincipal> {
        match &self.principal {
            None => Ok(None),
            Some(value) => Principal::deserialize(value)
                .map(Some)
                .map_err(|_| MalformedPrincipal),
        }
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
///
/// Its `result` can carry a secret in raw JSON (e.g. `office.app_token`'s), so don't log a
/// response or keep it longer than handling it takes. Its `Debug` shows only the id and the
/// error, if any, and elides the result (`<elided N bytes>`); `Clone` copies the secret along
/// with it.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawResponse")]
pub struct Response {
    pub id: u64,
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// A response's outcome. On the wire it is a `result` or an `error` key, never both.
///
/// A result can hold a secret: its `Debug` elides it (`Result(<elided N bytes>)`) and shows
/// an error in full.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
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
///
/// Its `data` can carry a secret in raw JSON, so don't log an event or keep it longer than
/// handling it takes. Its `Debug` shows only the event name and seq and elides the data
/// (`<elided N bytes>`); `Clone` copies the secret along with it.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
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
///
/// It can carry secrets: see [`Response`] and [`Event`]. Its `Debug` elides every payload,
/// including all of an unknown message.
#[derive(Clone, PartialEq, Serialize)]
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

/// A payload in a `Debug` print: its size in bytes as compact JSON, never its content.
struct Elided<'a>(&'a Value);

impl fmt::Debug for Elided<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Count the bytes as they're written, so the payload is never copied.
        struct Count(usize);
        impl std::io::Write for Count {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0 += buf.len();
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut count = Count(0);
        match serde_json::to_writer(&mut count, self.0) {
            Ok(()) => write!(f, "<elided {} bytes>", count.0),
            Err(_) => f.write_str("<elided>"),
        }
    }
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("Request");
        s.field("id", &self.id).field("method", &self.method);
        if let Some(principal) = &self.principal {
            s.field("principal", &Elided(principal));
        }
        s.field("params", &Elided(&self.params)).finish()
    }
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("id", &self.id)
            .field("outcome", &self.outcome)
            .finish()
    }
}

impl fmt::Debug for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Outcome::Result(v) => f.debug_tuple("Result").field(&Elided(v)).finish(),
            Outcome::Error(e) => f.debug_tuple("Error").field(e).finish(),
        }
    }
}

impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Event")
            .field("event", &self.event)
            .field("seq", &self.seq)
            .field("data", &Elided(&self.data))
            .finish()
    }
}

impl fmt::Debug for ServerMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServerMessage::Response(r) => f.debug_tuple("Response").field(r).finish(),
            ServerMessage::Event(e) => f.debug_tuple("Event").field(e).finish(),
            ServerMessage::Unknown(v) => f.debug_tuple("Unknown").field(&Elided(v)).finish(),
        }
    }
}
