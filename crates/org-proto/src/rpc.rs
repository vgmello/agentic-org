//! The org API as JSON-RPC 2.0: one method per `OrgApi` call, with its params and result types,
//! plus the envelope (request, response, notification, error object) orgd and the app's Rust
//! shell put them in.
//!
//! Conventions:
//! - Method names are the snake_case form of the `OrgApi` method (`needs_you`, `assign_task`).
//! - `params` is always a JSON object (by-name). Methods without params take [`NoParams`] (`{}`);
//!   orgd also accepts a request that omits `params` for them.
//! - Methods that return nothing have the result `null`.
//! - A method that fails returns a JSON-RPC error whose `data` is an [`ApiError`]. An orgd that
//!   doesn't offer a method yet (M1 ships the files side first) rejects it with `unavailable`;
//!   `goals` and `activity` return `[]` instead.
//! - Events arrive after [`methods::Subscribe`] as notifications with the method
//!   [`crate::EVENT_NOTIFICATION`] and an [`OrgEvent`] as `params`.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use ts_rs::TS;

use crate::error::{codes, ApiError};
use crate::event::OrgEvent;
use crate::hire::{HirePlan, HirePreview, HireRequest};
use crate::ids::{AgentId, SessionId, WorkspacePath};
use crate::model::{
    ActivityEvent, Agent, Backlog, Document, DocumentKind, DocumentRef, Environment, Goal, HrAgent,
    Machine, NeedsYouItem, Organization, Presence, Project, Role, Session, Task,
};

// ─── Params and results ─────────────────────────────────────────────────────

/// Params of a method that takes none: an empty object.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct NoParams {}

/// `hello()`: what the app checks before anything else (version negotiation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ApiInfo {
    /// The org API version orgd speaks. The app refuses to load a different one (`API_VERSION`).
    pub api_version: u32,
    /// orgd's own version, for Settings and bug reports.
    pub orgd_version: String,
    pub machine: Machine,
}

/// `snapshot()`: everything the first screen needs, in one call. Later changes arrive as
/// events and through the granular methods.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Snapshot {
    pub organization: Organization,
    pub machine: Machine,
    pub projects: Vec<Project>,
    pub roles: Vec<Role>,
    pub hr: HrAgent,
    /// Every agent, offboarded ones included (their records are kept): tell them apart by `status`.
    pub agents: Vec<Agent>,
    pub presence: Vec<Presence>,
    /// One per agent in `agents`; an unreadable file is `kind: 'unreadable'`, never an error.
    pub backlogs: Vec<Backlog>,
    /// Open and dismissed items (`dismissed_at` tells them apart). Null when this orgd can't
    /// report Needs you yet (M1), so the UI can say so instead of showing "Nothing needs you".
    pub needs_you: Option<Vec<NeedsYouItem>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AgentsParams {
    /// Include offboarded agents. Default: false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub include_offboarded: Option<bool>,
}

/// Params of the methods that address one agent: `agent`, `backlog`, `sessions` and
/// `open_session`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AgentIdParams {
    pub agent_id: AgentId,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct NeedsYouParams {
    /// Include dismissed items. Default: false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub include_dismissed: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ActivityParams {
    /// Newest first, at most this many. Default: orgd's page size. orgd caps it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DocumentsParams {
    pub agent_id: AgentId,
    /// Only documents of this kind. Default: all kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub kind: Option<DocumentKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DocumentParams {
    /// Untrusted: orgd must reject absolute paths and `..` and keep it inside the workspace.
    pub path: WorkspacePath,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DismissParams {
    pub item_id: String,
}

/// Writes the backlog row only; it never starts, or writes into, a session (spec §3.7, R#5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AssignTaskRequest {
    pub agent_id: AgentId,
    pub text: String,
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ResumeSessionParams {
    pub agent_id: AgentId,
    /// The session to resume. Default: the agent's latest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub session_id: Option<SessionId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StopSessionParams {
    pub session_id: SessionId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct OpenSessionResult {
    pub session: Session,
    /// One live session per agent: true when the running one was returned instead.
    pub already_running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProposeHireResult {
    /// Identifies this hire in every `hire_progress` event, through provisioning.
    pub draft_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HireParams {
    /// From `propose_hire`; provisioning's `hire_progress` events carry it.
    pub draft_id: String,
    pub plan: HirePlan,
    /// From the `preview_hire` result the owner confirmed.
    pub workspace_revision: String,
}

// ─── Methods ────────────────────────────────────────────────────────────────

/// One JSON-RPC method of the org API: its name and the types of its params and result.
pub trait Method {
    const NAME: MethodName;
    type Params: Serialize + DeserializeOwned + TS + 'static;
    type Result: Serialize + DeserializeOwned + TS + 'static;
}

/// A method's params and result, for the `OrgMethods` map. Never sent on the wire.
#[derive(TS)]
pub struct Rpc<P, R> {
    pub params: P,
    pub result: R,
}

macro_rules! org_methods {
    ($( $(#[$doc:meta])* $marker:ident $name:ident ($params:ty) -> $result:ty; )*) => {
        /// One zero-sized type per method, implementing [`Method`].
        pub mod methods {
            $(
                $(#[$doc])*
                #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
                pub struct $marker;
            )*
        }

        $(
            impl Method for methods::$marker {
                const NAME: MethodName = MethodName::$marker;
                type Params = $params;
                type Result = $result;
            }
        )*

        /// Every method name of the org API.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
        #[serde(rename_all = "snake_case")]
        pub enum MethodName {
            $( $marker, )*
        }

        impl MethodName {
            pub const ALL: &'static [MethodName] = &[$( MethodName::$marker, )*];

            /// The method's name on the wire.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( MethodName::$marker => stringify!($name), )*
                }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $( stringify!($name) => Some(MethodName::$marker), )*
                    _ => None,
                }
            }
        }

        /// The org API for TypeScript: each method's params and result, keyed by method name.
        #[derive(TS)]
        pub struct OrgMethods {
            $(
                $(#[$doc])*
                pub $name: Rpc<$params, $result>,
            )*
        }
    };
}

org_methods! {
    /// Who is answering and which API version it speaks. The app calls it first.
    Hello hello(NoParams) -> ApiInfo;
    /// Everything the UI needs for its first paint, in one call.
    Snapshot snapshot(NoParams) -> Snapshot;
    Organization organization(NoParams) -> Organization;
    Environment environment(NoParams) -> Environment;
    Projects projects(NoParams) -> Vec<Project>;
    Roles roles(NoParams) -> Vec<Role>;
    Hr hr(NoParams) -> HrAgent;
    Agents agents(AgentsParams) -> Vec<Agent>;
    Agent agent(AgentIdParams) -> Agent;
    Presence presence(NoParams) -> Vec<Presence>;
    Goals goals(NoParams) -> Vec<Goal>;
    Backlog backlog(AgentIdParams) -> Backlog;
    Sessions sessions(AgentIdParams) -> Vec<Session>;
    NeedsYou needs_you(NeedsYouParams) -> Vec<NeedsYouItem>;
    Activity activity(ActivityParams) -> Vec<ActivityEvent>;
    Documents documents(DocumentsParams) -> Vec<DocumentRef>;
    Document document(DocumentParams) -> Document;
    Dismiss dismiss(DismissParams) -> ();
    AssignTask assign_task(AssignTaskRequest) -> Task;
    /// Opens a new session, or returns the running one (one live session per agent).
    OpenSession open_session(AgentIdParams) -> OpenSessionResult;
    ResumeSession resume_session(ResumeSessionParams) -> OpenSessionResult;
    StopSession stop_session(StopSessionParams) -> ();
    /// HR proposal; progress and the result arrive as `hire_progress` events.
    ProposeHire propose_hire(HireRequest) -> ProposeHireResult;
    PreviewHire preview_hire(HirePlan) -> HirePreview;
    /// Provisions the hire for `draft_id` (from `propose_hire`); its `hire_progress` events carry
    /// that id. Rejects with `not_found` for an unknown draft, and with `conflict` (emitting
    /// `workspace_changed`) if `workspace_revision` is stale.
    Hire hire(HireParams) -> ();
    /// Starts this connection's event stream: `event` notifications carrying an `OrgEvent`.
    Subscribe subscribe(NoParams) -> ();
    /// Stops this connection's event stream.
    Unsubscribe unsubscribe(NoParams) -> ();
}

// ─── Envelope ───────────────────────────────────────────────────────────────

/// The `jsonrpc` member: always `"2.0"`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Version2;

impl Serialize for Version2 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("2.0")
    }
}

impl<'de> Deserialize<'de> for Version2 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value == "2.0" {
            Ok(Version2)
        } else {
            Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(&value),
                &"\"2.0\"",
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    String(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: Version2,
    pub id: RequestId,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Request {
    pub fn new<M: Method>(id: RequestId, params: &M::Params) -> serde_json::Result<Self> {
        Ok(Request {
            jsonrpc: Version2,
            id,
            method: M::NAME.as_str().to_owned(),
            params: Some(serde_json::to_value(params)?),
        })
    }

    /// Reads the params for `M`. A missing `params` reads as `{}`, so methods without params
    /// accept requests that omit it.
    pub fn params<M: Method>(&self) -> Result<M::Params, ErrorObject> {
        let params = self
            .params
            .clone()
            .unwrap_or_else(|| Value::Object(Default::default()));
        serde_json::from_value(params).map_err(|e| ErrorObject {
            code: codes::INVALID_PARAMS,
            message: format!("invalid params for {}: {e}", self.method),
            data: None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    pub jsonrpc: Version2,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Notification {
    /// The notification that carries an event to a subscribed connection.
    pub fn event(event: &OrgEvent) -> serde_json::Result<Self> {
        Ok(Notification {
            jsonrpc: Version2,
            method: crate::EVENT_NOTIFICATION.to_owned(),
            params: Some(serde_json::to_value(event)?),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorObject {
    pub code: i32,
    pub message: String,
    /// For the API's own errors, the [`ApiError`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl From<&ApiError> for ErrorObject {
    fn from(error: &ApiError) -> Self {
        ErrorObject {
            code: error.rpc_code(),
            message: error.message().to_owned(),
            // An ApiError is plain data; serializing it can't fail.
            data: serde_json::to_value(error).ok(),
        }
    }
}

impl From<ApiError> for ErrorObject {
    fn from(error: ApiError) -> Self {
        ErrorObject::from(&error)
    }
}

impl From<ErrorObject> for ApiError {
    /// The `ApiError` in `data` when there is one; otherwise a protocol error mapped by code:
    /// method not found → `unavailable` (an orgd that doesn't offer the method yet), invalid
    /// params → `invalid`, anything else → `internal`.
    fn from(error: ErrorObject) -> Self {
        if let Some(api_error) = error
            .data
            .as_ref()
            .and_then(|data| ApiError::deserialize(data).ok())
        {
            return api_error;
        }
        match error.code {
            codes::METHOD_NOT_FOUND => ApiError::unavailable(error.message),
            codes::INVALID_PARAMS => ApiError::invalid(error.message, Vec::new()),
            code => ApiError::internal(error.message, format!("JSON-RPC error {code}")),
        }
    }
}

/// A response's outcome: exactly one of `result` and `error`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Result(Value),
    Error(ErrorObject),
}

/// A response. Reading one is strict: an object (not an array), `id` present (it may be
/// `null`), and exactly one of `result` and `error`. A duplicated key keeps its last value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Response {
    pub jsonrpc: Version2,
    /// `null` only when the request's id couldn't be read (parse error, invalid request).
    pub id: Option<RequestId>,
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// A present key, even when its value is `null` (`Some(Value::Null)`); a missing key is `None`.
fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

/// A required key whose value may be `null`.
fn nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(Deserialize)]
struct RawResponse {
    jsonrpc: Version2,
    #[serde(deserialize_with = "nullable")]
    id: Option<RequestId>,
    #[serde(default, deserialize_with = "present")]
    result: Option<Value>,
    #[serde(default)]
    error: Option<ErrorObject>,
}

impl<'de> Deserialize<'de> for Response {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Read an object first: the derived reader alone would also take a positional array.
        let object = serde_json::Map::<String, Value>::deserialize(deserializer)?;
        let raw =
            RawResponse::deserialize(Value::Object(object)).map_err(serde::de::Error::custom)?;
        let outcome = match (raw.result, raw.error) {
            (Some(result), None) => Outcome::Result(result),
            (None, Some(error)) => Outcome::Error(error),
            (Some(_), Some(_)) => {
                return Err(serde::de::Error::custom(
                    "a response has both `result` and `error`",
                ))
            }
            (None, None) => {
                return Err(serde::de::Error::custom(
                    "a response has neither `result` nor `error`",
                ))
            }
        };
        Ok(Response {
            jsonrpc: raw.jsonrpc,
            id: raw.id,
            outcome,
        })
    }
}

impl Response {
    pub fn success<M: Method>(id: RequestId, result: &M::Result) -> serde_json::Result<Self> {
        Ok(Response {
            jsonrpc: Version2,
            id: Some(id),
            outcome: Outcome::Result(serde_json::to_value(result)?),
        })
    }

    pub fn error(id: Option<RequestId>, error: impl Into<ErrorObject>) -> Self {
        Response {
            jsonrpc: Version2,
            id,
            outcome: Outcome::Error(error.into()),
        }
    }

    /// Reads the result for `M`, or the error as an [`ApiError`].
    pub fn into_result<M: Method>(self) -> Result<M::Result, ApiError> {
        match self.outcome {
            Outcome::Result(value) => serde_json::from_value(value).map_err(|e| {
                ApiError::internal(
                    format!("unreadable result for {}", M::NAME.as_str()),
                    e.to_string(),
                )
            }),
            Outcome::Error(error) => Err(error.into()),
        }
    }
}

/// What orgd sends on the control connection: responses to requests, and notifications.
/// A message with `method` and no `id` is a notification; anything else reads as a response,
/// so a malformed response fails with its own error rather than a generic one.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ServerMessage {
    Response(Response),
    Notification(Notification),
}

impl<'de> Deserialize<'de> for ServerMessage {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let is_notification = value
            .as_object()
            .is_some_and(|o| o.contains_key("method") && !o.contains_key("id"));
        if is_notification {
            Notification::deserialize(value)
                .map(ServerMessage::Notification)
                .map_err(serde::de::Error::custom)
        } else {
            Response::deserialize(value)
                .map(ServerMessage::Response)
                .map_err(serde::de::Error::custom)
        }
    }
}
