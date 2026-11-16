//! Scripted native transport exercises the real browser registry, control actor,
//! descriptor handoff and approved client. It proves no production admission.
use super::*;
use crate::sessions::{BrowserSessions, Established, SessionAuthority};
use org_proto::{frame, Grant, GrantClaims, GrantMac, HelloResult};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::BufReader,
    os::{fd::AsFd, unix::net::UnixDatagram},
    thread::JoinHandle,
};

const INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
#[derive(Default)]
struct State {
    issued: usize,
    opened: usize,
    granted: usize,
    worker_generation: usize,
    channels: BTreeMap<String, usize>,
    revoked: BTreeSet<String>,
    unlinked: BTreeSet<String>,
    fail_grants: bool,
    refuse_open: bool,
    revoke_on_stale: bool,
    unlink_on_stale: bool,
    workers: Vec<UnixStream>,
    pending_descriptor: Option<UnixStream>,
    requests: Vec<String>,
}
struct Fixture {
    sessions: Arc<BrowserSessions>,
    state: Arc<Mutex<State>>,
    stop: UnixStream,
    host: Option<JoinHandle<()>>,
}
fn links() -> Value {
    json!([{"org_id":"org","member_id":"owner","office_ids":["office"]}])
}
fn scope() -> OfficeScope {
    OfficeScope {
        kind: org_proto::selected_tasks::OfficeKind::Office,
        instance_id: INSTANCE.into(),
        org_id: "org".into(),
        office_id: "office".into(),
    }
}

impl State {
    fn response(&mut self, request: &Value) -> (Value, Option<(String, UnixStream)>) {
        let method = request["method"].as_str().unwrap();
        let p = &request["params"];
        let handle = p["session"].as_str().unwrap_or("");
        if self.requests.len() == 16 {
            self.requests.remove(0);
        }
        self.requests.push(method.into());
        let mut descriptor = None;
        let result = if method != "hello"
            && method != "account.verify"
            && self.revoked.contains(handle)
        {
            Err("not_authorized")
        } else {
            match method {
                "hello" => Ok(serde_json::to_value(
                    HelloResult::host("test", "disposable").with_instance_id(INSTANCE),
                )
                .unwrap()),
                "account.verify" => {
                    self.issued += 1;
                    Ok(
                        json!({"session":format!("generated-session-{}",self.issued),"account":"account","admin":true,"orgs":links()}),
                    )
                }
                "session.inspect" => Ok(
                    json!({"account":"account","login":"Owner","admin":true,"orgs":if self.unlinked.contains(handle) {json!([])} else {links()},"server_time_ms":1794312000000u64,"absolute_expires_at_ms":1794355200000u64,"idle_expires_at_ms":1794313800000u64}),
                ),
                "session.renew" | "session.end" => Ok(json!({})),
                "fds.received" => {
                    self.pending_descriptor.take();
                    Ok(json!({}))
                }
                "office.open_channel" => self.open_channel(p).map(|(channel, client)| {
                    descriptor = Some((channel.clone(), client));
                    json!({"channel_id":channel})
                }),
                "session.grant" => self.grant(handle, p["channel_id"].as_str().unwrap()),
                _ => panic!("unexpected native request {method}"),
            }
        };
        let id = request["id"].as_u64().unwrap();
        let reply = match result {
            Ok(result) => json!({"id":id,"result":result}),
            Err(code) => json!({"id":id,"error":{"code":code,"message":"fixture refusal"}}),
        };
        (reply, descriptor)
    }
    fn open_channel(&mut self, p: &Value) -> Result<(String, UnixStream), &'static str> {
        self.opened += 1;
        if self.refuse_open {
            return Err("unavailable");
        }
        let handle = p["session"].as_str().unwrap_or("");
        if self.unlinked.contains(handle) || p["office_id"] != "office" {
            return Err("not_found");
        }
        let channel = format!("channel-{}", self.opened);
        self.channels
            .insert(channel.clone(), self.worker_generation);
        let (client, worker) = UnixStream::pair().unwrap();
        self.workers.push(worker.try_clone().unwrap());
        thread::spawn(move || serve_worker(worker));
        Ok((channel, client))
    }
    fn grant(&mut self, handle: &str, channel: &str) -> Result<Value, &'static str> {
        self.granted += 1;
        if self.fail_grants || self.channels.get(channel) != Some(&self.worker_generation) {
            if self.revoke_on_stale {
                self.revoked.insert(handle.into());
            }
            if self.unlink_on_stale {
                self.unlinked.insert(handle.into());
            }
            return Err("not_authorized");
        }
        Ok(serde_json::to_value(ops::SessionGrantResult {
            grant: Grant {
                claims: GrantClaims {
                    account: "account".into(),
                    member: "owner".into(),
                    org_id: "org".into(),
                    office_id: "office".into(),
                    channel_id: channel.into(),
                    session_id: handle.into(),
                    exp: 1794355200000,
                },
                mac: GrantMac::from_bytes([7; 32]),
            },
        })
        .unwrap())
    }
}
fn serve_host(host: UnixStream, transfer: UnixDatagram, observed: Arc<Mutex<State>>) {
    let mut host = BufReader::new(host);
    while let Ok(Some(request)) = frame::read_message::<Value>(&mut host) {
        let (reply, descriptor) = observed.lock().unwrap().response(&request);
        if frame::write_message(host.get_mut(), &reply).is_err() {
            break;
        }
        if let Some((channel_id, client)) = descriptor {
            let record = org_office_launch::v3::TransferRecord::Channel {
                kind: org_office_launch::v3::ChannelKind::Gateway,
                office_id: "office".into(),
                channel_id,
            };
            org_socket::rights::send(
                transfer.as_fd(),
                &serde_json::to_vec(&record).unwrap(),
                &[client.as_fd()],
                Instant::now() + DEADLINE,
            )
            .unwrap();
            // Match the host's transfer acknowledgement protocol. On
            // macOS, last-reference close before receive can flush a
            // UNIX socket in flight even after sendmsg succeeds.
            observed.lock().unwrap().pending_descriptor = Some(client);
        }
    }
}
fn serve_worker(worker: UnixStream) {
    let mut worker = BufReader::new(worker);
    loop {
        let request = match frame::read_message::<Value>(&mut worker) {
            Ok(Some(request)) => request,
            Ok(None) => break,
            Err(error) => {
                eprintln!("fixture worker read failed: {error:?}");
                break;
            }
        };
        assert_eq!(
            request["method"], "hello",
            "channel acquisition must never replay a task command"
        );
        let mut hello = HelloResult::host("test", "disposable").with_instance_id(INSTANCE);
        hello.api_version = 2;
        hello.office=Some(serde_json::from_value(json!({"organization_name":"Org","office":{"id":"office","organization_id":"org","name":"HQ","host":"disposable"}})).unwrap());
        if frame::write_message(
            worker.get_mut(),
            &json!({"id":request["id"],"result":hello}),
        )
        .is_err()
        {
            break;
        }
    }
}

impl Fixture {
    fn new() -> Self {
        let (gateway, host) = UnixStream::pair().unwrap();
        let stop = host.try_clone().unwrap();
        let (transfer, host_transfer) = UnixDatagram::pair().unwrap();
        let state = Arc::new(Mutex::new(State::default()));
        let observed = Arc::clone(&state);
        let host = thread::spawn(move || serve_host(host, host_transfer, observed));
        let control = HostControl::from_handoff(
            gateway.into(),
            transfer.into(),
            INSTANCE.into(),
            Arc::new(|| panic!("invalid ancillary frame")),
        )
        .unwrap();
        let sessions = BrowserSessions::new(control, INSTANCE.into(), false)
            .unwrap_or_else(|_| panic!("sessions"));
        Self {
            sessions,
            state,
            stop,
            host: Some(host),
        }
    }
    fn sign_in(&self) -> Established {
        self.sessions
            .sign_in(
                "Owner".into(),
                Password::new("generated unit credential".into()).unwrap(),
            )
            .unwrap_or_else(|_| panic!("sign-in"))
    }
    fn ticket(&self, e: &Established) -> crate::sessions::Ticket {
        let parts = hyper::Request::builder()
            .uri("/api/v1/session")
            .header("Cookie", e.cookie.split(';').next().unwrap())
            .header("X-Org-Session-Proof", e.proof.expose())
            .body(())
            .unwrap()
            .into_parts()
            .0;
        self.sessions
            .authenticate(&parts)
            .unwrap_or_else(|_| panic!("authenticate"))
    }
    fn access(&self, e: &Established) -> Result<OfficeAccess, crate::service::ApiReply> {
        let result = self
            .sessions
            .office(self.ticket(e), &scope(), Duration::from_secs(1));
        if let Err(error) = &result {
            let s = self.state.lock().unwrap();
            eprintln!("access refusal: status={} control_live={} opened={} granted={} recent_methods={:?}",error.inspected().0,self.sessions.host.live(),s.opened,s.granted,s.requests);
        }
        result
    }
    fn expire(&self, e: &Established, absolute: bool) {
        let ticket = self.ticket(e);
        let key = ticket.key;
        drop(ticket);
        let mut sessions = self.sessions.sessions.lock().unwrap();
        let s = Arc::get_mut(sessions.get_mut(&key).unwrap()).unwrap();
        if absolute {
            s.created = Instant::now() - crate::sessions::ABSOLUTE;
        } else {
            *s.touched.get_mut().unwrap() = Instant::now() - crate::sessions::IDLE;
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.shutdown(Shutdown::Both).unwrap();
        self.host.take().unwrap().join().unwrap();
        for worker in &self.state.lock().unwrap().workers {
            let _ = worker.shutdown(Shutdown::Both);
        }
    }
}

#[test]
fn expired_browser_handles_release_real_cache_capacity_and_preserve_an_unrelated_live_session() {
    let f = Fixture::new();
    let live = f.sign_in();
    let original = f
        .access(&live)
        .unwrap_or_else(|_| panic!("live access"))
        .principal
        .grant
        .claims
        .channel_id;
    for n in 0..257 {
        let abandoned = f.sign_in();
        assert!(
            f.access(&abandoned).is_ok(),
            "cache exhausted at abandoned session {n}"
        );
        f.expire(&abandoned, n % 2 == 0);
    }
    let next = f.sign_in();
    assert!(f.access(&next).is_ok());
    assert_eq!(
        f.access(&live)
            .unwrap_or_else(|_| panic!("unrelated live access"))
            .principal
            .grant
            .claims
            .channel_id,
        original
    );
    assert_eq!(f.sessions.host.channels.entries_for_test().len(), 2);
}

#[test]
fn rejected_host_session_removes_its_channels_without_disturbing_live_sessions() {
    let f = Fixture::new();
    let revoked = f.sign_in();
    let live = f.sign_in();
    assert!(f.access(&revoked).is_ok());
    assert!(f.access(&live).is_ok());
    let handle = f.ticket(&revoked).session.handle.expose().to_owned();
    f.state.lock().unwrap().revoked.insert(handle);
    assert!(f.sessions.get(f.ticket(&revoked)).is_err());
    assert_eq!(f.sessions.host.channels.entries_for_test().len(), 1);
    assert!(f.access(&live).is_ok());
    assert!(f.sessions.host.live());
}

#[test]
fn stale_worker_channel_reacquires_once_for_the_same_live_session_without_commands() {
    let f = Fixture::new();
    let e = f.sign_in();
    let first = f.access(&e).unwrap_or_else(|_| panic!("first access"));
    f.state.lock().unwrap().worker_generation += 1;
    let second = f.access(&e).unwrap_or_else(|_| panic!("reacquisition"));
    assert_ne!(
        first.principal.grant.claims.channel_id,
        second.principal.grant.claims.channel_id
    );
    assert_eq!(
        first.principal.grant.claims.session_id,
        second.principal.grant.claims.session_id
    );
    assert_eq!(f.state.lock().unwrap().opened, 2);
    assert_eq!(f.state.lock().unwrap().granted, 3);
}

#[test]
fn stale_worker_failure_is_bounded_and_revoked_or_unlinked_sessions_cannot_reopen() {
    for failure in ["grants", "open", "revoked", "unlinked"] {
        let f = Fixture::new();
        let e = f.sign_in();
        assert!(f.access(&e).is_ok());
        let handle = f.ticket(&e).session.handle.expose().to_owned();
        {
            let mut s = f.state.lock().unwrap();
            s.worker_generation += 1;
            match failure {
                "grants" => s.fail_grants = true,
                "open" => s.refuse_open = true,
                "revoked" => {
                    s.revoked.insert(handle);
                }
                "unlinked" => {
                    s.unlinked.insert(handle);
                }
                _ => unreachable!(),
            }
        }
        let error = f.access(&e).err().expect("must fail closed");
        let (opened, granted) = {
            let s = f.state.lock().unwrap();
            (s.opened, s.granted)
        };
        match failure {
            "grants" => assert_eq!((opened, granted), (2, 3)),
            "open" => assert_eq!((opened, granted), (2, 2)),
            "revoked" => {
                assert_eq!(opened, 1);
                assert_eq!(error.inspected().0, hyper::StatusCode::UNAUTHORIZED);
            }
            "unlinked" => {
                assert_eq!(opened, 1);
                assert_eq!(error.inspected().0, hyper::StatusCode::NOT_FOUND);
            }
            _ => unreachable!(),
        }
        assert!(f.sessions.host.live());
    }
}

#[test]
fn authority_lost_during_stale_grant_refusal_prevents_replacement_open() {
    for unlink in [false, true] {
        let f = Fixture::new();
        let e = f.sign_in();
        assert!(f.access(&e).is_ok());
        {
            let mut s = f.state.lock().unwrap();
            s.worker_generation += 1;
            s.revoke_on_stale = !unlink;
            s.unlink_on_stale = unlink;
        }
        let error = f.access(&e).err().expect("authority lost");
        assert_eq!(
            error.inspected().0,
            if unlink {
                hyper::StatusCode::NOT_FOUND
            } else {
                hyper::StatusCode::UNAUTHORIZED
            }
        );
        assert_eq!(f.state.lock().unwrap().opened, 1);
        assert_eq!(f.state.lock().unwrap().granted, 2);
        assert!(f.sessions.host.channels.entries_for_test().is_empty());
        assert!(f.sessions.host.live());
    }
}

#[test]
fn abandoned_host_lifetime_expires_cached_resources_without_a_remote_sweep() {
    let f = Fixture::new();
    let e = f.sign_in();
    assert!(f.access(&e).is_ok());
    let before = f.state.lock().unwrap().granted;
    {
        let mut channels = f.sessions.host.channels.entries_for_test();
        let c = channels.values_mut().next().unwrap();
        assert!(c.expires - Instant::now() <= crate::sessions::IDLE);
        c.expires = Instant::now();
    }
    let next = f.sign_in();
    assert!(f.access(&next).is_ok());
    assert_eq!(f.sessions.host.channels.entries_for_test().len(), 1);
    assert_eq!(f.state.lock().unwrap().granted, before + 1);
    assert!(f.access(&e).is_ok()); // Eviction did not end or authorize the session.
    assert_eq!(f.sessions.host.channels.entries_for_test().len(), 2);
}
