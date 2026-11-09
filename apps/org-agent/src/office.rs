//! The office path of `msg send | inbox | read | wait` and `who` (ADR-0003 Phase B): which way a
//! command goes ([`route`]), the calls, and the office's answers in the shapes this CLI has always
//! printed, so scripts and the inbox hooks read them the same way.
//!
//! - **Office found and reachable:** connect, `msg.bind` as the agent (no token: an agent run by
//!   hand), then the call. Its errors are the office's.
//! - **Office found but not reachable:** [`Route::Unreachable`]. `send` falls back to the local
//!   spool, as before, so no message is lost; the read side says so on stderr and shows the
//!   local spool.
//! - **No office on this Mac** (none asked for, no `office.json`): [`Route::NoOffice`], the
//!   spool, quietly, as before.
//!
//! Bodies and meta are printed only by the commands whose output shows them (`read`, `wait`),
//! never in a notice or an error.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::env;
use std::time::{Duration, Instant};

use org_client::agent::{find_office, open_as, FindOfficeError, OpenError};
use org_client::{Client, ClientError};
use org_proto::directory::{self, Who, WhoParams};
use org_proto::msg::{
    self, HeaderPage, Message, MessageHeader, MsgInbox, MsgInboxParams, MsgRead, MsgReadParams,
    MsgSend, MsgSendParams, MsgWait, MsgWaitParams, KIND_MSG, MAX_PAGE_LIMIT, MAX_WAIT_MS,
};
use org_proto::{Call, ErrorCode, MemberKind};
use org_spool::{
    generate_ulid, is_interrupted, DirectoryCard, InboxItem, InboxQuery, MessagePriority, Outgoing,
    RefEntry, TeamEntry, ENVELOPE_VERSION, INBOX_SUMMARY_DEFAULT_LIMIT,
};
use serde::Serialize;

use crate::{EXIT_IO, EXIT_TIMEOUT, EXIT_UNKNOWN_RECIPIENT, EXIT_USAGE};

/// The exit code for an office that is busy or not ready (`unavailable`; ADR-0003 §2.6).
const EXIT_OFFICE_UNAVAILABLE: u8 = 5;

/// The variable that names the agent's office (ADR-0003 §2.1); `--office` wins over it.
const OFFICE_ENV: &str = "ORG_OFFICE";

/// How much longer than the wait itself a `msg.wait` call may take to be answered.
const WAIT_ANSWER_MARGIN: Duration = Duration::from_secs(10);

/// A command that ends with an exit code and, unless it's silent, a message for stderr.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Failure {
    pub code: u8,
    pub message: Option<String>,
}

impl Failure {
    fn new(code: u8, message: impl Into<String>) -> Self {
        Failure {
            code,
            message: Some(message.into()),
        }
    }

    /// An error from the office: its code decides the exit code (ADR-0003 §2.6), and its text is
    /// the office's fixed wording (never a body).
    pub(crate) fn from_office(error: &ClientError) -> Self {
        Failure::new(exit_code(error), error.to_string())
    }
}

/// The exit code for an office error (ADR-0003 §2.6).
fn exit_code(error: &ClientError) -> u8 {
    let ClientError::Api(error) = error else {
        return EXIT_IO;
    };
    match error.code {
        ErrorCode::InvalidRequest
        | ErrorCode::InvalidName
        | ErrorCode::TooLarge
        | ErrorCode::AlreadyExists
        | ErrorCode::NotFound
        | ErrorCode::NotBound
        | ErrorCode::NotAuthorized
        | ErrorCode::Conflict => EXIT_USAGE,
        ErrorCode::Timeout => EXIT_TIMEOUT,
        ErrorCode::UnknownRecipient => EXIT_UNKNOWN_RECIPIENT,
        ErrorCode::Unavailable | ErrorCode::NotReady => EXIT_OFFICE_UNAVAILABLE,
        _ => EXIT_IO,
    }
}

/// Which way a command goes.
pub(crate) enum Route {
    /// Through the office, on a connection bound as the agent.
    Office(Office),
    /// The office was found but didn't answer: fall back to the spool, and say so.
    Unreachable(Unreachable),
    /// No office on this Mac: the spool, as before.
    NoOffice,
}

/// An office that didn't answer, and why.
pub(crate) struct Unreachable {
    office_id: String,
    reason: ClientError,
}

impl Unreachable {
    /// The stderr line when a send is spooled instead.
    pub(crate) fn queued_notice(&self) -> String {
        format!(
            "org-agent: office {} is not reachable ({}); the message is queued in the local spool",
            self.office_id, self.reason
        )
    }

    /// The stderr line when the read side shows the local spool instead.
    pub(crate) fn spool_notice(&self) -> String {
        format!(
            "org-agent: office {} is not reachable ({}); showing the local spool instead",
            self.office_id, self.reason
        )
    }
}

/// The office for `member`, from `office_flag` (`--office`), else `ORG_OFFICE`, else the only
/// office on this Mac (ADR-0003 §2.1). An office that answers but refuses the bind, an office
/// asked for that isn't a plain id, or several offices and none asked for, are failures, never a
/// fall back: the spool would only hide them.
pub(crate) fn route(member: &str, office_flag: Option<&str>) -> Result<Route, Failure> {
    let Some(home) = org_proto::location::default_office_home() else {
        return Ok(Route::NoOffice);
    };
    let from_env = env::var(OFFICE_ENV).ok().filter(|id| !id.is_empty());
    let chosen = office_flag.or(from_env.as_deref());
    decide(find_office(&home, chosen), member, |office_id| {
        open_as(&home, office_id, member)
    })
    .map_err(|why| why.into_failure(member))
}

/// Why [`decide`] found no route.
#[derive(Debug)]
enum NoRoute {
    Find(FindOfficeError),
    Refused {
        office_id: String,
        error: ClientError,
    },
}

impl NoRoute {
    fn into_failure(self, member: &str) -> Failure {
        match self {
            NoRoute::Find(error) => Failure::new(EXIT_USAGE, error.to_string()),
            NoRoute::Refused { office_id, error } => {
                let code = match &error {
                    ClientError::Api(_) => exit_code(&error),
                    _ => EXIT_IO,
                };
                Failure::new(
                    code,
                    format!("office {office_id} did not let '{member}' in: {error}"),
                )
            }
        }
    }
}

/// The routing decision: the office found (or not), then what opening it gave.
fn decide(
    found: Result<Option<String>, FindOfficeError>,
    member: &str,
    open: impl FnOnce(&str) -> Result<(Client, msg::MsgBindResult), OpenError>,
) -> Result<Route, NoRoute> {
    let Some(office_id) = found.map_err(NoRoute::Find)? else {
        return Ok(Route::NoOffice);
    };
    match open(&office_id) {
        Ok((client, _)) => Ok(Route::Office(Office::new(client, office_id, member))),
        Err(OpenError::Unreachable(reason)) => {
            Ok(Route::Unreachable(Unreachable { office_id, reason }))
        }
        Err(OpenError::Refused(error)) => Err(NoRoute::Refused { office_id, error }),
    }
}

/// A connection to the agent's office, bound as the agent.
///
/// The office checks the binding again before every call, and unbinds a connection whose member
/// was moved out, made office-run, offboarded or removed (its calls then answer `not_bound`).
/// Every call here therefore binds again once on `not_bound` and retries (a `send` with the same
/// id, which the office dedups); if the office won't bind the agent again, or unbinds it again,
/// the command fails saying so.
pub(crate) struct Office {
    client: Client,
    office_id: String,
    member: String,
    /// Whether this command has already bound again once.
    rebound: Cell<bool>,
}

/// How a `wait` through the office ended.
pub(crate) enum Waited {
    Message(Box<Message>),
    TimedOut,
    Interrupted,
}

impl Office {
    fn new(client: Client, office_id: String, member: &str) -> Self {
        Office {
            client,
            office_id,
            member: member.to_owned(),
            rebound: Cell::new(false),
        }
    }

    /// Calls `C`, binding again once if the office answers `not_bound`.
    fn call<C: Call>(&self, params: &C::Params) -> Result<C::Result, Failure> {
        loop {
            match self.client.call::<C>(params) {
                Err(error) if is_not_bound(&error) => self.bind_again()?,
                outcome => return outcome.map_err(|e| Failure::from_office(&e)),
            }
        }
    }

    /// After a `not_bound`: binds again, once per command. A refusal, or a second `not_bound`,
    /// ends the command with a message that says what happened.
    fn bind_again(&self) -> Result<(), Failure> {
        let lost = format!(
            "office {} unbound '{}' during the command",
            self.office_id, self.member
        );
        if self.rebound.replace(true) {
            return Err(Failure::new(
                EXIT_USAGE,
                format!("{lost}, again after it bound again; try once more"),
            ));
        }
        match self.client.bind_member(&self.member) {
            Ok(bound) if bound.office_id == self.office_id => Ok(()),
            Ok(_) => Err(Failure::new(
                EXIT_IO,
                format!("{lost}, and the server on its socket is another office"),
            )),
            Err(error) => Err(Failure::new(
                match &error {
                    ClientError::Api(_) => exit_code(&error),
                    _ => EXIT_IO,
                },
                format!("{lost}, and won't bind it again: {error}"),
            )),
        }
    }

    /// `msg send`: the new message's id.
    pub(crate) fn send(&self, to: &str, outgoing: Outgoing) -> Result<String, Failure> {
        let Outgoing {
            subject,
            body,
            meta,
            priority,
        } = outgoing;
        // `--meta priority=...` set the priority on the spool (it carries priority in meta);
        // the office takes it as a field. `--priority` wins.
        let priority = priority.or_else(|| {
            meta.get("priority")
                .and_then(|p| p.as_str())
                .and_then(|p| MessagePriority::parse(p).ok())
        });
        // One id for the message, so a retry after binding again is the same send.
        let sent = self.call::<MsgSend>(&MsgSendParams {
            id: generate_ulid(),
            to: to.to_owned(),
            kind: KIND_MSG.to_owned(),
            priority: priority.map(proto_priority),
            subject,
            body,
            meta,
            reply_to: None,
        })?;
        Ok(sent.id)
    }

    /// `msg inbox`: every page, oldest first and cut to the limit, as the spool lists it.
    /// Headers only: the office's inbox carries no bodies.
    pub(crate) fn inbox(&self, query: &InboxQuery) -> Result<Vec<InboxItem>, Failure> {
        let mut headers = Vec::new();
        let mut cursor = None;
        loop {
            let HeaderPage {
                headers: page,
                next_cursor,
            } = self.call::<MsgInbox>(&MsgInboxParams {
                include_all: query.include_all,
                from: query.from.clone(),
                thread_id: query.thread.clone(),
                cursor,
                limit: Some(MAX_PAGE_LIMIT),
            })?;
            let empty = page.is_empty();
            headers.extend(page);
            match next_cursor {
                // Cursors only go down; anything else would never end.
                Some(next) if !empty && cursor.is_none_or(|previous| next < previous) => {
                    cursor = Some(next);
                }
                _ => break,
            }
        }
        headers.reverse();
        let limit = query.limit.or(if query.summary_only {
            Some(INBOX_SUMMARY_DEFAULT_LIMIT)
        } else {
            None
        });
        if let Some(limit) = limit {
            headers.truncate(limit);
        }
        Ok(headers.iter().map(inbox_item).collect())
    }

    /// `msg read <id>` or `msg read --next` (`id` `None`): the message, marked read; `None`
    /// when `--next` finds the inbox empty.
    pub(crate) fn read(&self, id: Option<&str>) -> Result<Option<Message>, Failure> {
        let read = self.call::<MsgRead>(&MsgReadParams {
            id: id.map(str::to_owned),
        })?;
        Ok(read.message)
    }

    /// `msg wait`: the oldest unread message (not marked read), held for up to `timeout`, in
    /// calls of at most the office's limit; a person's Ctrl-C ends it at once.
    pub(crate) fn wait(
        &self,
        from: Option<&str>,
        urgent_only: bool,
        timeout: Duration,
    ) -> Result<Waited, Failure> {
        let deadline = Instant::now().checked_add(timeout);
        loop {
            let left = deadline.map_or(timeout, |d| d.saturating_duration_since(Instant::now()));
            let slice = left.min(Duration::from_millis(MAX_WAIT_MS));
            let params = MsgWaitParams {
                timeout_ms: u64::try_from(slice.as_millis()).unwrap_or(MAX_WAIT_MS),
                from: from.map(str::to_owned),
                urgent_only,
            };
            let Some(answer) = self.client.call_unless::<MsgWait>(
                &params,
                slice + WAIT_ANSWER_MARGIN,
                is_interrupted,
            ) else {
                return Ok(Waited::Interrupted);
            };
            let answer = match answer {
                // Bound again: wait on for what is left.
                Err(error) if is_not_bound(&error) => {
                    self.bind_again()?;
                    continue;
                }
                other => other.map_err(|e| Failure::from_office(&e))?,
            };
            if let Some(message) = answer.message {
                return Ok(Waited::Message(Box::new(message)));
            }
            if slice >= left {
                return Ok(Waited::TimedOut);
            }
        }
    }

    /// `who`: the directory cards, in the spool's card shape.
    pub(crate) fn who(&self, params: &WhoParams) -> Result<Vec<DirectoryCard>, Failure> {
        let result = self.call::<Who>(params)?;
        Ok(result.members.iter().map(card).collect())
    }
}

fn is_not_bound(error: &ClientError) -> bool {
    matches!(error, ClientError::Api(e) if e.code == ErrorCode::NotBound)
}

/// `who`'s params from the CLI's flags: `--kind owner` (the spool's) is the office's
/// `member: owner`, since the office has no owner kind.
pub(crate) fn who_params(
    member: Option<&str>,
    team: Option<&str>,
    kind: Option<&str>,
) -> Result<WhoParams, String> {
    let mut params = WhoParams {
        member: member.map(str::to_owned),
        team: team.map(str::to_owned),
        kind: None,
    };
    if let Some(kind) = kind {
        match kind.to_ascii_lowercase().as_str() {
            "agent" => params.kind = Some(MemberKind::Agent),
            "person" => params.kind = Some(MemberKind::Person),
            "owner" if member.is_none() => {
                params.member = Some(org_proto::OWNER_HANDLE.to_owned());
            }
            "owner" => {}
            _ => {
                return Err(format!(
                    "invalid --kind '{kind}' (expected agent, person, or owner)"
                ))
            }
        }
    }
    Ok(params)
}

fn proto_priority(priority: MessagePriority) -> msg::MessagePriority {
    match priority {
        MessagePriority::Normal => msg::MessagePriority::Normal,
        MessagePriority::High => msg::MessagePriority::High,
        MessagePriority::Urgent => msg::MessagePriority::Urgent,
    }
}

/// A unit enum's wire name (`"dead_letter"`, `"needs_you"`, ...).
fn wire_name(value: impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => "unknown".to_owned(),
    }
}

/// An inbox row from an office header: the spool's `InboxItem`, with no body or meta (the
/// office's inbox lists headers only).
fn inbox_item(header: &MessageHeader) -> InboxItem {
    InboxItem {
        v: ENVELOPE_VERSION,
        id: header.id.clone(),
        from: header.from_member_id.clone(),
        to: header.to_member_id.clone(),
        kind: header.kind.clone(),
        priority: wire_name(header.priority),
        urgent: header.urgent,
        triage_gen: header.triage_gen,
        status: wire_name(header.state),
        snoozed_until: header.snoozed_until.clone(),
        snooze_note: header.snooze_note.clone(),
        reply_to: header.reply_to.clone(),
        created_at: header.created_at.clone(),
        expires_at: header.expires_at.clone(),
        subject: header.subject.clone(),
        body: None,
        meta: None,
    }
}

/// A message as `read` and `wait` print it: the fields and order of the spool's envelope
/// (`org_spool::Envelope`), so `--json` reads the same either way.
#[derive(Debug, Serialize)]
pub(crate) struct Shown {
    pub v: u32,
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: String,
    pub reply_to: Option<String>,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub subject: Option<String>,
    pub body: String,
    pub meta: BTreeMap<String, serde_json::Value>,
}

impl Shown {
    pub(crate) fn from_message(message: Message) -> Self {
        let Message {
            header,
            body,
            mut meta,
        } = message;
        // The spool carries a message's priority in `meta.priority`; keep it there for readers
        // of the old shape.
        if header.priority != msg::MessagePriority::Normal {
            meta.entry("priority".to_owned())
                .or_insert_with(|| serde_json::Value::String(wire_name(header.priority)));
        }
        Shown {
            v: ENVELOPE_VERSION,
            id: header.id,
            from: header.from_member_id,
            to: header.to_member_id,
            kind: header.kind,
            reply_to: header.reply_to,
            created_at: header.created_at,
            expires_at: header.expires_at,
            subject: header.subject,
            body,
            meta,
        }
    }
}

/// A directory card from the office, in the spool's card shape.
fn card(card: &directory::DirectoryCard) -> DirectoryCard {
    DirectoryCard {
        id: card.id.clone(),
        name: card.name.clone(),
        kind: wire_name(card.kind),
        owner: card.owner,
        role_title: card.role_title.clone(),
        teams: card
            .teams
            .iter()
            .map(|t| TeamEntry {
                id: t.id.clone(),
                name: t.name.clone(),
                title_in_team: t.title_in_team.clone(),
            })
            .collect(),
        reports_to: card.reports_to.as_ref().map(|r| RefEntry {
            id: r.id.clone(),
            name: r.name.clone(),
        }),
        office: card.office.as_ref().map(|o| RefEntry {
            id: o.id.clone(),
            name: o.name.clone(),
        }),
        presence: wire_name(card.presence),
        last_seen_at: card.last_seen_at.clone(),
        status: card.status.clone(),
        status_expires_at: card.status_expires_at.clone(),
        current_task_id: card.current_task_id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::sync::mpsc;

    use org_proto::msg::{MessageLifecycleState, MsgBind, MsgBindResult};
    use org_proto::{ApiError, Request, Response};
    use org_spool::Envelope;

    use super::*;

    fn api(code: ErrorCode) -> ClientError {
        ClientError::Api(ApiError::new(code, "fixed words"))
    }

    fn not_running() -> ClientError {
        ClientError::NotRunning(io::Error::from(io::ErrorKind::ConnectionRefused))
    }

    /// A connected client for `decide`'s happy path: any socket will do.
    fn a_client() -> (tempfile::TempDir, Client) {
        let dir = tempfile::Builder::new()
            .prefix("oar")
            .tempdir_in("/tmp")
            .unwrap();
        let path = dir.path().join("s.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            let held = listener.accept();
            std::thread::sleep(Duration::from_secs(5));
            drop(held);
        });
        let client = Client::connect(&path).unwrap();
        (dir, client)
    }

    fn bound() -> MsgBindResult {
        MsgBindResult {
            member_id: Some("M1".into()),
            office_id: "O1".into(),
        }
    }

    #[test]
    fn no_office_on_this_mac_is_the_spool_and_nothing_is_opened() {
        let route = decide(Ok(None), "pixel", |_| {
            panic!("opened an office that isn't there")
        });
        assert!(matches!(route, Ok(Route::NoOffice)));
    }

    #[test]
    fn a_reachable_office_is_the_office() {
        let (_dir, client) = a_client();
        let mut asked = None;
        let route = decide(Ok(Some("O1".into())), "pixel", |id| {
            asked = Some(id.to_owned());
            Ok((client, bound()))
        });
        assert!(matches!(route, Ok(Route::Office(_))));
        assert_eq!(asked.as_deref(), Some("O1"));
    }

    #[test]
    fn an_office_that_does_not_answer_falls_back_and_says_why_without_a_body() {
        let route = decide(Ok(Some("O1".into())), "pixel", |_| {
            Err(OpenError::Unreachable(not_running()))
        });
        let Ok(Route::Unreachable(why)) = route else {
            panic!("expected a fall back");
        };
        let queued = why.queued_notice();
        assert!(queued.contains("office O1 is not reachable"), "{queued}");
        assert!(queued.contains("queued in the local spool"), "{queued}");
        assert!(why.spool_notice().contains("showing the local spool"));
    }

    #[test]
    fn an_office_that_refuses_the_bind_is_a_failure_not_a_fall_back() {
        let route = decide(Ok(Some("O1".into())), "pixel", |_| {
            Err(OpenError::Refused(api(ErrorCode::NotAuthorized)))
        });
        let Err(why) = route else {
            panic!("a refused bind fell back");
        };
        let failure = why.into_failure("pixel");
        assert_eq!(failure.code, EXIT_USAGE);
        let text = failure.message.unwrap();
        assert!(
            text.contains("office O1") && text.contains("'pixel'"),
            "{text}"
        );
        // A refusal that isn't the office's answer (e.g. another protocol version) is exit 4.
        let other = decide(Ok(Some("O1".into())), "pixel", |_| {
            Err(OpenError::Refused(ClientError::VersionMismatch {
                client: 1,
                server: 2,
            }))
        });
        assert_eq!(other.err().unwrap().into_failure("pixel").code, EXIT_IO);
    }

    #[test]
    fn several_offices_and_none_chosen_is_a_usage_error_naming_the_way_out() {
        let route = decide(
            Err(FindOfficeError::Ambiguous(vec!["A1".into(), "B1".into()])),
            "pixel",
            |_| panic!("guessed an office"),
        );
        let failure = route.err().unwrap().into_failure("pixel");
        assert_eq!(failure.code, EXIT_USAGE);
        assert!(failure.message.unwrap().contains("ORG_OFFICE"));
    }

    /// What the scripted office answers to each request, in order.
    type Answer = Box<dyn FnOnce(&Request) -> Response + Send>;

    fn not_bound() -> Answer {
        Box::new(|r: &Request| Response::error(r.id, ApiError::new(ErrorCode::NotBound, "bind")))
    }

    fn bind_ok() -> Answer {
        Box::new(|r: &Request| Response::ok::<MsgBind>(r.id, &bound()).unwrap())
    }

    /// An `Office` on a socket that answers each request with the next of `answers`, and the
    /// requests it got.
    fn scripted(answers: Vec<Answer>) -> (tempfile::TempDir, Office, mpsc::Receiver<Request>) {
        let dir = tempfile::Builder::new()
            .prefix("oas")
            .tempdir_in("/tmp")
            .unwrap();
        let path = dir.path().join("s.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let (seen, got) = mpsc::channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = io::BufReader::new(stream.try_clone().unwrap());
            for answer in answers {
                let Ok(Some(line)) = org_proto::frame::read_line(&mut reader) else {
                    return;
                };
                let request: Request = serde_json::from_slice(&line).unwrap();
                let mut reply = serde_json::to_vec(&answer(&request)).unwrap();
                reply.push(b'\n');
                // Recorded before the answer goes, so the caller sees every request.
                let _ = seen.send(request);
                (&stream).write_all(&reply).unwrap();
            }
            let _ = org_proto::frame::read_line(&mut reader);
        });
        let client = Client::connect(&path).unwrap();
        (dir, Office::new(client, "O1".into(), "pixel"), got)
    }

    fn methods(got: &mpsc::Receiver<Request>) -> Vec<Request> {
        got.try_iter().collect()
    }

    #[test]
    fn a_call_answered_not_bound_binds_again_once_and_is_retried() {
        let page: Answer = Box::new(|r: &Request| {
            Response::ok::<MsgInbox>(
                r.id,
                &HeaderPage {
                    headers: vec![header()],
                    next_cursor: None,
                },
            )
            .unwrap()
        });
        let (_dir, office, got) = scripted(vec![not_bound(), bind_ok(), page]);
        let items = office.inbox(&InboxQuery::default()).unwrap();
        assert_eq!(items.len(), 1);
        let requests = methods(&got);
        let names: Vec<&str> = requests.iter().map(|r| r.method.as_str()).collect();
        assert_eq!(names, ["msg.inbox", "msg.bind", "msg.inbox"]);
        assert_eq!(requests[1].params, serde_json::json!({"member": "pixel"}));
    }

    #[test]
    fn a_send_retried_after_binding_again_keeps_its_id() {
        let sent: Answer = Box::new(|r: &Request| {
            let id = r.params["id"].as_str().unwrap().to_owned();
            Response::ok::<MsgSend>(
                r.id,
                &msg::SendResult {
                    id,
                    thread_id: "T".into(),
                    local_seq: 1,
                    origin_seq: 1,
                },
            )
            .unwrap()
        });
        let (_dir, office, got) = scripted(vec![not_bound(), bind_ok(), sent]);
        let id = office
            .send(
                "nova",
                Outgoing {
                    body: "b".into(),
                    ..Outgoing::default()
                },
            )
            .unwrap();
        let requests = methods(&got);
        assert_eq!(requests[0].method, "msg.send");
        assert_eq!(requests[2].method, "msg.send");
        assert_eq!(requests[0].params["id"], requests[2].params["id"]);
        assert_eq!(requests[2].params["id"], id.as_str());
    }

    #[test]
    fn an_office_that_will_not_bind_the_agent_again_ends_the_command_saying_so() {
        let refused: Answer = Box::new(|r: &Request| {
            Response::error(
                r.id,
                ApiError::new(ErrorCode::NotAuthorized, "not authorized"),
            )
        });
        let (_dir, office, _got) = scripted(vec![not_bound(), refused]);
        let failure = office.read(None).unwrap_err();
        assert_eq!(failure.code, EXIT_USAGE);
        let text = failure.message.unwrap();
        assert!(text.contains("office O1 unbound 'pixel'"), "{text}");
        assert!(
            text.contains("won't bind it again: not authorized"),
            "{text}"
        );
    }

    #[test]
    fn unbound_again_after_binding_again_is_not_retried_forever() {
        let (_dir, office, got) = scripted(vec![not_bound(), bind_ok(), not_bound()]);
        let failure = office.who(&WhoParams::default()).unwrap_err();
        assert_eq!(failure.code, EXIT_USAGE);
        assert!(failure.message.unwrap().contains("again"));
        assert_eq!(methods(&got).len(), 3);
    }

    #[test]
    fn office_errors_map_to_the_cli_exit_codes() {
        assert_eq!(exit_code(&api(ErrorCode::NotFound)), EXIT_USAGE);
        assert_eq!(exit_code(&api(ErrorCode::NotBound)), EXIT_USAGE);
        assert_eq!(exit_code(&api(ErrorCode::NotAuthorized)), EXIT_USAGE);
        assert_eq!(exit_code(&api(ErrorCode::TooLarge)), EXIT_USAGE);
        assert_eq!(exit_code(&api(ErrorCode::InvalidRequest)), EXIT_USAGE);
        assert_eq!(exit_code(&api(ErrorCode::Timeout)), EXIT_TIMEOUT);
        assert_eq!(
            exit_code(&api(ErrorCode::UnknownRecipient)),
            EXIT_UNKNOWN_RECIPIENT
        );
        assert_eq!(
            exit_code(&api(ErrorCode::Unavailable)),
            EXIT_OFFICE_UNAVAILABLE
        );
        assert_eq!(exit_code(&api(ErrorCode::StorageError)), EXIT_IO);
        assert_eq!(exit_code(&api(ErrorCode::Unknown)), EXIT_IO);
        assert_eq!(exit_code(&ClientError::Timeout), EXIT_IO);
        assert_eq!(exit_code(&ClientError::Disconnected), EXIT_IO);
    }

    fn header() -> MessageHeader {
        MessageHeader {
            id: "01MSG0000000000000000000A1".into(),
            thread_id: "01MSG0000000000000000000A1".into(),
            reply_to: None,
            from_member_id: "M-PIXEL".into(),
            from_display_name: "Pixel".into(),
            to_member_id: "M-NOVA".into(),
            to_display_name: "nova".into(),
            kind: KIND_MSG.into(),
            priority: msg::MessagePriority::High,
            urgent: true,
            subject: Some("hi".into()),
            created_at: "2026-11-09T10:00:00.000Z".into(),
            expires_at: None,
            ttl_ms: None,
            body_bytes: 7,
            meta_bytes: 0,
            local_seq: 3,
            state: MessageLifecycleState::DeadLetter,
            triage_gen: 2,
            snoozed_until: None,
            snooze_after_task: false,
            snooze_note: None,
            read_at: None,
            expired_waiting_person: false,
        }
    }

    #[test]
    fn an_inbox_row_has_the_spool_shape_and_no_body_or_meta() {
        let item = inbox_item(&header());
        assert_eq!(item.from, "M-PIXEL");
        assert_eq!(item.to, "M-NOVA");
        assert_eq!(item.priority, "high");
        assert_eq!(item.status, "dead_letter");
        assert_eq!(item.triage_gen, 2);
        let json = serde_json::to_value(&item).unwrap();
        let object = json.as_object().unwrap();
        assert!(!object.contains_key("body") && !object.contains_key("meta"));
        for key in [
            "v",
            "id",
            "from",
            "to",
            "kind",
            "priority",
            "urgent",
            "triage_gen",
            "status",
            "snoozed_until",
            "snooze_note",
            "reply_to",
            "created_at",
            "expires_at",
            "subject",
        ] {
            assert!(object.contains_key(key), "{key}");
        }
    }

    #[test]
    fn a_shown_message_prints_exactly_like_a_spool_envelope() {
        let mut meta = BTreeMap::new();
        meta.insert("k".to_owned(), serde_json::json!("v"));
        let message = Message {
            header: header(),
            body: "the body".into(),
            meta: meta.clone(),
        };
        let shown = serde_json::to_string(&Shown::from_message(message)).unwrap();
        meta.insert("priority".to_owned(), serde_json::json!("high"));
        let envelope = Envelope {
            v: ENVELOPE_VERSION,
            id: "01MSG0000000000000000000A1".into(),
            from: "M-PIXEL".into(),
            to: "M-NOVA".into(),
            kind: org_spool::MessageKind::Msg,
            reply_to: None,
            created_at: "2026-11-09T10:00:00.000Z".into(),
            expires_at: None,
            subject: Some("hi".into()),
            body: "the body".into(),
            meta,
        };
        assert_eq!(shown, serde_json::to_string(&envelope).unwrap());
    }

    #[test]
    fn a_normal_message_gets_no_priority_in_meta_and_its_own_meta_wins() {
        let mut plain = header();
        plain.priority = msg::MessagePriority::Normal;
        let shown = Shown::from_message(Message {
            header: plain,
            body: String::new(),
            meta: BTreeMap::new(),
        });
        assert!(shown.meta.is_empty());
        let mut own = BTreeMap::new();
        own.insert("priority".to_owned(), serde_json::json!("custom"));
        let shown = Shown::from_message(Message {
            header: header(),
            body: String::new(),
            meta: own,
        });
        assert_eq!(shown.meta["priority"], "custom");
    }

    #[test]
    fn who_flags_become_the_offices_params() {
        let all = who_params(None, None, None).unwrap();
        assert_eq!(all, WhoParams::default());
        let owner = who_params(None, Some("T1"), Some("Owner")).unwrap();
        assert_eq!(owner.member.as_deref(), Some("owner"));
        assert_eq!(owner.team.as_deref(), Some("T1"));
        assert_eq!(owner.kind, None);
        let agents = who_params(None, None, Some("agent")).unwrap();
        assert_eq!(agents.kind, Some(MemberKind::Agent));
        let named = who_params(Some("nova"), None, Some("owner")).unwrap();
        assert_eq!(named.member.as_deref(), Some("nova"));
        let bad = who_params(None, None, Some("robot")).unwrap_err();
        assert_eq!(
            bad,
            "invalid --kind 'robot' (expected agent, person, or owner)"
        );
    }

    #[test]
    fn an_office_card_keeps_the_spool_card_fields() {
        let office_card: directory::DirectoryCard = serde_json::from_value(serde_json::json!({
            "id": "M1", "name": "Nova", "handle": "nova", "kind": "agent",
            "agent_status": "active", "role_title": "Engineer",
            "teams": [{"id": "T1", "name": "Core", "title_in_team": "Lead"}],
            "reports_to": {"id": "M0", "name": "Ada", "handle": "ada"},
            "office": {"id": "O1", "name": "HQ"},
            "presence": "needs_you", "status": "on it"
        }))
        .unwrap();
        let spool_card = card(&office_card);
        assert_eq!(spool_card.kind, "agent");
        assert_eq!(spool_card.presence, "needs_you");
        assert_eq!(spool_card.teams[0].title_in_team.as_deref(), Some("Lead"));
        assert_eq!(spool_card.reports_to.unwrap().name, "Ada");
        assert!(!spool_card.owner);
    }
}
