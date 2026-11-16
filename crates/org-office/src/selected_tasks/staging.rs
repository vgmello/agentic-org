//! Protected, opaque adoption uploads. These files never enter public history or
//! the acceptance journal. Publication of a chunk is not adoption acceptance.
use super::{service, OwnerAuthority};
use crate::{
    store::{acceptance::durable, Store},
    Office, OfficeError,
};
use org_proto::selected_tasks::staging::{Blob, Chunk, Params, Status};
use org_work::selected_codec as codec;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
};

fn invalid() -> OfficeError {
    OfficeError::Invalid("selected staging source rejected")
}
fn storage(e: impl std::error::Error + Send + Sync + 'static) -> OfficeError {
    OfficeError::storage(e)
}
fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(super) fn source(
    root: &Path,
    id: &super::AcceptanceIdentity,
    owner: &super::OwnerAccount,
    stage: &str,
    manifest: &str,
    blob: &Blob,
    limit: usize,
) -> Result<Vec<u8>, OfficeError> {
    if !digest(manifest)
        || !digest(&blob.sha256)
        || blob.byte_length > limit as u64
        || blob.byte_length > codec::MAX_BLOB_BYTES
    {
        return Err(invalid());
    }
    // stage is an already-decoded canonical operation ULID at every entry port.
    if org_proto::gateway::OperationId::parse(stage.into()).is_err() {
        return Err(invalid());
    }
    let path = root.join(stage);
    ensure(root, &path, false)?;
    let retained: Binding =
        serde_json::from_slice(&durable::read(&path, "binding.v1.json", 4096).map_err(storage)?)
            .map_err(storage)?;
    let expected = Binding {
        version: 1,
        resource_profile: codec::RESOURCE_PROFILE.into(),
        instance: id.instance_id.clone(),
        organization: id.organization_id.clone(),
        office: id.office_id.clone(),
        account: owner.account_id.clone(),
        manifest_sha256: manifest.into(),
    };
    if retained != expected {
        return Err(OfficeError::Invalid("not authorized"));
    }
    let dir = path.join("blobs").join(&blob.sha256);
    ensure(&path, &dir, false)?;
    let descriptor: Blob =
        serde_json::from_slice(&durable::read(&dir, "blob.v1.json", 1024).map_err(storage)?)
            .map_err(storage)?;
    if descriptor != *blob {
        return Err(invalid());
    }
    let mut bytes = Vec::with_capacity(usize::try_from(blob.byte_length).map_err(|_| invalid())?);
    let count = blob
        .byte_length
        .div_ceil(codec::MAX_CHUNK_BYTES as u64)
        .max(1);
    for index in 0..count {
        let offset = index * codec::MAX_CHUNK_BYTES as u64;
        let chunk = durable::read(&dir, &format!("{offset}.bin"), codec::MAX_CHUNK_BYTES)
            .map_err(storage)?;
        if chunk.len() as u64 != (blob.byte_length - offset).min(codec::MAX_CHUNK_BYTES as u64) {
            return Err(invalid());
        }
        bytes.extend_from_slice(&chunk);
    }
    if codec::sha256(&bytes) != blob.sha256 {
        return Err(invalid());
    }
    Ok(bytes)
}

pub(super) fn manifest_source(
    root: &Path,
    id: &super::AcceptanceIdentity,
    owner: &super::OwnerAccount,
    stage: &str,
    digest: &str,
) -> Result<Vec<u8>, OfficeError> {
    if !self::digest(digest) || org_proto::gateway::OperationId::parse(stage.into()).is_err() {
        return Err(invalid());
    }
    let stage_dir = root.join(stage);
    ensure(root, &stage_dir, false)?;
    let dir = stage_dir.join("blobs").join(digest);
    ensure(&stage_dir, &dir, false)?;
    let blob: Blob =
        serde_json::from_slice(&durable::read(&dir, "blob.v1.json", 1024).map_err(storage)?)
            .map_err(storage)?;
    if blob.sha256 != digest {
        return Err(invalid());
    }
    source(root, id, owner, stage, digest, &blob, codec::MAX_JSON_BYTES)
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Binding {
    version: u32,
    resource_profile: String,
    instance: String,
    organization: String,
    office: String,
    account: String,
    manifest_sha256: String,
}

impl Office {
    pub fn stage_selected_sources(
        &self,
        authority: Arc<dyn OwnerAuthority>,
        params: Params,
    ) -> Result<Status, OfficeError> {
        // The trusted office folder, never an ingress-supplied filesystem path.
        run(
            self.local_store(),
            self.folder().join("selected-adoption"),
            authority,
            params,
        )
    }
}

fn ensure(root: &Path, path: &Path, create: bool) -> Result<(), OfficeError> {
    use std::os::unix::fs::DirBuilderExt;
    if !root.is_absolute() || !path.starts_with(root) {
        return Err(invalid());
    }
    let mut current = root.to_owned();
    for next in std::iter::once(None).chain(
        path.strip_prefix(root)
            .map_err(storage)?
            .components()
            .map(Some),
    ) {
        if let Some(next) = next {
            if !matches!(next, std::path::Component::Normal(_)) {
                return Err(invalid());
            }
            current.push(next);
        }
        if create && !current.try_exists().map_err(storage)? {
            match fs::DirBuilder::new().mode(0o700).create(&current) {
                Ok(()) => {
                    fs::File::open(current.parent().ok_or_else(invalid)?)
                        .map_err(storage)?
                        .sync_all()
                        .map_err(storage)?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(storage(e)),
            }
        }
        let m = fs::symlink_metadata(&current).map_err(storage)?;
        // No chmod of an unfamiliar path and no symlink/hardlink redirection.
        // SAFETY: geteuid has no arguments or memory preconditions.
        let uid = unsafe { libc::geteuid() };
        if !m.is_dir() || m.file_type().is_symlink() || m.mode() & 0o7777 != 0o700 || m.uid() != uid
        {
            return Err(invalid());
        }
    }
    Ok(())
}

pub(super) fn run(
    store: &Store,
    root: PathBuf,
    authority: Arc<dyn OwnerAuthority>,
    params: Params,
) -> Result<Status, OfficeError> {
    let Params {
        stage_id,
        manifest_sha256,
        blob,
        chunk,
    } = params;
    if !digest(&manifest_sha256)
        || !digest(&blob.sha256)
        || blob.byte_length > codec::MAX_BLOB_BYTES
    {
        return Err(invalid());
    }
    let bytes = decode_chunk(&blob, chunk.as_ref())?;
    // This is the existing writer's nonretrying FIFO barrier, not a second
    // writer or a journal transaction. Only protected staging is touched.
    store.selected_read(move |db, id| {
        let owner = service::owner(db, id, &*authority)?;
        let stage = root.join(stage_id.as_str());
        ensure(&root, &stage, chunk.is_some())?;
        let binding = Binding {
            version: 1,
            resource_profile: codec::RESOURCE_PROFILE.into(),
            instance: id.instance_id.clone(),
            organization: id.organization_id.clone(),
            office: id.office_id.clone(),
            account: owner.account_id,
            manifest_sha256: manifest_sha256.clone(),
        };
        let encoded = serde_json::to_vec(&binding).map_err(storage)?;
        if chunk.is_some() {
            durable::publish(&stage, "binding.v1.json", &encoded, true, None).map_err(storage)?;
        }
        let retained: Binding = serde_json::from_slice(
            &durable::read(&stage, "binding.v1.json", 4096).map_err(storage)?,
        )
        .map_err(storage)?;
        if retained != binding {
            return Err(OfficeError::Invalid("not authorized"));
        }
        let blobs = stage.join("blobs");
        ensure(&stage, &blobs, chunk.is_some())?;
        if chunk.is_some() {
            reserve_blob(&blobs, &blob)?;
        }
        let dir = blobs.join(&blob.sha256);
        ensure(&stage, &dir, chunk.is_some())?;
        let descriptor = serde_json::to_vec(&blob).map_err(storage)?;
        if chunk.is_some() {
            durable::publish(&dir, "blob.v1.json", &descriptor, true, None).map_err(storage)?;
        }
        if durable::read(&dir, "blob.v1.json", 1024).map_err(storage)? != descriptor {
            return Err(invalid());
        }
        if let (Some(chunk), Some(bytes)) = (&chunk, &bytes) {
            durable::publish(&dir, &format!("{}.bin", chunk.offset), bytes, true, None)
                .map_err(storage)?;
        }
        let (offsets, complete, actual) = inspect_blob(&dir, &blob)?;
        Ok(Status {
            stage_id,
            manifest_sha256,
            verified: complete && actual == blob.sha256,
            blob,
            chunk_offsets: offsets,
        })
    })
}

fn decode_chunk(blob: &Blob, chunk: Option<&Chunk>) -> Result<Option<Vec<u8>>, OfficeError> {
    // Check encoded/raw work budgets before decoding/allocation.
    Ok(if let Some(chunk) = chunk {
        if chunk.hex.len() > codec::MAX_CHUNK_BYTES * 2
            || chunk.hex.len() % 2 != 0
            || chunk
                .hex
                .bytes()
                .any(|b| !b.is_ascii_digit() && !(b'a'..=b'f').contains(&b))
            || chunk.offset % codec::MAX_CHUNK_BYTES as u64 != 0
        {
            return Err(invalid());
        }
        let length = (chunk.hex.len() / 2) as u64;
        let remaining = blob
            .byte_length
            .checked_sub(chunk.offset)
            .ok_or_else(invalid)?;
        if length != remaining.min(codec::MAX_CHUNK_BYTES as u64)
            || (blob.byte_length > 0 && length == 0)
        {
            return Err(invalid());
        }
        let mut bytes = Vec::with_capacity(length as usize);
        for pair in chunk.hex.as_bytes().as_chunks::<2>().0 {
            let digit = |b: u8| if b <= b'9' { b - b'0' } else { b - b'a' + 10 };
            bytes.push(digit(pair[0]) * 16 + digit(pair[1]));
        }
        Some(bytes)
    } else {
        None
    })
}

fn reserve_blob(blobs: &Path, blob: &Blob) -> Result<(), OfficeError> {
    // Reserve the descriptor before chunk allocation. Retained incomplete
    // uploads count toward aggregate budgets and are never truncated.
    let mut total = blob.byte_length;
    let mut refs = 1usize;
    for entry in fs::read_dir(blobs).map_err(storage)? {
        let entry = entry.map_err(storage)?;
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(invalid)?;
        if !digest(name) {
            return Err(invalid());
        }
        if name == blob.sha256 {
            continue;
        }
        refs = refs.checked_add(1).ok_or_else(invalid)?;
        if refs > codec::MAX_BLOB_REFS {
            return Err(invalid());
        }
        ensure(blobs, &entry.path(), false)?;
        let old: Blob = serde_json::from_slice(
            &durable::read(&entry.path(), "blob.v1.json", 1024).map_err(storage)?,
        )
        .map_err(storage)?;
        if old.sha256 != name {
            return Err(invalid());
        }
        total = total
            .checked_add(old.byte_length)
            .filter(|n| *n <= codec::MAX_BLOB_BYTES)
            .ok_or_else(invalid)?;
    }
    Ok(())
}

fn inspect_blob(dir: &Path, blob: &Blob) -> Result<(Vec<u64>, bool, String), OfficeError> {
    let mut offsets = Vec::new();
    let mut hash = Sha256::new();
    let mut complete = true;
    let count = blob
        .byte_length
        .div_ceil(codec::MAX_CHUNK_BYTES as u64)
        .max(1);
    for index in 0..count {
        let offset = index * codec::MAX_CHUNK_BYTES as u64;
        let name = format!("{offset}.bin");
        match durable::read(dir, &name, codec::MAX_CHUNK_BYTES) {
            Ok(bytes) => {
                if bytes.len() as u64
                    != (blob.byte_length - offset).min(codec::MAX_CHUNK_BYTES as u64)
                {
                    return Err(invalid());
                }
                offsets.push(offset);
                hash.update(bytes);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => complete = false,
            Err(e) => return Err(storage(e)),
        }
    }
    let actual: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok((offsets, complete, actual))
}
