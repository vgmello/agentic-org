//! What the library needs from outside it, as contracts it owns: an office to send a command to,
//! and somewhere to read a named file and standard input from. The executable supplies both.

use std::io;

use org_proto::ErrorCode;
use serde_json::{Map, Value};

use crate::table::{Ack, Command};

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
    GateRequired,
}

impl Status {
    pub const ALL: [Status; 5] = [
        Status::Completed,
        Status::TimedOut,
        Status::Withdrawn,
        Status::Suspended,
        Status::GateRequired,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Status::Completed => "completed",
            Status::TimedOut => "timed_out",
            Status::Withdrawn => "withdrawn",
            Status::Suspended => "suspended",
            Status::GateRequired => "gate_required",
        }
    }
}

/// The office's answer to a command it did not refuse.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    /// One record, or a list of records.
    pub data: Value,
    pub status: Status,
    /// For a write.
    pub operation_id: Option<String>,
    /// For a write.
    pub ack: Option<Ack>,
    /// When the write produced a revision.
    pub revision: Option<u64>,
    /// For a list with more to read.
    pub next_cursor: Option<String>,
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
    /// The flags to change for a retry that can succeed, from flag name to value.
    pub retry_with: Option<Map<String, Value>>,
    /// For a write.
    pub operation_id: Option<String>,
}

impl Refusal {
    pub fn new(code: ErrorCode, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: hint.into(),
            retry_with: None,
            operation_id: None,
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
pub trait Sources {
    fn file(&mut self, path: &str) -> io::Result<Vec<u8>>;
    fn stdin(&mut self) -> io::Result<Vec<u8>>;
}
