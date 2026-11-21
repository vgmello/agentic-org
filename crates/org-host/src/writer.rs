//! Host-owned writer custody for each office's journal. Custody is outside the instance
//! root; this ledger does not allocate positions or make domain decisions.
//!
//! One record per office (schema 3, wire version 2): the writer identity minted at the
//! protected reservation, the organization's data roots, the owner binding and the last
//! journal head the office confirmed. Every office needs one to start: the host never
//! creates a missing record for an existing office, and an older record is refused, not
//! migrated (nothing has shipped).
use crate::{
    host_secrets::{HostSecrets, Name, Value},
    hostdb::HostStore,
    sync::lock,
};
use org_office_launch::writer::{
    self as wire, marker, Admission, Head, Identity, Request, Response,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, BufReader, Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Root {
    path: PathBuf,
    device: u64,
    inode: u64,
}
impl Root {
    fn read(path: &Path) -> io::Result<Self> {
        let meta = fs::symlink_metadata(path)?;
        if !path.is_absolute()
            || !meta.is_dir()
            || meta.uid() != crate::home::euid()
            || meta.mode() & 0o7777 != 0o700
        {
            return Err(refused());
        }
        Ok(Self {
            path: path.into(),
            device: meta.dev(),
            inode: meta.ino(),
        })
    }
    fn verify(&self) -> io::Result<()> {
        if &Self::read(&self.path)? != self {
            return Err(refused());
        }
        Ok(())
    }
}
/// The only record schema. Schema 2 (one origin high-water, public and protected roots,
/// a ULID incarnation) is refused.
const SCHEMA_VERSION: u32 = 3;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema_version: u32,
    operation_id: String,
    root: Root,
    admission: Admission,
    head: Option<Head>,
}

/// What protected provisioning binds an office's writer to. The host mints the rest: the
/// journal incarnation, the epoch, the device identity and the data roots.
pub(crate) struct Binding<'a> {
    pub(crate) organization_id: &'a str,
    pub(crate) office_id: &'a str,
    pub(crate) owner_account: &'a str,
    pub(crate) owner_member: &'a str,
}
pub(crate) struct Writers {
    custody: Arc<dyn HostSecrets>,
    db: Arc<dyn HostStore>,
    root: Root,
    instance: String,
    active: Mutex<BTreeSet<String>>,
    gate: Mutex<()>,
}
impl std::fmt::Debug for Writers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Writers").finish_non_exhaustive()
    }
}
struct Lease {
    writers: Arc<Writers>,
    office: String,
    record: Record,
    live: Arc<AtomicBool>,
}
impl Drop for Lease {
    fn drop(&mut self) {
        lock(&self.writers.active).remove(&self.office);
    }
}
fn refused() -> io::Error {
    io::Error::other("selected writer authority requires recovery")
}
fn private<T>(_: T) -> io::Error {
    refused()
}
/// The custody store or the host registry could not be read: a condition that may pass,
/// unlike a missing or invalid record.
#[derive(Debug)]
struct Unreadable;
impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("writer custody is unreadable")
    }
}
impl std::error::Error for Unreadable {}
fn unreadable() -> io::Error {
    io::Error::other(Unreadable)
}
/// An I/O failure that passes on its own. Every other kind (a symlinked or unreadable
/// item, a missing store, no adapter) is a lasting state: a refusal. Running out of
/// descriptors (EMFILE, ENFILE) is refused too: the error keeps only its kind, which is
/// `Uncategorized`, so it can't be told from other uncategorized failures.
fn transient(kind: io::ErrorKind) -> bool {
    use io::ErrorKind as K;
    matches!(
        kind,
        K::Interrupted | K::WouldBlock | K::TimedOut | K::ResourceBusy | K::OutOfMemory
    )
}
/// A custody store error: a transient I/O failure, a locked credential store
/// (`InteractionNotAllowed`) or an item in use is [`Unreadable`]; anything else (corrupt,
/// unsafe, linked, missing, unsupported, any other OS status) is a refusal.
fn store(error: crate::host_secrets::Error) -> io::Error {
    use crate::host_secrets::Error as E;
    match error {
        E::Io(kind) if transient(kind) => unreadable(),
        E::InteractionNotAllowed | E::InUse => unreadable(),
        _ => refused(),
    }
}
/// A host registry error: a transient storage fault (busy/locked, transient I/O, out of
/// memory) or a transient filesystem I/O failure is [`Unreadable`]; anything else (a
/// corrupt or tampered registry, another schema, a binding it does not hold) is a refusal.
fn registry(error: crate::hostdb::Error) -> io::Error {
    use crate::hostdb::Error as E;
    match error {
        E::Io(kind) if transient(kind) => unreadable(),
        E::Store(fault) if fault.is_transient() => unreadable(),
        _ => refused(),
    }
}

/// Why an office may not start: [`Self::Refused`] (no record, an invalid or older one, or
/// no exact marker) needs protected provisioning or recovery, never a restart;
/// [`Self::Unreadable`] (the custody store or the registry could not be read) may pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmitError {
    Refused,
    Unreadable,
}
impl From<io::Error> for AdmitError {
    fn from(error: io::Error) -> Self {
        if error.get_ref().is_some_and(|e| e.is::<Unreadable>()) {
            Self::Unreadable
        } else {
            Self::Refused
        }
    }
}
/// What an admitted office's writer is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Bound {
    pub(crate) organization_id: String,
    pub(crate) owner_member: String,
}
fn name(office: &str) -> io::Result<Name> {
    let hash: String = Sha256::digest(office.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Name::parse(&format!("writer.{hash}")).map_err(private)
}

const MARKER_BYTES: &[u8] = b"selected-writer/1\n";

fn flush_marker(file: &fs::File) -> io::Result<()> {
    org_platform::fs::full_sync(file)
}

fn existing_marker(path: &Path) -> io::Result<Option<fs::File>> {
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != crate::home::euid()
        || meta.mode() & 0o7777 != 0o600
        || meta.nlink() != 1
        || meta.len() != MARKER_BYTES.len() as u64
    {
        return Err(refused());
    }
    let mut bytes = Vec::new();
    std::io::Read::take(&file, 32).read_to_end(&mut bytes)?;
    if bytes != MARKER_BYTES {
        return Err(refused());
    }
    Ok(Some(file))
}

fn publish_marker(parent: &fs::File, temporary: &str) -> io::Result<()> {
    let from = std::ffi::CString::new(temporary).map_err(private)?;
    let to = std::ffi::CString::new(marker::MARKER).map_err(private)?;
    // Fixed/generated single-component names under the retained parent;
    // exclusive rename never overwrites an existing final entry.
    org_platform::fs::rename_noreplace_at(parent, &from, parent, &to)
}

fn empty_reservation_root(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Ok(meta) if meta.is_dir() => {}
        _ => return Err(refused()),
    }
    if fs::read_dir(path)?.next().is_some() {
        return Err(refused());
    }
    Ok(())
}

impl Writers {
    pub(crate) fn open(
        custody: Arc<dyn HostSecrets>,
        db: Arc<dyn HostStore>,
        instance: &str,
        root: &Path,
    ) -> io::Result<Arc<Self>> {
        db.verify_instance(instance).map_err(private)?;
        Ok(Arc::new(Self {
            custody,
            db,
            root: Root::read(root)?,
            instance: instance.into(),
            active: Mutex::default(),
            gate: Mutex::new(()),
        }))
    }
    fn load(&self, office: &str) -> io::Result<Option<Record>> {
        self.custody
            .load(&name(office)?)
            .map_err(store)?
            .map(|v| serde_json::from_slice(v.expose()).map_err(private))
            .transpose()
    }
    fn save(&self, office: &str, record: &Record) -> io::Result<()> {
        let bytes = serde_json::to_vec(record).map_err(private)?;
        self.custody
            .put(&name(office)?, &Value::new(bytes.clone()).map_err(private)?)
            .map_err(private)?;
        let loaded = self
            .custody
            .load(&name(office)?)
            .map_err(private)?
            .ok_or_else(refused)?;
        if loaded.expose() != bytes {
            return Err(refused());
        }
        Ok(())
    }
    fn verify(&self, record: &Record, office: &str) -> io::Result<()> {
        self.root.verify()?;
        let a = &record.admission;
        let (durable, runtime) = self.data_roots(&a.identity.organization_id)?;
        if record.schema_version != SCHEMA_VERSION
            || org_proto::ulid_time_ms(&record.operation_id).is_none()
            || !a.check()
            || record.root != self.root
            || a.identity.instance_id != self.instance
            || a.identity.office_id != office
            || Path::new(&a.durable_root) != durable
            || Path::new(&a.runtime_root) != runtime
            || record.head.as_ref().is_some_and(|h| !h.check())
        {
            return Err(refused());
        }
        let device = self
            .custody
            .load(&Name::parse("writer-device").map_err(private)?)
            .map_err(store)?
            .ok_or_else(refused)?;
        if device.expose() != a.identity.machine_id.as_bytes() {
            return Err(refused());
        }
        self.db
            .verify_selected_owner(
                &a.identity.organization_id,
                &a.owner_account,
                &a.owner_member,
            )
            .map_err(registry)
    }
    /// The organization's data roots: `<instance-root>/<organization-id>/durable` and
    /// `.../runtime`. Only a canonical UUIDv7 organization has them.
    pub(crate) fn data_roots(&self, organization: &str) -> io::Result<(PathBuf, PathBuf)> {
        if !org_proto::is_uuid_v7(organization) {
            return Err(refused());
        }
        let base = self.root.path.join(organization);
        Ok((base.join("durable"), base.join("runtime")))
    }
    fn marker(&self, office: &str) -> io::Result<PathBuf> {
        if !org_office_launch::is_storage_id(office) {
            return Err(refused());
        }
        Ok(self
            .root
            .path
            .join("office-state/offices")
            .join(office)
            .join(marker::MARKER))
    }
    fn install_marker(&self, office: &str) -> io::Result<()> {
        let path = self.marker(office)?;
        let parent_path = path.parent().ok_or_else(refused)?;
        let parent = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(parent_path)?;
        if existing_marker(&path)?.is_none() {
            self.stage_marker(&path, &parent)?;
        }
        let file = existing_marker(&path)?.ok_or_else(refused)?;
        flush_marker(&file)?;
        parent.sync_all()?;
        flush_marker(&file)
    }
    fn stage_marker(&self, path: &Path, parent: &fs::File) -> io::Result<()> {
        let temporary = marker::staging_name(&crate::ids::generate()?);
        let temporary_path = path.parent().ok_or_else(refused)?.join(&temporary);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&temporary_path)?;
        let result = (|| {
            file.write_all(MARKER_BYTES)?;
            flush_marker(&file)?;
            self.root.verify()?;
            let held = file.metadata()?;
            let staged = fs::symlink_metadata(&temporary_path)?;
            if !staged.is_file() || staged.dev() != held.dev() || staged.ino() != held.ino() {
                return Err(refused());
            }
            match publish_marker(parent, &temporary) {
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    existing_marker(path)?.ok_or_else(refused)?;
                    Ok(())
                }
                result => result,
            }
        })();
        // Only this attempt's private staging name is removed. SIGKILL leftovers
        // never become authority and do not block replay with intact custody.
        match fs::remove_file(&temporary_path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound && result.is_ok() => {
                return Err(error)
            }
            _ => {}
        }
        result
    }
    /// Called only by protected provisioning after owner binding. Missing custody on a
    /// normal start never takes this path. Existing records cannot be replaced, reset or
    /// rotated. An exact operation replay may finish the marker publication after a crash;
    /// it retains every authority and the confirmed head.
    ///
    /// At the first reservation the office folder and both data roots must be absent or
    /// empty: retained data without its record needs explicit recovery, never a new writer.
    pub(crate) fn reserve(&self, operation: &str, binding: &Binding<'_>) -> io::Result<Admission> {
        let _gate = lock(&self.gate);
        self.root.verify()?;
        let (durable, runtime) = self.data_roots(binding.organization_id)?;
        if org_proto::ulid_time_ms(operation).is_none() || !org_proto::is_uuid_v7(binding.office_id)
        {
            return Err(refused());
        }
        if let Some(record) = self.load(binding.office_id)? {
            self.verify(&record, binding.office_id)?;
            let prior = &record.admission;
            if record.operation_id != operation
                || prior.identity.organization_id != binding.organization_id
                || prior.owner_account != binding.owner_account
                || prior.owner_member != binding.owner_member
            {
                return Err(refused());
            }
            self.install_marker(binding.office_id)?;
            return Ok(record.admission);
        }
        let marker = self.marker(binding.office_id)?;
        for path in [marker.parent().ok_or_else(refused)?, &durable, &runtime] {
            empty_reservation_root(path)?;
        }
        self.db
            .verify_selected_owner(
                binding.organization_id,
                binding.owner_account,
                binding.owner_member,
            )
            .map_err(private)?;
        let device_name = Name::parse("writer-device").map_err(private)?;
        if self.custody.load(&device_name).map_err(private)?.is_none() {
            if self
                .custody
                .list()
                .map_err(private)?
                .iter()
                .any(|name| name.as_str().starts_with("writer."))
            {
                return Err(refused());
            }
            let random = crate::secrets::generate_office_secrets_key()?;
            let value: String = random
                .expose_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            self.custody
                .put(
                    &device_name,
                    &Value::new(value.into_bytes()).map_err(private)?,
                )
                .map_err(private)?;
        }
        let device = self
            .custody
            .load(&device_name)
            .map_err(private)?
            .ok_or_else(refused)?;
        let admission = Admission {
            version: wire::VERSION,
            identity: Identity {
                instance_id: self.instance.clone(),
                organization_id: binding.organization_id.into(),
                office_id: binding.office_id.into(),
                // A store ID: canonical lowercase UUIDv7, never a ULID (P1).
                journal_incarnation: crate::ids::uuid_v7()?,
                writer_epoch: 1,
                machine_id: String::from_utf8(device.expose().to_vec()).map_err(private)?,
            },
            durable_root: durable.to_str().ok_or_else(refused)?.into(),
            runtime_root: runtime.to_str().ok_or_else(refused)?.into(),
            owner_account: binding.owner_account.into(),
            owner_member: binding.owner_member.into(),
        };
        let record = Record {
            schema_version: SCHEMA_VERSION,
            operation_id: operation.into(),
            root: self.root.clone(),
            admission: admission.clone(),
            head: None,
        };
        self.verify(&record, binding.office_id)?;
        self.save(binding.office_id, &record)?;
        self.install_marker(binding.office_id)?;
        Ok(admission)
    }
    /// Whether `office` may start or be created: it has a valid record of this schema and
    /// its reservation marker. Every office needs both; the host never creates them here.
    /// Returns what the office's writer is bound to.
    pub(crate) fn admit(&self, office: &str) -> Result<Bound, AdmitError> {
        let admission = self.admitted(office)?.admission;
        Ok(Bound {
            organization_id: admission.identity.organization_id,
            owner_member: admission.owner_member,
        })
    }
    fn admitted(&self, office: &str) -> io::Result<Record> {
        let record = self.load(office)?.ok_or_else(refused)?;
        self.verify(&record, office)?;
        // The exact marker: a regular owner-only file with one link and the exact bytes.
        existing_marker(&self.marker(office)?)?.ok_or_else(refused)?;
        Ok(record)
    }
    /// Retain one live host-side control connection per office. A protected binding must
    /// already exist ([`Self::admit`]); an office without one is never promoted.
    pub(crate) fn serve(
        self: &Arc<Self>,
        office: &str,
        stream: UnixStream,
        live: Arc<AtomicBool>,
    ) -> io::Result<()> {
        let record = self.admitted(office)?;
        if !lock(&self.active).insert(office.into()) {
            return Err(refused());
        }
        let lease = Lease {
            writers: Arc::clone(self),
            office: office.into(),
            record,
            live,
        };
        std::thread::Builder::new()
            .name("org-writer-authority".into())
            .spawn(move || {
                let _ = lease.run(stream);
            })?;
        Ok(())
    }
}
impl Lease {
    fn run(mut self, mut stream: UnixStream) -> io::Result<()> {
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        wire::write(&mut stream, &self.record.admission)?;
        let mut reader = BufReader::new(stream);
        while let Some(request) = wire::read(&mut reader)? {
            let response = self.answer(request).unwrap_or(Response::Denied);
            let denied = matches!(response, Response::Denied);
            wire::write(reader.get_mut(), &response)?;
            if denied {
                return Err(refused());
            }
        }
        Ok(())
    }
    fn answer(&mut self, request: Request) -> io::Result<Response> {
        if !self.live.load(Ordering::SeqCst) {
            return Err(refused());
        }
        let _gate = lock(&self.writers.gate);
        self.writers.verify(&self.record, &self.office)?;
        let stored = self.writers.load(&self.office)?.ok_or_else(refused)?;
        if serde_json::to_vec(&stored).map_err(private)?
            != serde_json::to_vec(&self.record).map_err(private)?
        {
            return Err(refused());
        }
        let identity = match &request {
            Request::Checkpoint { identity }
            | Request::Confirm { identity, .. }
            | Request::Owner { identity } => identity,
        };
        if identity != &self.record.admission.identity {
            return Err(refused());
        }
        match request {
            Request::Checkpoint { .. } => Ok(Response::Checkpoint {
                head: self.record.head.clone(),
            }),
            Request::Owner { .. } => Ok(Response::Owner {
                account: self.record.admission.owner_account.clone(),
                member: self.record.admission.owner_member.clone(),
            }),
            Request::Confirm { before, after, .. } => {
                if !wire::confirms(self.record.head.as_ref(), before.as_ref(), &after) {
                    return Err(refused());
                }
                if self.record.head.as_ref() != Some(&after) {
                    self.record.head = Some(after);
                    self.writers.save(&self.office, &self.record)?;
                }
                Ok(Response::Confirmed)
            }
        }
    }
}

#[cfg(test)]
mod tests;

/// Real writer custody for tests that start offices: a development custody directory and a
/// host registry under `dir`, one organization owned by a seeded administrator, and a
/// reservation for each of `offices` (canonical UUIDv7 IDs).
#[cfg(test)]
pub(crate) struct TestCustody {
    pub(crate) writers: Arc<Writers>,
    pub(crate) organization: String,
    pub(crate) member: String,
    /// While set, every read of the custody store fails with a transient I/O error.
    pub(crate) unreadable: Arc<AtomicBool>,
}

/// A custody store whose reads time out while `fail` is set.
#[cfg(test)]
struct Faulty {
    inner: Arc<dyn HostSecrets>,
    fail: Arc<AtomicBool>,
}
#[cfg(test)]
impl HostSecrets for Faulty {
    fn put(&self, name: &Name, value: &Value) -> Result<(), crate::host_secrets::Error> {
        self.inner.put(name, value)
    }
    fn load(&self, name: &Name) -> Result<Option<Value>, crate::host_secrets::Error> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(crate::host_secrets::Error::Io(io::ErrorKind::TimedOut));
        }
        self.inner.load(name)
    }
    fn delete(&self, name: &Name) -> Result<(), crate::host_secrets::Error> {
        self.inner.delete(name)
    }
    fn list(&self) -> Result<Vec<Name>, crate::host_secrets::Error> {
        self.inner.list()
    }
}

#[cfg(test)]
impl TestCustody {
    pub(crate) fn reserve(dir: &Path, instance: &str, offices: &[&str]) -> Self {
        use std::os::unix::fs::DirBuilderExt;
        let private = |path: &Path| {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(path)
                .unwrap();
        };
        // Its own folder each time: one test may build several supervisors in one `dir`.
        let base = dir.join(format!("custody-{}", crate::ids::generate().unwrap()));
        let root = base.join("instance");
        let home = root.join("office-state");
        let keys = base.join("keys");
        private(&home);
        private(&keys);
        let unreadable = Arc::new(AtomicBool::new(false));
        let custody: Arc<dyn HostSecrets> = Arc::new(Faulty {
            inner: Arc::new(
                crate::host_secrets::dev::DevSecrets::open(&keys, &home, instance).unwrap(),
            ),
            fail: Arc::clone(&unreadable),
        });
        let init = crate::hostdb::HostDbInit {
            instance_id: instance.into(),
            clock_high_ms: 1,
        };
        let db = Arc::new(crate::hostdb::HostDb::create(&home, &*custody, &init).unwrap());
        let account = crate::ids::generate().unwrap();
        let password =
            org_host_control::Password::new("synthetic custody password".into()).unwrap();
        db.seed_account(
            &account,
            "Owner",
            &crate::service::authentication::hash_password(&password).unwrap(),
            true,
            1,
        )
        .unwrap();
        let admin = db
            .account_authority(&crate::accounts::identity::AccountId::parse(&account).unwrap())
            .unwrap()
            .unwrap();
        let organization = crate::ids::uuid_v7().unwrap();
        let member = crate::ids::uuid_v7().unwrap();
        db.establish_organization_owner(&admin, &organization, &member)
            .unwrap();
        let writers = Writers::open(custody, db, instance, &root.canonicalize().unwrap()).unwrap();
        for office in offices {
            fs::create_dir_all(home.join("offices").join(office)).unwrap();
            writers
                .reserve(
                    &crate::ids::generate().unwrap(),
                    &Binding {
                        organization_id: &organization,
                        office_id: office,
                        owner_account: &account,
                        owner_member: &member,
                    },
                )
                .unwrap();
        }
        Self {
            writers,
            organization,
            member,
            unreadable,
        }
    }
}
