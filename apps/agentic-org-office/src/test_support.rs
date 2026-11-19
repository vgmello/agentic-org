//! Test support shared by the in-process tests and the binary suite (feature `test-support`):
//! one grant signer for the test grant key, and a raw-JSON caller for a host channel.
//!
//! Persons and the owner act only through grants on host channels, so every test that sends an
//! organization command signs a grant here. Nothing in this module is compiled into a release.
use hmac::{Hmac, Mac};
use org_proto::{grant_mac_input, Grant, GrantKey};
use serde_json::{json, Value};
use sha2::Sha256;
use std::io::BufReader;
use std::os::unix::net::UnixStream;

/// The grant key the tests launch an office with; `[7; 32]`, as the launch handoff tests use.
pub const TEST_GRANT_KEY: [u8; 32] = [7; 32];

/// The test grant key as the launch hands it over.
pub fn grant_key() -> GrantKey {
    GrantKey::from_bytes(TEST_GRANT_KEY)
}

/// The claims of a grant for `member` on `channel_id`, in session `session_id`, that never
/// expires. Its MAC is a placeholder until [`sign_grant`] runs.
pub fn grant(
    member: &str,
    org_id: &str,
    office_id: &str,
    channel_id: &str,
    session_id: &str,
) -> Value {
    json!({"account":"test-account","member":member,"org_id":org_id,"office_id":office_id,
        "channel_id":channel_id,"session_id":session_id,"exp":u64::MAX,"mac":"00".repeat(32)})
}

/// Sets the grant's `mac` to the HMAC-SHA256 of its claims under [`TEST_GRANT_KEY`].
pub fn sign_grant(grant: &mut Value) {
    let parsed: Grant = serde_json::from_value(grant.clone()).expect("a grant's claims");
    let mut mac = Hmac::<Sha256>::new_from_slice(&TEST_GRANT_KEY).expect("an HMAC key");
    mac.update(&grant_mac_input(&parsed.claims).expect("canonical claims"));
    let mut tag = mac.finalize().into_bytes();
    grant["mac"] = tag
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        .into();
    tag.fill(0);
}

/// A signed grant: [`grant`] then [`sign_grant`].
pub fn signed_grant(
    member: &str,
    org_id: &str,
    office_id: &str,
    channel_id: &str,
    session_id: &str,
) -> Value {
    let mut value = grant(member, org_id, office_id, channel_id, session_id);
    sign_grant(&mut value);
    value
}

/// A raw-JSON caller on the peer end of a host channel. Each call writes one frame and reads the
/// next; `principal` is attached to every call after `hello` when set.
pub struct ChannelCaller {
    peer: BufReader<UnixStream>,
    next: u64,
    principal: Option<Value>,
}

impl ChannelCaller {
    /// A caller on `peer`, with no principal yet.
    pub fn new(peer: UnixStream) -> Self {
        Self {
            peer: BufReader::with_capacity(1, peer),
            next: 0,
            principal: None,
        }
    }

    /// Acts with `grant` (a signed grant) on every later call.
    pub fn with_grant(mut self, grant: Value) -> Self {
        self.principal = Some(json!({ "grant": grant }));
        self
    }

    /// `hello` at api version 2; returns the whole answer.
    pub fn hello(&mut self) -> Value {
        self.raw(json!({"method":"hello","params":{"api":{"min":2,"max":2}}}))
    }

    /// `method` with `params`, carrying the principal; returns the whole answer.
    pub fn call(&mut self, method: &str, params: Value) -> Value {
        let mut frame = json!({ "method": method, "params": params });
        if let Some(principal) = &self.principal {
            frame["principal"] = principal.clone();
        }
        self.raw(frame)
    }

    /// Sends `frame` with the next id and reads one answer frame.
    pub fn raw(&mut self, mut frame: Value) -> Value {
        self.next += 1;
        frame["id"] = self.next.into();
        org_proto::frame::write_message(self.peer.get_mut(), &frame).expect("write a frame");
        org_proto::frame::read_message::<Value>(&mut self.peer)
            .expect("read a frame")
            .expect("an answer, not the end of the channel")
    }
}

/// The host's writer custody, as tests run it: it sends `admission` first, then answers
/// `checkpoint` with the head it holds and applies the host's rule to every `confirm`
/// (`org_office_launch::writer::confirms`). A refused confirmation is denied and ends the
/// channel, as the host's does. One `FakeWriter` keeps its head across channels, so an office
/// reopened on a new channel meets the head its last run confirmed.
#[derive(Clone)]
pub struct FakeWriter {
    admission: org_office_launch::writer::Admission,
    state: std::sync::Arc<std::sync::Mutex<FakeState>>,
}

#[derive(Default)]
struct FakeState {
    head: Option<org_office_launch::writer::Head>,
    confirms: usize,
}

/// A valid admission for tests: the given organization, office and owner, a fixed device
/// and incarnation, epoch 1, the owner account `01ARZ3NDEKTSV4RRFFQ69G5FAV`.
pub fn test_admission(
    instance: &str,
    organization: &str,
    office: &str,
    owner_member: &str,
    durable: &std::path::Path,
    runtime: &std::path::Path,
) -> org_office_launch::writer::Admission {
    use org_office_launch::writer::{Admission, Identity, VERSION};
    Admission {
        version: VERSION,
        identity: Identity {
            instance_id: instance.into(),
            organization_id: organization.into(),
            office_id: office.into(),
            journal_incarnation: "01900000-0000-7000-8000-00000000ab01".into(),
            writer_epoch: 1,
            machine_id: "ab".repeat(32),
        },
        durable_root: durable.to_str().expect("a UTF-8 root").into(),
        runtime_root: runtime.to_str().expect("a UTF-8 root").into(),
        owner_account: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        owner_member: owner_member.into(),
    }
}

impl FakeWriter {
    pub fn new(admission: org_office_launch::writer::Admission) -> Self {
        Self {
            admission,
            state: Default::default(),
        }
    }

    pub fn admission(&self) -> &org_office_launch::writer::Admission {
        &self.admission
    }

    /// The head the host holds: the last one it confirmed.
    pub fn head(&self) -> Option<org_office_launch::writer::Head> {
        self.state.lock().expect("fake writer").head.clone()
    }

    /// Sets the head the host holds, as a host that confirmed it would.
    pub fn set_head(&self, head: Option<org_office_launch::writer::Head>) {
        self.state.lock().expect("fake writer").head = head;
    }

    /// How many confirmations it was asked for, accepted or not.
    pub fn confirms(&self) -> usize {
        self.state.lock().expect("fake writer").confirms
    }

    /// A new channel: the worker's end, and the thread that serves the host's end until the
    /// worker closes it or a request is refused.
    pub fn channel(&self) -> (UnixStream, std::thread::JoinHandle<()>) {
        let (worker, host) = UnixStream::pair().expect("a socket pair");
        (worker, self.serve(host))
    }

    /// Serves the host's end of one channel on a thread.
    pub fn serve(&self, host: UnixStream) -> std::thread::JoinHandle<()> {
        use org_office_launch::writer::{self as wire, confirms, Request, Response};
        let this = self.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(host);
            if wire::write(reader.get_mut(), &this.admission).is_err() {
                return;
            }
            while let Ok(Some(request)) = wire::read::<Request>(&mut reader) {
                let ours = |identity: &org_office_launch::writer::Identity| {
                    identity == &this.admission.identity
                };
                let response = match request {
                    Request::Checkpoint { identity } if ours(&identity) => {
                        Response::Checkpoint { head: this.head() }
                    }
                    Request::Confirm {
                        identity,
                        before,
                        after,
                    } if ours(&identity) => {
                        let mut state = this.state.lock().expect("fake writer");
                        state.confirms += 1;
                        if confirms(state.head.as_ref(), before.as_ref(), &after) {
                            state.head = Some(after);
                            Response::Confirmed
                        } else {
                            Response::Denied
                        }
                    }
                    Request::Owner { identity } if ours(&identity) => Response::Owner {
                        account: this.admission.owner_account.clone(),
                        member: this.admission.owner_member.clone(),
                    },
                    _ => Response::Denied,
                };
                let denied = matches!(response, Response::Denied);
                if wire::write(reader.get_mut(), &response).is_err() || denied {
                    return;
                }
            }
        })
    }

    /// The custody and roots an office takes from a new channel, as the worker takes them
    /// from the host: through the worker's own adapter.
    pub fn admitted(
        &self,
    ) -> (
        org_office::journal_office::Custody,
        org_office::journal_office::DataRoots,
    ) {
        let (worker, _thread) = self.channel();
        let identity = &self.admission.identity;
        let admitted =
            crate::transfer::install_writer(worker, &identity.instance_id, &identity.office_id)
                .expect("the fake writer's admission");
        (admitted.custody, admitted.roots)
    }
}
