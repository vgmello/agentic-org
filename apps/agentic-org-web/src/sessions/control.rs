//! One bounded actor owns the already-admitted private host connection and
//! transfer receiver. No host.sock/path fallback, office key or registry access.
use crate::selected::OfficeAccess;
use org_host_control::{ops, Password, SessionHandle};
use org_proto::{selected_tasks::OfficeScope, Call, Principal};
use org_socket::rights::{DatagramReceiver, ReceiveError, StreamReceiver};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    io::{self, Write},
    net::Shutdown,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::net::UnixStream,
    },
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const DEADLINE: Duration = Duration::from_secs(5);
const MAX_CONTROL_BYTES: usize = 64 * 1024;
static SPAWN_FENCE: Mutex<()> = Mutex::new(());
#[cfg(test)]
mod tests;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Denied,
    NotFound,
    Unavailable,
}
struct Reply {
    bytes: Zeroizing<Vec<u8>>,
    descriptor: Option<OwnedFd>,
}
struct Job {
    line: Zeroizing<Vec<u8>>,
    id: u64,
    office: Option<String>,
    until: Instant,
    answer: SyncSender<Result<Reply, Failure>>,
}
use channel_cache::{Cached, ChannelCache};

/// Local resource lifetime and capacity, never session authority. Keeping the
/// map private prevents transport callers from bypassing expiry or evicting a
/// concurrently installed replacement. Each operation releases its lock before
/// the caller can perform host or office RPCs.
mod channel_cache {
    use super::{Failure, SessionHandle};
    use org_client::selected::ApprovedOffice;
    use sha2::{Digest, Sha256};
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex, MutexGuard},
        time::Instant,
    };

    type Key = ([u8; 32], String);
    #[derive(Clone)]
    pub(super) struct Cached {
        pub channel: String,
        pub office: Arc<ApprovedOffice>,
        pub expires: Instant,
    }
    #[derive(Default)]
    pub(super) struct ChannelCache {
        entries: Mutex<BTreeMap<Key, Cached>>,
    }
    fn key(handle: &SessionHandle, office: &str) -> Key {
        (
            Sha256::digest(handle.expose().as_bytes()).into(),
            office.into(),
        )
    }
    fn available(entries: &BTreeMap<Key, Cached>, key: &Key) -> Result<(), Failure> {
        if !entries.contains_key(key) && entries.len() >= 256 {
            return Err(Failure::Unavailable);
        }
        Ok(())
    }
    impl ChannelCache {
        fn live_entries(&self) -> Result<MutexGuard<'_, BTreeMap<Key, Cached>>, Failure> {
            let mut entries = self.entries.lock().map_err(|_| Failure::Unavailable)?;
            // Abandoned/revoked handles may never make another request. Their
            // last host-observed lifetime still bounds these local resources.
            entries.retain(|_, cached| cached.expires > Instant::now());
            Ok(entries)
        }
        pub(super) fn get(
            &self,
            handle: &SessionHandle,
            office: &str,
        ) -> Result<Option<Cached>, Failure> {
            let key = key(handle, office);
            let entries = self.live_entries()?;
            available(&entries, &key)?;
            Ok(entries.get(&key).cloned())
        }
        pub(super) fn insert(
            &self,
            handle: &SessionHandle,
            office: &str,
            cached: Cached,
        ) -> Result<(), Failure> {
            let key = key(handle, office);
            let mut entries = self.live_entries()?;
            available(&entries, &key)?;
            entries.insert(key, cached);
            Ok(())
        }
        pub(super) fn refresh(
            &self,
            handle: &SessionHandle,
            office: &str,
            channel: &str,
            expires: Instant,
        ) -> Result<(), Failure> {
            self.entries
                .lock()
                .map_err(|_| Failure::Unavailable)?
                .entry(key(handle, office))
                .and_modify(|current| {
                    if current.channel == channel {
                        current.expires = expires;
                    }
                });
            Ok(())
        }
        pub(super) fn evict(
            &self,
            handle: &SessionHandle,
            office: &str,
            channel: &str,
        ) -> Result<(), Failure> {
            let key = key(handle, office);
            let mut entries = self.entries.lock().map_err(|_| Failure::Unavailable)?;
            if entries
                .get(&key)
                .is_some_and(|current| current.channel == channel)
            {
                entries.remove(&key);
            }
            Ok(())
        }
        pub(super) fn forget(&self, handle: &SessionHandle) {
            if let Ok(mut entries) = self.entries.lock() {
                let digest: [u8; 32] = Sha256::digest(handle.expose().as_bytes()).into();
                entries.retain(|(key, _), _| *key != digest);
            }
        }
        // Keep existing regression observations/injected clock boundaries
        // available in tests without exposing the map in production.
        #[cfg(test)]
        pub(super) fn entries_for_test(&self) -> MutexGuard<'_, BTreeMap<Key, Cached>> {
            self.entries.lock().unwrap()
        }
    }
}
pub struct HostControl {
    instance: String,
    live: Arc<AtomicBool>,
    socket: UnixStream,
    jobs: SyncSender<Job>,
    next: std::sync::atomic::AtomicU64,
    channels: ChannelCache,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response<T> {
    id: u64,
    result: Option<T>,
    error: Option<org_proto::ApiError>,
}
fn decode<T: DeserializeOwned>(bytes: &[u8], id: u64) -> Result<T, Failure> {
    if bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{') {
        return Err(Failure::Unavailable);
    }
    let reply: Response<T> = serde_json::from_slice(bytes).map_err(|_| Failure::Unavailable)?;
    if reply.id != id {
        return Err(Failure::Unavailable);
    }
    match (reply.result, reply.error) {
        (Some(result), None) => Ok(result),
        (None, Some(e)) if e.code == org_proto::ErrorCode::NotAuthorized => Err(Failure::Denied),
        (None, Some(e)) if e.code == org_proto::ErrorCode::NotFound => Err(Failure::NotFound),
        _ => Err(Failure::Unavailable),
    }
}

struct Counter(usize);
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|n| *n < MAX_CONTROL_BYTES)
            .ok_or_else(|| io::Error::other("control request too large"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn encode<T: Serialize>(value: &T) -> Result<Zeroizing<Vec<u8>>, Failure> {
    let mut n = Counter(0);
    serde_json::to_writer(&mut n, value).map_err(|_| Failure::Unavailable)?;
    let mut bytes = Zeroizing::new(Vec::with_capacity(n.0 + 1));
    serde_json::to_writer(&mut *bytes, value).map_err(|_| Failure::Unavailable)?;
    bytes.push(b'\n');
    Ok(bytes)
}
fn readable(fd: std::os::fd::BorrowedFd<'_>, until: Instant) -> Result<(), Failure> {
    let left = until
        .checked_duration_since(Instant::now())
        .ok_or(Failure::Unavailable)?;
    if org_socket::poll_readable(fd.as_raw_fd(), left).map_err(|_| Failure::Unavailable)? {
        Ok(())
    } else {
        Err(Failure::Unavailable)
    }
}
fn receive_error(error: ReceiveError, fatal: &dyn Fn()) -> Failure {
    if matches!(&error,ReceiveError::Fatal(e) if e.kind()!=io::ErrorKind::UnexpectedEof) {
        fatal();
    }
    Failure::Unavailable
}
impl HostControl {
    /// The verified gateway rendezvous supplies both descriptors. `fatal` must
    /// terminate the gateway on fatal ancillary framing (X-02 §5.2); it is not
    /// an authorization hook. No process is spawned by this crate.
    pub fn from_handoff(
        control: OwnedFd,
        transfer: OwnedFd,
        instance: String,
        fatal: Arc<dyn Fn() + Send + Sync>,
    ) -> io::Result<Arc<Self>> {
        let socket = org_socket::rights::connected_unix_stream(control)?;
        let transfer = org_socket::rights::unix_datagram(transfer)?;
        socket.set_nonblocking(false)?;
        socket.set_write_timeout(Some(DEADLINE))?;
        let writer = socket.try_clone()?;
        let receiver = StreamReceiver::new(socket.try_clone()?);
        let transfer = DatagramReceiver::new(transfer);
        let (jobs, pending) = mpsc::sync_channel::<Job>(32);
        let live = Arc::new(AtomicBool::new(true));
        let alive = Arc::clone(&live);
        thread::Builder::new()
            .name("gateway-host-control".into())
            .spawn(move || {
                actor(writer, receiver, transfer, pending, &alive, &*fatal);
                alive.store(false, Ordering::SeqCst);
            })?;
        let control = Arc::new(Self {
            instance,
            live,
            socket,
            jobs,
            next: std::sync::atomic::AtomicU64::new(1),
            channels: ChannelCache::default(),
        });
        let hello = control
            .call::<org_host_control::Hello>(&Default::default(), None)
            .map_err(|_| io::Error::other("host hello failed"))?
            .0;
        if hello.api_version != org_host_control::API_VERSION
            || hello.instance_id.as_deref() != Some(control.instance())
        {
            return Err(io::Error::other("host scope mismatch"));
        }
        Ok(control)
    }
    pub fn instance(&self) -> &str {
        &self.instance
    }
    /// Configuration for the listener received on this admitted connection only.
    pub fn fetch_listener(&self) -> Result<ops::TlsFetchResult, Failure> {
        self.call::<ops::TlsFetch>(&Default::default(), None)
            .map(|r| r.0)
    }
    pub fn live(&self) -> bool {
        self.live.load(Ordering::SeqCst)
    }
    fn close(&self) {
        self.live.store(false, Ordering::SeqCst);
        let _ = self.socket.shutdown(Shutdown::Both);
    }
    fn call<C: Call>(
        &self,
        params: &C::Params,
        office: Option<String>,
    ) -> Result<(C::Result, Option<OwnedFd>), Failure> {
        #[derive(Serialize)]
        struct Request<'a, T> {
            id: u64,
            method: &'static str,
            params: &'a T,
        }
        if !self.live() {
            return Err(Failure::Unavailable);
        }
        let id = self
            .next
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < (1 << 53) - 1).then_some(n + 1)
            })
            .map_err(|_| Failure::Unavailable)?;
        let line = encode(&Request {
            id,
            method: C::METHOD,
            params,
        })?;
        let until = Instant::now() + DEADLINE;
        let (answer, receiver) = mpsc::sync_channel(1);
        self.jobs
            .try_send(Job {
                line,
                id,
                office,
                until,
                answer,
            })
            .map_err(|_| Failure::Unavailable)?;
        let reply = match receiver.recv_timeout(DEADLINE) {
            Ok(reply) => reply?,
            Err(_) => {
                self.close();
                return Err(Failure::Unavailable);
            }
        };
        Ok((decode(&reply.bytes, id)?, reply.descriptor))
    }
    pub fn verify(
        &self,
        login: &str,
        password: &Password,
    ) -> Result<ops::AccountVerifyResult, Failure> {
        #[derive(Serialize)]
        struct Params<'a> {
            login: &'a str,
            password: &'a Password,
        }
        // Borrow the password for typed direct encoding, without a raw Value.
        struct Verify;
        impl Call for Verify {
            const METHOD: &'static str = "account.verify";
            type Params = ();
            type Result = ops::AccountVerifyResult;
        }
        self.borrowed::<Verify, _>(&Params { login, password })
    }
    fn borrowed<C: Call, T: Serialize>(&self, params: &T) -> Result<C::Result, Failure> {
        #[derive(Serialize)]
        struct Request<'a, T> {
            id: u64,
            method: &'static str,
            params: &'a T,
        }
        if !self.live() {
            return Err(Failure::Unavailable);
        }
        let id = self
            .next
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < (1 << 53) - 1).then_some(n + 1)
            })
            .map_err(|_| Failure::Unavailable)?;
        let (answer, receiver) = mpsc::sync_channel(1);
        let until = Instant::now() + DEADLINE;
        self.jobs
            .try_send(Job {
                line: encode(&Request {
                    id,
                    method: C::METHOD,
                    params,
                })?,
                id,
                office: None,
                until,
                answer,
            })
            .map_err(|_| Failure::Unavailable)?;
        match receiver.recv_timeout(DEADLINE) {
            Ok(reply) => decode(&reply?.bytes, id),
            Err(_) => {
                self.close();
                Err(Failure::Unavailable)
            }
        }
    }
    pub fn renew(&self, handle: &SessionHandle) -> Result<(), Failure> {
        #[derive(Serialize)]
        struct Params<'a> {
            session: &'a SessionHandle,
        }
        let result = self
            .borrowed::<ops::SessionRenew, _>(&Params { session: handle })
            .map(|_| ());
        if result == Err(Failure::Denied) {
            self.forget(handle);
        }
        result
    }
    pub fn inspect(&self, handle: &SessionHandle) -> Result<ops::SessionInspectResult, Failure> {
        #[derive(Serialize)]
        struct Params<'a> {
            session: &'a SessionHandle,
        }
        let result = self.borrowed::<ops::SessionInspect, _>(&Params { session: handle });
        if matches!(result, Err(Failure::Denied)) {
            self.forget(handle);
        }
        result
    }
    pub fn end(&self, handle: &SessionHandle) -> Result<(), Failure> {
        #[derive(Serialize)]
        struct Params<'a> {
            session: &'a SessionHandle,
        }
        let result = self.borrowed::<ops::SessionEnd, _>(&Params { session: handle });
        self.forget(handle);
        result.map(|_| ())
    }
    /// Local resource cleanup only. The host remains the authority for sessions,
    /// grants and revocation; dropping a browser entry never fabricates logout.
    pub(super) fn forget(&self, handle: &SessionHandle) {
        self.channels.forget(handle);
    }
    pub fn office(
        &self,
        handle: &SessionHandle,
        scope: &OfficeScope,
    ) -> Result<OfficeAccess, Failure> {
        if scope.instance_id != self.instance {
            return Err(Failure::Denied);
        }
        let mut expires = self.office_authority(handle, scope)?;
        if let Some(cached) = self.channels.get(handle, &scope.office_id)? {
            match self.office_grant(handle, scope, &cached.channel) {
                Ok(principal) => {
                    self.channels
                        .refresh(handle, &scope.office_id, &cached.channel, expires)?;
                    return Ok(OfficeAccess {
                        office: cached.office,
                        principal,
                    });
                }
                Err(error) => {
                    // Never retain the failed worker binding, or evict a newer
                    // channel installed concurrently by another request.
                    self.channels
                        .evict(handle, &scope.office_id, &cached.channel)?;
                    if !self.live() {
                        return Err(error);
                    }
                    // A failed grant is not authority. Recheck the SAME session
                    // and live link before the single replacement open/grant.
                    expires = self.office_authority(handle, scope)?;
                }
            }
        }
        // Acquisition happens before dispatch. No submitted task command is
        // replayed here, including one whose result may be unknown.
        let (channel, descriptor) = self.open(handle, &scope.office_id)?;
        let office = Arc::new(
            org_client::selected::ApprovedOffice::from_descriptor(
                descriptor,
                self.instance.clone(),
                scope.org_id.clone(),
                scope.office_id.clone(),
                channel.clone(),
            )
            .map_err(|_| Failure::Unavailable)?,
        );
        let principal = self.office_grant(handle, scope, &channel)?;
        self.channels.insert(
            handle,
            &scope.office_id,
            Cached {
                channel,
                office: Arc::clone(&office),
                expires,
            },
        )?;
        Ok(OfficeAccess { office, principal })
    }
    fn office_authority(
        &self,
        handle: &SessionHandle,
        scope: &OfficeScope,
    ) -> Result<Instant, Failure> {
        let status = self.inspect(handle)?;
        if !status
            .orgs
            .iter()
            .any(|link| link.org_id == scope.org_id && link.office_ids.contains(&scope.office_id))
        {
            return Err(Failure::NotFound);
        }
        let remaining = status
            .absolute_expires_at_ms
            .min(status.idle_expires_at_ms)
            .checked_sub(status.server_time_ms)
            .filter(|ms| *ms > 0)
            .ok_or(Failure::Denied)?;
        Instant::now()
            .checked_add(Duration::from_millis(remaining))
            .ok_or(Failure::Unavailable)
    }
    fn office_grant(
        &self,
        handle: &SessionHandle,
        scope: &OfficeScope,
        channel: &str,
    ) -> Result<Principal, Failure> {
        #[derive(Serialize)]
        struct Grant<'a> {
            session: &'a SessionHandle,
            office_id: &'a str,
            channel_id: &'a str,
        }
        let grant = self.borrowed::<ops::SessionGrant, _>(&Grant {
            session: handle,
            office_id: &scope.office_id,
            channel_id: channel,
        })?;
        if grant.grant.claims.org_id != scope.org_id
            || grant.grant.claims.office_id != scope.office_id
            || grant.grant.claims.channel_id != channel
        {
            self.close();
            return Err(Failure::Unavailable);
        }
        Ok(Principal { grant: grant.grant })
    }
    fn open(&self, handle: &SessionHandle, office: &str) -> Result<(String, OwnedFd), Failure> {
        #[derive(Serialize)]
        struct Params<'a> {
            session: &'a SessionHandle,
            office_id: &'a str,
        }
        #[derive(Serialize)]
        struct Request<T> {
            id: u64,
            method: &'static str,
            params: T,
        }
        let id = self
            .next
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < (1 << 53) - 1).then_some(n + 1)
            })
            .map_err(|_| Failure::Unavailable)?;
        let (answer, receiver) = mpsc::sync_channel(1);
        self.jobs
            .try_send(Job {
                line: encode(&Request {
                    id,
                    method: ops::OfficeOpenChannel::METHOD,
                    params: Params {
                        session: handle,
                        office_id: office,
                    },
                })?,
                id,
                office: Some(office.into()),
                until: Instant::now() + DEADLINE,
                answer,
            })
            .map_err(|_| Failure::Unavailable)?;
        let reply = match receiver.recv_timeout(DEADLINE) {
            Ok(reply) => reply?,
            Err(_) => {
                self.close();
                return Err(Failure::Unavailable);
            }
        };
        let result: ops::OfficeOpenChannelResult = decode(&reply.bytes, id)?;
        let fd = reply.descriptor.ok_or(Failure::Unavailable)?;
        let stream =
            org_socket::rights::connected_unix_stream(fd).map_err(|_| Failure::Unavailable)?;
        self.call::<ops::FdsReceived>(
            &ops::FdsReceivedParams {
                of: result.channel_id.clone(),
            },
            None,
        )?;
        Ok((result.channel_id, stream.into()))
    }
}
impl Drop for HostControl {
    fn drop(&mut self) {
        self.close();
    }
}
fn actor(
    mut writer: UnixStream,
    mut receiver: StreamReceiver,
    mut transfer: DatagramReceiver,
    pending: mpsc::Receiver<Job>,
    live: &AtomicBool,
    fatal: &dyn Fn(),
) {
    while live.load(Ordering::SeqCst) {
        let job = match pending.recv_timeout(Duration::from_millis(100)) {
            Ok(job) => job,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if receiver.fd().is_none_or(|fd| {
                    org_socket::poll_readable(fd.as_raw_fd(), Duration::ZERO).unwrap_or(true)
                }) {
                    break;
                }
                continue;
            }
        };
        if Instant::now() >= job.until {
            let _ = job.answer.send(Err(Failure::Unavailable));
            continue;
        }
        let result = (|| {
            writer
                .write_all(&job.line)
                .map_err(|_| Failure::Unavailable)?;
            let mut bytes = Zeroizing::new(Vec::with_capacity(MAX_CONTROL_BYTES));
            loop {
                readable(receiver.fd().ok_or(Failure::Unavailable)?, job.until)?;
                let (b, _) = receiver
                    .receive_byte(0, &SPAWN_FENCE)
                    .map_err(|e| receive_error(e, fatal))?;
                if b == b'\n' {
                    break;
                }
                if bytes.len() == MAX_CONTROL_BYTES {
                    return Err(Failure::Unavailable);
                }
                bytes.push(b);
            }
            let mut descriptor = None;
            if let Some(office) = job.office {
                // Error responses carry no descriptor. A successful response is
                // followed by exactly its matching transfer datagram.
                if let Ok(result) = decode::<ops::OfficeOpenChannelResult>(&bytes, job.id) {
                    readable(transfer.fd().ok_or(Failure::Unavailable)?, job.until)?;
                    let mut header = vec![0; org_office_launch::v3::TRANSFER_HEADER_MAX_BYTES];
                    let received = transfer
                        .receive(&mut header, 1, &SPAWN_FENCE)
                        .map_err(|e| receive_error(e, fatal))?;
                    let record =
                        org_office_launch::v3::decode_transfer_record(&header[..received.bytes])
                            .map_err(|_| Failure::Unavailable)?;
                    if !matches!(record,org_office_launch::v3::TransferRecord::Channel {kind:org_office_launch::v3::ChannelKind::Gateway,office_id,channel_id} if office_id==office&&channel_id==result.channel_id)
                    {
                        return Err(Failure::Unavailable);
                    }
                    descriptor = received.descriptors.into_iter().next();
                }
            }
            Ok(Reply { bytes, descriptor })
        })();
        let failed = result.is_err();
        let _ = job.answer.send(result);
        if failed {
            break;
        }
    }
    live.store(false, Ordering::SeqCst);
    let _ = writer.shutdown(Shutdown::Both);
}

#[cfg(test)]
mod error_tests {
    use super::*;
    #[test]
    fn native_scope_refusal_is_distinct_from_session_end_and_transport_failure() {
        for (code, expected) in [
            ("not_found", Failure::NotFound),
            ("not_authorized", Failure::Denied),
            ("unavailable", Failure::Unavailable),
            ("storage_error", Failure::Unavailable),
        ] {
            let bytes = serde_json::to_vec(
                &serde_json::json!({"id":1,"error":{"code":code,"message":"sanitized"}}),
            )
            .unwrap();
            assert_eq!(
                decode::<ops::OfficeOpenChannelResult>(&bytes, 1).err(),
                Some(expected)
            );
        }
        assert_eq!(
            decode::<ops::OfficeOpenChannelResult>(
                br#"{"id":2,"error":{"code":"not_found","message":"sanitized"}}"#,
                1
            )
            .err(),
            Some(Failure::Unavailable)
        );
    }
}
