//! Local human adoption client. Explicit instance, protected sign-in and one
//! host-approved descriptor; no office database or custody access by this client.
use org_host_control::{ops, Password, SessionHandle};
use org_proto::{
    selected_tasks::{adoption, staging},
    Call, Hello, HelloParams, HelloResult, Principal, Response,
};
use org_socket::rights::{ReceiveError, StreamReceiver};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::OwnedFd,
        unix::{
            fs::{MetadataExt, OpenOptionsExt},
            net::UnixStream,
        },
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
const CHUNK: usize = org_proto::selected_tasks::MAX_CHUNK_BYTES;
const BLOB: u64 = 64 * 1024 * 1024;
fn unavailable() -> String {
    "adoption is unresolved; retain the original stage/operation IDs and retry or look up that operation".into()
}

/// Exclusive byte/descriptor reader. No buffered ordinary reader can consume
/// SCM_RIGHTS. Fatal ancillary truncation requires process termination.
struct Wire {
    write: UnixStream,
    read: StreamReceiver,
    id: u64,
}
impl Wire {
    fn new(stream: UnixStream) -> Result<Self, String> {
        stream.set_nonblocking(false).map_err(|_| unavailable())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(30)))
            .map_err(|_| unavailable())?;
        Ok(Self {
            read: StreamReceiver::new(stream.try_clone().map_err(|_| unavailable())?),
            write: stream,
            id: 0,
        })
    }
    fn byte(&mut self, count: usize) -> Result<(u8, Vec<OwnedFd>), String> {
        self.byte_until(count, Instant::now() + Duration::from_secs(30))
    }
    fn byte_until(
        &mut self,
        count: usize,
        deadline: Instant,
    ) -> Result<(u8, Vec<OwnedFd>), String> {
        loop {
            match self.read.receive_byte(count, &crate::cloexec::SPAWN_LOCK) {
                Ok(value) => return Ok(value),
                Err(ReceiveError::Fatal(_)) => std::process::abort(),
                Err(ReceiveError::WouldBlock) => {
                    if Instant::now() >= deadline {
                        return Err(unavailable());
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }
    }
    fn call<C: Call>(
        &mut self,
        params: &C::Params,
        principal: Option<&Principal>,
    ) -> Result<C::Result, String> {
        self.id = self
            .id
            .checked_add(1)
            .filter(|id| *id <= 9_007_199_254_740_991)
            .ok_or_else(unavailable)?;
        // Serialize typed parameters directly: avoid an extra secret-bearing
        // JSON Value tree, and wipe encoded password/session/grant bytes even
        // if a size check or write fails. Wire shape remains Request's shape.
        #[derive(serde::Serialize)]
        struct Outgoing<'a, P> {
            id: u64,
            method: &'static str,
            #[serde(skip_serializing_if = "Option::is_none")]
            principal: Option<&'a Principal>,
            params: &'a P,
        }
        struct Encoded(Vec<u8>);
        impl Drop for Encoded {
            fn drop(&mut self) {
                crate::secrets::zero(&mut self.0);
            }
        }
        let mut bytes = Encoded(Vec::new());
        serde_json::to_writer(
            &mut bytes.0,
            &Outgoing {
                id: self.id,
                method: C::METHOD,
                principal,
                params,
            },
        )
        .map_err(|_| unavailable())?;
        if bytes.0.len() > org_proto::MAX_FRAME_BYTES {
            return Err(unavailable());
        }
        bytes.0.push(b'\n');
        self.write
            .write_all(&bytes.0)
            .and_then(|_| self.write.flush())
            .map_err(|_| unavailable())?;
        drop(bytes);
        let mut raw = Encoded(Vec::new());
        let wait = if C::METHOD == ops::RecoveryRelease::METHOD {
            120
        } else {
            30
        };
        let deadline = Instant::now() + Duration::from_secs(wait);
        loop {
            if Instant::now() >= deadline {
                return Err(unavailable());
            }
            let (byte, empty) = self.byte_until(0, deadline)?;
            debug_assert!(empty.is_empty());
            if byte == b'\n' {
                break;
            }
            if raw.0.len() >= org_proto::MAX_FRAME_BYTES {
                return Err(unavailable());
            }
            raw.0.push(byte);
        }
        let response: Response = serde_json::from_slice(&raw.0).map_err(|_| unavailable())?;
        drop(raw);
        if response.id != self.id {
            return Err(unavailable());
        }
        response.into_result::<C>().map_err(|error| {
            #[cfg(test)]
            match &error {
                org_proto::CallError::Api(error) => {
                    eprintln!("native {} refused: {:?}", C::METHOD, error.code)
                }
                org_proto::CallError::Decode(_) => {
                    eprintln!("native {} result decode failed", C::METHOD)
                }
            }
            let _ = error;
            unavailable()
        })
    }
}
fn session(handle: &SessionHandle) -> SessionHandle {
    SessionHandle::new(handle.expose().into()).expect("live nonempty handle")
}

/// Parses explicit public arguments; passwords never enter argv or environment.
pub fn adopt(arguments: &[String]) -> Result<(), String> {
    adopt_with_password(arguments, password)
}
pub(crate) fn adopt_with_password(
    arguments: &[String],
    credential: impl FnOnce() -> Result<Password, String>,
) -> Result<(), String> {
    let lookup = arguments.first().is_some_and(|s| s == "--lookup");
    let directory = arguments.first().is_some_and(|s| s == "--directory");
    let arguments = if lookup || directory {
        &arguments[1..]
    } else {
        arguments
    };
    let args = adoption_options(arguments)?;
    let get = |key| {
        args.get(key)
            .copied()
            .ok_or_else(|| format!("missing {key}"))
    };
    let instance = get("--instance-id")?;
    let org = get("--org-id")?;
    let office = get("--office-id")?;
    let op = org_proto::gateway::OperationId::parse(get("--operation-id")?.into())
        .map_err(|_| "invalid operation ID")?;
    let path = Path::new(get("--host-socket")?);
    if !path.is_absolute() {
        return Err("host socket must be absolute".into());
    }
    let socket = UnixStream::connect(path).map_err(|_| "cannot connect to selected instance")?;
    verify_host(&socket)?;
    let mut host = Wire::new(socket)?;
    let hello: HelloResult = host.call::<Hello>(
        &HelloParams::offering(org_host_control::SUPPORTED_API, "agentic-org"),
        None,
    )?;
    if hello.instance_id.as_deref() != Some(instance) {
        return Err("selected host instance mismatch".into());
    }
    let login = host.call::<ops::AccountVerify>(
        &ops::AccountVerifyParams {
            login: get("--login")?.into(),
            password: credential()?,
        },
        None,
    )?;
    if !login
        .orgs
        .iter()
        .any(|link| link.org_id == org && link.office_ids.iter().any(|id| id == office))
    {
        return Err("account has no selected office authority".into());
    }
    let (channel_id, mut worker) =
        open_adoption_worker(&mut host, &login.session, instance, org, office)?;
    let mut grant = || {
        host.call::<ops::SessionGrant>(
            &ops::SessionGrantParams {
                session: session(&login.session),
                office_id: office.into(),
                channel_id: channel_id.clone(),
            },
            None,
        )
        .map(|r| Principal { grant: r.grant })
    };
    if lookup {
        let principal = grant()?;
        let out = worker.call::<adoption::Adopt>(
            &adoption::Action::Lookup { operation_id: op },
            Some(&principal),
        )?;
        print_outcome(out);
        return Ok(());
    }
    let stage = org_proto::gateway::OperationId::parse(get("--stage-id")?.into())
        .map_err(|_| "invalid stage ID")?;
    let generation = match get("--directory-generation")? {
        "none" if directory => None,
        raw => Some(
            raw.parse()
                .ok()
                .and_then(|n| org_proto::selected_tasks::Revision::new(n).ok())
                .ok_or("invalid directory generation")?,
        ),
    };
    let file = public_file(Path::new(get("--manifest")?), 16 * 1024 * 1024)?;
    let mut raw = Vec::new();
    file.take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| "cannot read adoption manifest")?;
    if raw.len() > 16 * 1024 * 1024 {
        return Err("adoption manifest budget exceeded".into());
    }
    let blobs = manifest_blobs(&raw, directory)?;
    let digest = hash(&raw);
    let root = PathBuf::from(get("--sources")?);
    let refs = source_references(blobs, raw.len())?;
    // Retain manifest as its own immutable checksum-addressed source.
    upload(
        &mut worker,
        &mut grant,
        &stage,
        &digest,
        &staging::Blob {
            sha256: digest.clone(),
            byte_length: raw.len() as u64,
        },
        &mut raw.as_slice(),
    )?;
    for (sha256, byte_length) in refs {
        let mut file = public_file(&root.join(&sha256), BLOB)?;
        verify_source(&mut file, byte_length, &sha256)?;
        upload(
            &mut worker,
            &mut grant,
            &stage,
            &digest,
            &staging::Blob {
                sha256,
                byte_length,
            },
            &mut file,
        )?;
    }
    let principal = grant()?;
    let action = adoption_action(directory, op, stage, digest, generation)?;
    let out = worker.call::<adoption::Adopt>(&action, Some(&principal))?;
    print_outcome(out);
    Ok(())
}
fn adoption_action(
    directory: bool,
    op: org_proto::gateway::OperationId,
    stage: org_proto::gateway::OperationId,
    digest: String,
    generation: Option<org_proto::selected_tasks::Revision>,
) -> Result<adoption::Action, String> {
    let action = if directory {
        adoption::Action::DirectoryCommit {
            operation_id: op,
            stage_id: stage,
            manifest_sha256: digest,
            expected_directory_generation: generation,
        }
    } else {
        adoption::Action::Commit {
            operation_id: op,
            stage_id: stage,
            manifest_sha256: digest,
            expected_directory_generation: generation.ok_or("task directory is unavailable")?,
        }
    };
    Ok(action)
}

fn adoption_options(arguments: &[String]) -> Result<BTreeMap<&str, &str>, String> {
    let mut args = BTreeMap::new();
    for pair in arguments.chunks(2) {
        if pair.len() != 2
            || !matches!(
                pair[0].as_str(),
                "--host-socket"
                    | "--instance-id"
                    | "--org-id"
                    | "--office-id"
                    | "--login"
                    | "--stage-id"
                    | "--operation-id"
                    | "--directory-generation"
                    | "--manifest"
                    | "--sources"
            )
            || args.insert(pair[0].as_str(), pair[1].as_str()).is_some()
        {
            return Err(
                "invalid adoption arguments; use explicit instance, scope, stage and operation IDs"
                    .into(),
            );
        }
    }
    Ok(args)
}

fn open_adoption_worker(
    host: &mut Wire,
    handle: &SessionHandle,
    instance: &str,
    org: &str,
    office: &str,
) -> Result<(String, Wire), String> {
    let opened = host.call::<ops::OfficeOpenChannel>(
        &ops::OfficeOpenChannelParams {
            session: session(handle),
            office_id: office.into(),
        },
        None,
    )?;
    let (marker, mut descriptors) = host.byte(1)?;
    if marker != ops::HOST_SOCK_MARKER {
        return Err(unavailable());
    }
    let stream =
        org_socket::rights::connected_unix_stream(descriptors.pop().ok_or_else(unavailable)?)
            .map_err(|_| unavailable())?;
    host.call::<ops::FdsReceived>(
        &ops::FdsReceivedParams {
            of: opened.channel_id.clone(),
        },
        None,
    )?;
    let mut worker = Wire::new(stream)?;
    let hello: HelloResult = worker.call::<Hello>(
        &HelloParams::offering(org_proto::ApiRange { min: 2, max: 2 }, "agentic-org"),
        None,
    )?;
    if hello.instance_id.as_deref() != Some(instance)
        || !hello
            .office
            .as_ref()
            .is_some_and(|info| info.office.id == office && info.office.organization_id == org)
        || !hello.supports(adoption::CAPABILITY)
        || !hello.supports(staging::CAPABILITY)
    {
        return Err("selected office adoption is unavailable or scope mismatched".into());
    }
    Ok((opened.channel_id, worker))
}

fn manifest_blobs(raw: &[u8], directory: bool) -> Result<Vec<staging::Blob>, String> {
    let blobs: Vec<staging::Blob> = if directory {
        let manifest: adoption::DirectoryManifest =
            serde_json::from_slice(raw).map_err(|_| "invalid directory manifest")?;
        manifest.files.into_iter().map(|f| f.blob).collect()
    } else {
        let manifest: adoption::Manifest =
            serde_json::from_slice(raw).map_err(|_| "invalid adoption manifest")?;
        std::iter::once(manifest.public_snapshot)
            .chain(manifest.events.into_iter().map(|e| e.blob))
            .chain(manifest.public_blobs)
            .chain(manifest.number_counter)
            .chain(manifest.number_allocation)
            .collect()
    };
    Ok(blobs)
}

fn source_references(
    blobs: Vec<staging::Blob>,
    manifest_length: usize,
) -> Result<BTreeMap<String, u64>, String> {
    let mut refs = BTreeMap::new();
    for blob in blobs {
        if !is_digest(&blob.sha256)
            || blob.byte_length > BLOB
            || refs
                .insert(blob.sha256.clone(), blob.byte_length)
                .is_some_and(|n| n != blob.byte_length)
        {
            return Err("invalid adoption source reference".into());
        }
    }
    if refs.len() > 1023
        || refs
            .values()
            .try_fold(manifest_length as u64, |n, b| n.checked_add(*b))
            .is_none_or(|n| n > BLOB)
    {
        return Err("adoption source budget exceeded".into());
    }
    Ok(refs)
}

fn verify_source(file: &mut std::fs::File, byte_length: u64, sha256: &str) -> Result<(), String> {
    if file
        .metadata()
        .map_err(|_| "source metadata unavailable")?
        .len()
        != byte_length
    {
        return Err("adoption source length mismatch".into());
    }
    let mut hasher = Sha256::new();
    let mut buf = vec![0; CHUNK];
    let mut bounded = std::io::Read::by_ref(file).take(byte_length + 1);
    let mut read = 0u64;
    loop {
        let n = bounded
            .read(&mut buf)
            .map_err(|_| "cannot read adoption source")?;
        if n == 0 {
            break;
        }
        read += n as u64;
        hasher.update(&buf[..n]);
    }
    if read != byte_length || format!("{:x}", hasher.finalize()) != sha256 {
        return Err("adoption source checksum mismatch".into());
    }
    use std::io::Seek;
    file.rewind().map_err(|_| "cannot rewind adoption source")?;
    Ok(())
}

fn upload(
    worker: &mut Wire,
    grant: &mut impl FnMut() -> Result<Principal, String>,
    stage: &org_proto::gateway::OperationId,
    manifest: &str,
    blob: &staging::Blob,
    source: &mut impl Read,
) -> Result<(), String> {
    let principal = grant()?;
    let status = worker.call::<staging::Stage>(
        &staging::Params {
            stage_id: stage.clone(),
            manifest_sha256: manifest.into(),
            blob: blob.clone(),
            chunk: None,
        },
        Some(&principal),
    );
    if status.as_ref().is_ok_and(|s| s.verified) {
        return Ok(());
    }
    let mut remaining = blob.byte_length;
    let mut offset = 0;
    loop {
        let mut bytes = vec![0; remaining.min(CHUNK as u64) as usize];
        source
            .read_exact(&mut bytes)
            .map_err(|_| "adoption source became incomplete")?;
        let hex = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let principal = grant()?;
        let status = worker.call::<staging::Stage>(
            &staging::Params {
                stage_id: stage.clone(),
                manifest_sha256: manifest.into(),
                blob: blob.clone(),
                chunk: Some(staging::Chunk { offset, hex }),
            },
            Some(&principal),
        )?;
        remaining -= bytes.len() as u64;
        offset += bytes.len() as u64;
        if remaining == 0 {
            if !status.verified {
                return Err("staged checksum not verified; original sources retained".into());
            }
            break;
        }
    }
    Ok(())
}
fn print_outcome(out: adoption::Outcome) {
    match out {
        adoption::Outcome::Completed {
            task_id,
            revision,
            history_coverage,
        } => println!("adopted {task_id} revision {revision}; history {history_coverage}"),
        adoption::Outcome::DirectoryCompleted { generation } => {
            println!("directory admitted at generation {generation}")
        }
        adoption::Outcome::Staged { .. } => {
            println!("source retained in staging; task not adopted")
        }
    }
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn is_digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn public_file(path: &Path, limit: u64) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| "cannot open adoption source")?;
    let m = file
        .metadata()
        .map_err(|_| "cannot inspect adoption source")?;
    if !m.is_file() || m.len() > limit || m.nlink() != 1 {
        return Err("unsupported adoption input file".into());
    }
    Ok(file)
}
fn verify_host(stream: &UnixStream) -> Result<(), String> {
    if !org_socket::peer_allowed(stream, org_socket::effective_uid()) {
        return Err("host peer refused".into());
    }
    #[cfg(target_os = "macos")]
    {
        let token = crate::peer::audit_token(stream).map_err(|_| "host identity unavailable")?;
        if let Some(team) = crate::APP_TEAM_ID {
            let req = crate::code_signing::developer_id_requirement(crate::HOST_SIGNING_ID, team)?;
            if crate::code_signing::satisfies_hardened(&token, &req) {
                return Ok(());
            }
        }
        #[cfg(debug_assertions)]
        if crate::peer::pid(stream)
            .and_then(crate::peer::executable)
            .ok()
            == std::env::current_exe().ok()
        {
            return Ok(());
        }
    }
    Err("host signing identity refused".into())
}
fn password() -> Result<Password, String> {
    terminal::password()
}
mod recovery;
mod setup;
pub use recovery::recovery;
mod terminal;
pub use setup::{serve_selected, setup};
#[cfg(all(test, target_os = "macos"))]
pub(crate) mod native_auth_tests;
#[cfg(test)]
pub(crate) mod tests;
