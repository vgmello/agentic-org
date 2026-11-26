//! What the library needs from outside it, as contracts it owns: an office to send a command to,
//! somewhere to read a named file and standard input from, and a source of idempotency keys.
//! The executable supplies all three.

use std::io;

use org_proto::ErrorCode;
use serde_json::{Map, Value};

use crate::row::{Ack, Command};

/// One command, read and checked, ready for the office. The office authorizes and decides;
/// nothing here is a check that protects anything.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub command: &'static Command,
    /// The input object: every required field present, every field of its declared type.
    pub input: Map<String, Value>,
    /// The page size, for a list.
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub dry_run: bool,
    /// The key of a write that is not a dry run: the caller's `--idempotency-key`, or one from
    /// the [`KeySource`]. `None` for a read and for a dry run.
    pub idempotency_key: Option<String>,
    pub if_match: Option<u64>,
    pub durable: bool,
}

/// How a command that was not refused ended (`meta.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Completed,
    TimedOut,
    Withdrawn,
    Suspended,
    /// A question is still open and this command's short hold has ended.
    Waiting,
    GateRequired,
}

impl Status {
    pub const ALL: [Status; 6] = [
        Status::Completed,
        Status::TimedOut,
        Status::Withdrawn,
        Status::Suspended,
        Status::Waiting,
        Status::GateRequired,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Status::Completed => "completed",
            Status::TimedOut => "timed_out",
            Status::Withdrawn => "withdrawn",
            Status::Suspended => "suspended",
            Status::Waiting => "waiting",
            Status::GateRequired => "gate_required",
        }
    }
}

/// The office's answer to a command it did not refuse.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    /// One record, or a list of records: exactly the fields the row declares. The library drops
    /// a field the row does not declare and fails the command when a required one is missing.
    /// An optional field that has no value is left out; `null` is a value only where the row
    /// declares open JSON.
    pub data: Value,
    pub status: Status,
    /// For a write: the operation's ID, when the office has one to report. Without it
    /// `meta.operation_id` is the request's idempotency key.
    pub operation_id: Option<String>,
    /// For a write.
    pub ack: Option<Ack>,
    /// When the write produced a revision.
    pub revision: Option<u64>,
    /// For a list with more to read.
    pub next_cursor: Option<String>,
    /// Optional non-zero waiting messages/wakes hint for bound agents (`TASK-115` / `doc-28` §2.3).
    pub waiting_messages: Option<u32>,
    /// For a list: how many records match across every page (`meta.total`).
    pub total: Option<u64>,
}

impl Answer {
    /// A completed read of `data`.
    pub fn completed(data: Value) -> Self {
        Self {
            data,
            status: Status::Completed,
            operation_id: None,
            ack: None,
            revision: None,
            next_cursor: None,
            waiting_messages: None,
            total: None,
        }
    }
}

/// A command the office refused, or that failed (design 2.4).
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub code: ErrorCode,
    /// What happened, in a sentence.
    pub message: String,
    /// What to do next, in a sentence.
    pub hint: String,
    /// The flags to change for a retry that can succeed, from flag name to value. For
    /// `unavailable`, `not_ready` and `timeout` the library adds the write's `idempotency-key` itself.
    pub retry_with: Option<Map<String, Value>>,
    /// For a write, when the office has one to report. Without it `meta.operation_id` is the
    /// request's idempotency key.
    pub operation_id: Option<String>,
    /// What else the office said in structured form, when it said anything
    /// ([`RefusalDetails`]). Boxed: most refusals carry none.
    pub details: Option<Box<RefusalDetails>>,
}

/// The structured details of a refusal, printed in `error` beside the code, message and hint.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefusalDetails {
    /// With `rate_limited`: how long until a retry can succeed (`error.retry_after_ms`).
    pub retry_after_ms: Option<u64>,
    /// The input field a rule refused (`error.field`), when the office names one.
    pub field: Option<String>,
    /// With `field`: the rule it failed (`error.rule`), such as `format` or `too_long`.
    pub rule: Option<String>,
}

impl Refusal {
    pub fn new(code: ErrorCode, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: hint.into(),
            retry_with: None,
            operation_id: None,
            details: None,
        }
    }
}

/// The office, as the command line needs it. An implementation connects when it is first
/// called, never before: a command line that cannot be read does not reach it.
pub trait Office {
    fn call(&mut self, request: &Request) -> Result<Answer, Refusal>;
}

/// Where `--input-file` and `--stdin` read from. The library calls `stdin` only when `--stdin`
/// was given, and `file` only for the path of `--input-file`.
///
/// Every read is bounded. `max_bytes` is the most the command accepts
/// ([`crate::MAX_INPUT_BYTES`], or [`crate::MAX_YAML_INPUT_BYTES`] for `--input-format yaml`).
/// An implementation reads at most `max_bytes + 1` bytes and returns what it read: it never
/// holds more than that, however long the file or the stream is. The library refuses an answer
/// longer than `max_bytes` with `invalid_request`, so the one byte over is how an
/// implementation says the input is too long.
pub trait Sources {
    fn file(&mut self, path: &str, max_bytes: usize) -> io::Result<Vec<u8>>;
    fn stdin(&mut self, max_bytes: usize) -> io::Result<Vec<u8>>;
}

/// Where the idempotency key of a write that carries none comes from (design 2.7). The library
/// makes no randomness itself. It asks for a key only for a write that is not a dry run and has
/// no `--idempotency-key`, after the command line is read and before the office is called.
pub trait KeySource {
    /// A new key no earlier call used: 1 to 64 letters, digits and hyphens.
    fn new_key(&mut self) -> String;
}
