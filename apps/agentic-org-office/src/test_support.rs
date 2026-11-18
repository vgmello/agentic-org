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
