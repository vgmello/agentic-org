use std::fmt;

use serde::{Deserialize, Serialize};

/// Why a request failed. Unknown codes from a newer server decode as [`ErrorCode::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ErrorCode {
    /// The request couldn't be read (bad JSON, a bad envelope, or params of the wrong shape), or
    /// it can be read but makes no sense: a value outside its documented limits (e.g. an
    /// agent's `title` or `home`), a call that gives nothing to change, `Unknown` or a
    /// non-canonical value where a known one is required, an agent-only call on a person (or
    /// the other way round), or a call on a member in the wrong state (e.g. editing or
    /// offboarding an agent that is already offboarded). In [`crate::msg`] and
    /// [`crate::directory`], a number out of range (a `limit`, `timeout_ms`, `wait_ms` or
    /// `ttl_ms`) is `invalid_request`, while a string, body, meta or note over its size limit
    /// is `too_large`.
    InvalidRequest,
    /// The server doesn't know the method.
    UnknownMethod,
    /// A name breaks its rule. Each kind of name (an organization, a team, a person, an agent, a
    /// project, an area) has its own rule; an organization's is [`crate::OrganizationName::parse`].
    /// An agent's `messaging_name` that breaks its format is also `invalid_name`, and so is a
    /// person's name or an agent's `messaging_name` that looks like an address (a `run:` prefix,
    /// or 26 letters and digits, the shape of a member id).
    InvalidName,
    /// Something with that name or id already exists: an organization or a sibling team with
    /// that name (names compare ignoring case), another agent with that `messaging_name`
    /// (ignoring case), a person's name or `messaging_name` that is a reserved word (`owner`,
    /// `office`), a project with that key or name (ignoring case), a live area of the project
    /// with that name (ignoring case), a membership, project-team link or work-item link that
    /// already exists, or a client-made id that's taken.
    AlreadyExists,
    /// The server couldn't read or write its storage. Partial state is cleaned up.
    StorageError,
    /// A loop: a team would own itself or a team above it, or someone would report to
    /// themselves, directly or through others.
    Cycle,
    /// A team still owns teams, so it can't be deleted.
    NotEmpty,
    /// The lead isn't a member of the team.
    NotAMember,
    /// The command would remove the owner, add the owner to a team, make it a lead, or give it
    /// someone to report to.
    OwnerProtected,
    /// Something the command names doesn't exist (any more).
    NotFound,
    /// The `expected_version` is stale: the thing changed since the caller saw it. Also
    /// `office.restart` on an office that isn't `offline` or `unavailable` (e.g. it's already
    /// starting or active).
    Conflict,
    /// `subscribe { from_position }` can't resume from there; take a fresh `org.snapshot`.
    SnapshotRequired,
    /// The command is too old for HQ to accept (its id's time is past the dedup window).
    /// Applies to every organization command.
    Expired,
    /// The command's id is dated too far ahead of HQ's clock. Applies to every organization
    /// command.
    ClockSkew,
    /// A message or task is addressed to someone who isn't an agent or person of the
    /// organization: no member at all, an offboarded agent, or one whose status this office
    /// can't read.
    UnknownRecipient,
    /// The caller isn't allowed to make this call, e.g. the host's check of the peer's code
    /// signature failed for `office.restart`, an app-only command
    /// (`member.set_run_mode`, `project.create`) arrived on a connection that is
    /// not the app's, `msg.bind` refused a member (it fails closed), or
    /// `org.snapshot` or `subscribe` came on an office server connection that isn't bound (ADR-0003 §2.1.4;
    /// unlike `who`, which answers that with `not_bound`).
    NotAuthorized,
    /// What was asked for exists but can't serve now, e.g. the office isn't running. Trying again later may work. For `office.restart`, though, it
    /// means the office's reason is one a restart can't fix (`unsupported_version` or
    /// `invalid_request`); the message says why. Clients show an office's `status_reason`, not
    /// this code: the code says only why the call failed.
    Unavailable,
    /// The host (or office) is still starting; retry shortly. Temporary, unlike
    /// `unavailable`: e.g. `organization.create` or `office.restart` while
    /// the host's startup discards run.
    NotReady,
    /// An `ask`'s deadline (`expires_at`) passed without a reply, including when the
    /// recipient's office reports that the request expired.
    Timeout,
    /// A string, body, meta or note is over its size limit, in bytes (or, for a status line,
    /// characters; see the limits in [`crate::msg`] and [`crate::directory`]). A number out of
    /// range is `invalid_request` instead. Never stored as a write's answer.
    TooLarge,
    /// The connection hasn't bound with `msg.bind` (ADR-0003: an agent bound as itself; a
    /// person or the owner acts through a grant instead), or the bound member was moved out of
    /// this office or removed. Also ends held `msg.wait` and `msg.ask` calls when the connection
    /// re-binds. (`org.snapshot` and `subscribe` on an office server's unbound connection are
    /// `not_authorized` instead.) Never stored as a write's answer: a retry may succeed.
    NotBound,
    /// A task was to be completed while required checklist steps are unticked and no reason to
    /// complete it anyway was given; the message names the steps. Clients branch on it ("Mark
    /// done anyway…"). `tasks.transition` answers it (`TASK-88`), and the web gateway's task
    /// routes answer it as `409`. A build that doesn't know it reads it as
    /// [`ErrorCode::Unknown`].
    ChecklistIncomplete,
    /// `hello` offered no version this server speaks, or a connection that needs a v2
    /// `hello` didn't start with one (X-02 §3). The server closes the connection after it and
    /// never stores it. Not the office status reason of the same name
    /// ([`crate::OfficeStatusReason::UnsupportedVersion`]), which says why an office can't start.
    UnsupportedVersion,
    /// A known operation id (`command_id`, `change_id`, `msg.send`'s `id`, or a host op's
    /// `operation_id`) came with different params (its fingerprint differs) or from a
    /// different principal than the stored one. Use a new id for a different operation
    /// (X-02 §4.2).
    OperationReused,
    /// A reserved organization create was cancelled or abandoned after its window expired.
    Cancelled,
    /// A throttle key is locked; the error carries `retry_after_ms`.
    RateLimited,
    /// The protocol has this call; this office does not serve it yet. Not temporary, unlike
    /// `unavailable` and `not_ready`: retrying the same call against the same build answers
    /// the same. Never stored as a write's answer. A build that doesn't know it reads it as
    /// [`ErrorCode::Unknown`].
    Unsupported,
    /// A server-enforced mutation policy refused a direct write (for example, the Chief of Staff
    /// attempting a `ConfirmRequired` mutation while acting for a human caller instead of
    /// proposing it through `suggestion.create`). A build that doesn't know it reads it as
    /// [`ErrorCode::Unknown`].
    PolicyViolation,
    /// A code this build doesn't know (sent by a newer server).
    #[serde(other)]
    Unknown,
}

/// Boxed transparent wrapper for an optional server recovery hint so [`ApiError`] and
/// [`crate::Response`] stay compact on the stack (`size_of::<Response>() < 128`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ErrorHint(pub String);

impl ErrorHint {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Maximum number of intervening events carried inline in [`ConflictDetail::events_since`]
/// (`TASK-90`).
pub const CONFLICT_MAX_EVENTS: usize = 20;

/// Maximum serialized UTF-8 JSON byte budget for [`ConflictDetail::events_since`] (`TASK-90`,
/// 8 KiB).
pub const CONFLICT_MAX_EVENTS_BYTES: usize = 8 * 1024;

/// Compact summary of one intervening event in [`ConflictDetail::events_since`] (`TASK-90`).
/// Never carries full `body_markdown`, comment bodies, or private task notes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictEvent {
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
    pub actor_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    /// Compact structured delta (event `type`, changed fields/values, step info, etc.).
    pub change: serde_json::Value,
}

/// Compact current state snapshot carried on [`ConflictDetail::current_summary`] (`TASK-90`).
/// Never inlines `body_markdown`, comment bodies, or private task notes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub title: String,
    pub status: String,
    pub priority: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_team_id: Option<String>,
    pub checklist_total: u64,
    pub checklist_done: u64,
    #[serde(default)]
    pub ac_total: u64,
    #[serde(default)]
    pub ac_done: u64,
    #[serde(default)]
    pub dod_total: u64,
    #[serde(default)]
    pub dod_done: u64,
    #[serde(default)]
    pub work_steps_total: u64,
    #[serde(default)]
    pub work_steps_done: u64,
    #[serde(default)]
    pub comment_count: u64,
    #[serde(default)]
    pub watcher_count: u64,
}

/// Structured optimistic-concurrency detail returned with [`ErrorCode::Conflict`] when an
/// `expected_revision` / `--if-match` is stale (`TASK-98.4`, `TASK-90`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictDetail {
    pub expected_revision: u64,
    pub current_revision: u64,
    /// Sorted section names modified after `expected_revision` (e.g. `"title"`, `"attachments"`,
    /// `"checklist"`, `"comments"`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_sections: Vec<String>,
    /// Bounded intervening events (`revision > expected_revision`), strictly capped at
    /// `<= CONFLICT_MAX_EVENTS` and `<= CONFLICT_MAX_EVENTS_BYTES` serialized JSON.
    #[serde(default)]
    pub events_since: Vec<ConflictEvent>,
    /// True when one or more intervening events were omitted to stay within the cap.
    #[serde(default)]
    pub events_truncated: bool,
    /// Count of intervening events omitted from `events_since`.
    #[serde(default)]
    pub omitted_events: u64,
    /// True when the caller should re-inspect the record before retrying (e.g., events were
    /// truncated, body/comments changed, or `expected_revision > current_revision`).
    #[serde(default)]
    pub reread_recommended: bool,
    /// Compact current-state summary (never inlines `body_markdown`, comments, or private notes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_summary: Option<ConflictSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ApiErrorExtra {
    hint: Option<ErrorHint>,
    conflict: Option<ConflictDetail>,
}

/// Wire representation of [`ApiError`] so `hint` and `conflict` stay behind a single
/// `Option<Box<_>>` on the stack (`size_of::<Response>() < 128`) while preserving exact flat JSON.
#[derive(Serialize, Deserialize)]
struct ApiErrorWire {
    code: ErrorCode,
    message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hint: Option<ErrorHint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry_after_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    conflict: Option<ConflictDetail>,
}

impl From<ApiErrorWire> for ApiError {
    fn from(w: ApiErrorWire) -> Self {
        let extra = (w.hint.is_some() || w.conflict.is_some()).then(|| {
            Box::new(ApiErrorExtra {
                hint: w.hint,
                conflict: w.conflict,
            })
        });
        Self {
            code: w.code,
            message: w.message,
            retry_after_ms: w.retry_after_ms,
            field: w.field,
            rule: w.rule,
            extra,
        }
    }
}

impl From<ApiError> for ApiErrorWire {
    fn from(e: ApiError) -> Self {
        let (hint, conflict) = match e.extra {
            Some(ex) => (ex.hint, ex.conflict),
            None => (None, None),
        };
        Self {
            code: e.code,
            message: e.message,
            hint,
            retry_after_ms: e.retry_after_ms,
            field: e.field,
            rule: e.rule,
            conflict,
        }
    }
}

/// An error returned by the server. `message` is for people; it never holds paths or SQL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "ApiErrorWire", into = "ApiErrorWire")]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
    /// With `rate_limited`: how long until a retry can succeed.
    pub retry_after_ms: Option<u64>,
    /// With `invalid_request` for a credential rule: the input field (`login`, `password`).
    pub field: Option<String>,
    /// With `field`: the rule it failed (`format`, `taken`, `too_short`, `too_long`).
    pub rule: Option<String>,
    /// Optional server-supplied hint (`TASK-80.4`) and optimistic-concurrency detail (`TASK-98.4`),
    /// boxed together so [`crate::Response`] stays under 128 bytes.
    extra: Option<Box<ApiErrorExtra>>,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retry_after_ms: None,
            field: None,
            rule: None,
            extra: None,
        }
    }

    /// Attaches an actionable recovery hint to this error.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.extra
            .get_or_insert_with(|| Box::new(ApiErrorExtra::default()))
            .hint = Some(ErrorHint(hint.into()));
        self
    }

    /// Returns the server-supplied actionable hint, when present.
    pub fn hint(&self) -> Option<&str> {
        self.extra
            .as_deref()
            .and_then(|e| e.hint.as_ref())
            .map(ErrorHint::as_str)
    }

    /// Returns structured optimistic-concurrency conflict details, when present (`TASK-98.4`).
    pub fn conflict_detail(&self) -> Option<&ConflictDetail> {
        self.extra.as_deref().and_then(|e| e.conflict.as_ref())
    }

    /// `policy_violation` with a server-supplied actionable hint.
    pub fn policy_violation(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::new(ErrorCode::PolicyViolation, message).with_hint(hint)
    }

    /// `conflict` carrying the current revision and sections changed since `expected_revision`.
    pub fn conflict(message: impl Into<String>, detail: ConflictDetail) -> Self {
        let mut error = Self::new(ErrorCode::Conflict, message);
        error
            .extra
            .get_or_insert_with(|| Box::new(ApiErrorExtra::default()))
            .conflict = Some(detail);
        error
    }

    /// `rate_limited` with the delay until a retry can succeed.
    pub fn rate_limited(retry_after_ms: u64) -> Self {
        let mut error = Self::new(ErrorCode::RateLimited, "too many attempts; try again later");
        error.retry_after_ms = Some(retry_after_ms);
        error
    }

    /// `invalid_request` naming the input field and the rule it failed.
    pub fn rule(field: &str, rule: &str) -> Self {
        let mut error = Self::new(ErrorCode::InvalidRequest, "a credential rule was not met");
        error.field = Some(field.to_owned());
        error.rule = Some(rule.to_owned());
        error
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

/// Reading a typed result out of a [`crate::Response`] failed.
#[derive(Debug)]
pub enum CallError {
    /// The server answered with an error.
    Api(ApiError),
    /// The server's result didn't match the call's result type. The serde error can quote the
    /// result's values (e.g. a message body), so never log it for `msg.*` results.
    Decode(serde_json::Error),
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CallError::Api(e) => write!(f, "server error: {e}"),
            CallError::Decode(e) => write!(f, "unexpected result: {e}"),
        }
    }
}

impl std::error::Error for CallError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_is_its_own_code_on_the_wire() {
        let error = ApiError::new(ErrorCode::Unsupported, "not served by this office yet");
        let wire = serde_json::to_value(&error).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({ "code": "unsupported", "message": "not served by this office yet" })
        );
        assert_eq!(serde_json::from_value::<ApiError>(wire).unwrap(), error);
        assert_eq!(error.hint(), None);
    }

    #[test]
    fn a_code_this_build_does_not_know_still_reads_as_unknown() {
        let code: ErrorCode = serde_json::from_str("\"some_newer_code\"").unwrap();
        assert_eq!(code, ErrorCode::Unknown);
    }

    #[test]
    fn policy_violation_and_hint_serialize_cleanly() {
        let error = ApiError::policy_violation(
            "Direct durable writes require caller confirmation.",
            "Propose this change with `agent-os suggestion create`.",
        );
        let wire = serde_json::to_value(&error).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({
                "code": "policy_violation",
                "message": "Direct durable writes require caller confirmation.",
                "hint": "Propose this change with `agent-os suggestion create`."
            })
        );
        assert_eq!(serde_json::from_value::<ApiError>(wire).unwrap(), error);
        assert_eq!(
            error.hint(),
            Some("Propose this change with `agent-os suggestion create`.")
        );
        assert!(
            std::mem::size_of::<crate::Response>() < 128,
            "Response must stay under Clippy's 128-byte result_large_err threshold, got {}",
            std::mem::size_of::<crate::Response>()
        );
    }

    #[test]
    fn conflict_detail_roundtrips_and_omits_when_absent() {
        let err = ApiError::conflict(
            "stale expected_revision 1: task is at revision 3",
            ConflictDetail {
                expected_revision: 1,
                current_revision: 3,
                changed_sections: vec!["attachments".into(), "title".into()],
                events_since: vec![ConflictEvent {
                    revision: 2,
                    occurred_at: Some("2026-11-27T00:00:00.000Z".into()),
                    actor_kind: "employee".into(),
                    actor_id: Some("0193a5c0-0000-7000-8000-000000000020".into()),
                    change: serde_json::json!({"type": "task_updated", "fields": ["title"]}),
                }],
                events_truncated: false,
                omitted_events: 0,
                reread_recommended: false,
                current_summary: Some(ConflictSummary {
                    key: Some("T-A1".into()),
                    title: "Updated title".into(),
                    status: "in_progress".into(),
                    priority: "high".into(),
                    assignee_id: None,
                    assigned_team_id: None,
                    checklist_total: 2,
                    checklist_done: 1,
                    ac_total: 1,
                    ac_done: 1,
                    dod_total: 1,
                    dod_done: 0,
                    work_steps_total: 0,
                    work_steps_done: 0,
                    comment_count: 0,
                    watcher_count: 1,
                }),
            },
        );
        let wire = serde_json::to_value(&err).unwrap();
        assert_eq!(wire["code"], "conflict");
        assert_eq!(wire["conflict"]["expected_revision"], 1);
        assert_eq!(wire["conflict"]["current_revision"], 3);
        assert_eq!(
            wire["conflict"]["changed_sections"],
            serde_json::json!(["attachments", "title"])
        );
        assert_eq!(
            wire["conflict"]["events_since"].as_array().unwrap().len(),
            1
        );
        assert_eq!(wire["conflict"]["events_truncated"], false);
        assert_eq!(wire["conflict"]["omitted_events"], 0);
        assert_eq!(wire["conflict"]["reread_recommended"], false);
        assert_eq!(wire["conflict"]["current_summary"]["key"], "T-A1");
        assert_eq!(serde_json::from_value::<ApiError>(wire).unwrap(), err);

        // Minimal legacy ConflictDetail JSON still decodes cleanly with defaults.
        let minimal = serde_json::json!({
            "code": "conflict",
            "message": "stale",
            "conflict": {
                "expected_revision": 1,
                "current_revision": 2,
                "changed_sections": ["title"]
            }
        });
        let decoded: ApiError = serde_json::from_value(minimal).unwrap();
        let detail = decoded.conflict_detail().expect("decoded detail");
        assert_eq!(detail.events_since, vec![]);
        assert!(!detail.events_truncated);
        assert_eq!(detail.omitted_events, 0);
        assert!(!detail.reread_recommended);
        assert_eq!(detail.current_summary, None);
    }
}
