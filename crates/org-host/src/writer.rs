//! Host-owned selected writer continuity. Custody is outside the instance root;
//! this ledger does not allocate work events or make domain decisions.
use crate::{
    host_secrets::{HostSecrets, Name, Value},
    hostdb::HostDb,
    sync::lock,
};
use org_office_launch::writer::{self as wire, Admission, Request, Response};
use org_proto::selected_tasks::JournalPosition;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(target_os = "macos")]
use std::os::fd::AsRawFd;
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
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema_version: u32,
    operation_id: String,
    root: Root,
    admission: Admission,
    position: Option<JournalPosition>,
}
pub(crate) struct Writers {
    custody: Arc<dyn HostSecrets>,
    db: Arc<HostDb>,
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
fn name(office: &str) -> io::Result<Name> {
    let hash: String = Sha256::digest(office.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Name::parse(&format!("writer.{hash}")).map_err(private)
}
impl Writers {
    pub(crate) fn open(
        custody: Arc<dyn HostSecrets>,
        db: Arc<HostDb>,
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
            .map_err(private)?
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
        if record.schema_version != 2
            || org_proto::ulid_time_ms(&record.operation_id).is_none()
            || a.version != wire::VERSION
            || record.root != self.root
            || a.identity.instance_id != self.instance
            || a.identity.office_id != office
            || a.identity.writer_epoch <= 0
            || a.identity.journal_incarnation.is_empty()
            || record.position.as_ref().is_some_and(|p| !p.valid())
        {
            return Err(refused());
        }
        let device = self
            .custody
            .load(&Name::parse("writer-device").map_err(private)?)
            .map_err(private)?
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
            .map_err(private)
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
            .join("selected-writer.required"))
    }
    fn install_marker(&self, office: &str) -> io::Result<()> {
        let path = self.marker(office)?;
        let opened = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path);
        let mut file = match opened {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let file = fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                    .open(&path)?;
                let meta = file.metadata()?;
                if !meta.is_file()
                    || meta.uid() != crate::home::euid()
                    || meta.mode() & 0o7777 != 0o600
                    || meta.len() != b"selected-writer/1\n".len() as u64
                {
                    return Err(refused());
                }
                let mut bytes = Vec::new();
                std::io::Read::take(&file, 32).read_to_end(&mut bytes)?;
                if bytes != b"selected-writer/1\n" {
                    return Err(refused());
                }
                file
            }
            Err(error) => return Err(error),
        };
        if file.metadata()?.len() == 0 {
            file.write_all(b"selected-writer/1\n")?;
        }
        file.sync_all()?;
        #[cfg(target_os = "macos")]
        // SAFETY: valid writable fd; F_FULLFSYNC takes no pointer argument.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } != 0 {
            return Err(io::Error::last_os_error());
        }
        fs::File::open(path.parent().ok_or_else(refused)?)?.sync_all()?;
        #[cfg(target_os = "macos")]
        // SAFETY: live file on the same filesystem; flush parent publication too.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    /// Called only by protected provisioning/adoption after owner binding. Missing
    /// custody on a normal start never takes this path. Existing records cannot
    /// be replaced, reset or rotated. An exact operation replay may finish the
    /// marker publication after a crash; it retains every authority/checkpoint.
    pub(crate) fn reserve(
        &self,
        operation: &str,
        mut admission: Admission,
    ) -> io::Result<Admission> {
        let _gate = lock(&self.gate);
        self.root.verify()?;
        if admission.version != wire::VERSION
            || admission.identity.instance_id != self.instance
            || org_proto::ulid_time_ms(operation).is_none()
        {
            return Err(refused());
        }
        if let Some(record) = self.load(&admission.identity.office_id)? {
            self.verify(&record, &admission.identity.office_id)?;
            let prior = &record.admission;
            if record.operation_id != operation
                || prior.identity.organization_id != admission.identity.organization_id
                || prior.protected_root != admission.protected_root
                || prior.public_root != admission.public_root
                || prior.owner_account != admission.owner_account
                || prior.owner_member != admission.owner_member
            {
                return Err(refused());
            }
            self.install_marker(&admission.identity.office_id)?;
            return Ok(record.admission);
        }
        self.db
            .verify_selected_owner(
                &admission.identity.organization_id,
                &admission.owner_account,
                &admission.owner_member,
            )
            .map_err(private)?;
        let device_name = Name::parse("writer-device").map_err(private)?;
        if self.custody.load(&device_name).map_err(private)?.is_none() {
            let random = crate::secrets::generate_db_key()?;
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
        admission.identity.machine_id =
            String::from_utf8(device.expose().to_vec()).map_err(private)?;
        admission.identity.writer_epoch = 1;
        admission.identity.journal_incarnation = crate::ids::generate()?;
        self.validate_reserved_roots(&admission)?;
        let record = Record {
            schema_version: 2,
            operation_id: operation.into(),
            root: self.root.clone(),
            admission: admission.clone(),
            position: None,
        };
        self.verify(&record, &admission.identity.office_id)?;
        self.save(&admission.identity.office_id, &record)?;
        self.install_marker(&admission.identity.office_id)?;
        Ok(admission)
    }
    fn validate_reserved_roots(&self, admission: &Admission) -> io::Result<()> {
        let protected = Path::new(&admission.protected_root);
        let public = Path::new(&admission.public_root);
        if !protected.is_absolute()
            || !public.is_absolute()
            || protected.starts_with(public)
            || public.starts_with(protected)
            || !protected.starts_with(&self.root.path)
            || !public.starts_with(&self.root.path)
            || [protected, public].iter().any(|p| {
                p.components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            })
        {
            return Err(refused());
        }
        Ok(())
    }
    /// Retain one live host-side control connection per office. A protected
    /// binding must already exist; legacy offices are deliberately not promoted.
    pub(crate) fn serve(
        self: &Arc<Self>,
        office: &str,
        stream: UnixStream,
        live: Arc<AtomicBool>,
    ) -> io::Result<()> {
        let record = self.load(office)?.ok_or_else(refused)?;
        self.verify(&record, office)?;
        if !fs::symlink_metadata(self.marker(office)?).is_ok_and(|m| {
            m.is_file() && m.uid() == crate::home::euid() && m.mode() & 0o7777 == 0o600
        }) {
            return Err(refused());
        }
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
    pub(crate) fn required(&self, office: &str) -> io::Result<bool> {
        if self.load(office)?.is_some() {
            return Ok(true);
        }
        match fs::symlink_metadata(self.marker(office)?) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            _ => Err(refused()),
        }
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
                position: self.record.position.clone(),
            }),
            Request::Owner { .. } => Ok(Response::Owner {
                account: self.record.admission.owner_account.clone(),
                member: self.record.admission.owner_member.clone(),
            }),
            Request::Confirm { before, after, .. } => {
                if before != self.record.position || !after.valid() {
                    return Err(refused());
                }
                let valid = match &before {
                    None => after.ordinal == 0,
                    Some(p) => &after == p || after.follows(p),
                };
                if !valid {
                    return Err(refused());
                }
                if before.as_ref() != Some(&after) {
                    self.record.position = Some(after);
                    self.writers.save(&self.office, &self.record)?;
                }
                Ok(Response::Confirmed)
            }
        }
    }
}

#[cfg(test)]
mod tests;
