//! Unattached uploads (`TASK-98.1`, ADR-0037): each in its own owner-only folder under
//! `<durable>/local/offices/<office-id>/attachment-staging/<upload-id>/`, a companion store
//! (`crate::companion_paths`) that the journal never carries and every agent sandbox masks.
//!
//! - `meta.json`: who uploads what, how much arrived, and once committed what was kept.
//! - `data`: the bytes received so far, appended in order.
//! - `kept`: after commit, the validated (re-encoded) bytes that a binding stores.
//!
//! An upload is its uploader's alone: any other caller is told it does not exist. Each
//! employee may hold at most [`MAX_STAGED_FILES`] uploads and [`MAX_STAGED_BYTES`] bytes, and
//! an upload expires [`STAGED_TTL_MS`] after it began; expired uploads are swept on every
//! `begin` and on [`StagingArea::gc`].
//!
//! The Chief of Staff's run acts for one member at a time (`TASK-80.1`), so an upload she makes
//! also names that member (`acting_for_caller_id`, read through [`ActingFor`] when it begins),
//! and is hers only while her run acts for that same member ([`StagingArea::made_for_now`],
//! `TASK-240`): one member's staged file never reaches another's turn or a wake. Every other
//! uploader's runs act for nobody, so their uploads name nobody and nothing changes for them.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use org_proto::{ApiError, ErrorCode};
use serde::{Deserialize, Serialize};

use crate::error::OfficeError;
use crate::private_fs::{
    full_sync, private_dir_synced, private_file, sync_dir, write_private_atomic,
};

use super::blobs::ContentHash;
use super::media::{MediaType, ValidatedFile};

/// The companion folder's name below `durable/local/offices/<office-id>/`.
pub const STAGING_DIR: &str = crate::companion_paths::ATTACHMENT_STAGING_DIR_NAME;
/// The most uploads one employee may have staged.
pub const MAX_STAGED_FILES: usize = 50;
/// The most bytes one employee may have staged (declared, or kept once committed).
pub const MAX_STAGED_BYTES: u64 = 200 * 1024 * 1024;
/// How long an upload may stay unattached.
pub const STAGED_TTL_MS: u64 = 24 * 60 * 60 * 1000;

const META: &str = "meta.json";
const DATA: &str = "data";
const KEPT: &str = "kept";

/// What a committed upload kept.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct CommittedUpload {
    pub mime_type: String,
    pub byte_count: u64,
    pub content_sha256: String,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub page_count: Option<u32>,
}

impl CommittedUpload {
    pub fn media(&self) -> Option<MediaType> {
        MediaType::from_mime(&self.mime_type)
    }

    pub fn hash(&self) -> Option<ContentHash> {
        ContentHash::parse(&self.content_sha256)
    }
}

/// One staged upload, as its `meta.json` holds it.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct StagedUpload {
    pub upload_id: String,
    pub uploaded_by: String,
    pub filename: String,
    pub declared_mime: String,
    pub declared_bytes: u64,
    pub received_bytes: u64,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    #[serde(default)]
    pub committed: Option<CommittedUpload>,
    /// The member the uploader's run acted for when the upload began: set only for the Chief
    /// of Staff's runs (`TASK-240`), `None` for every other upload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acting_for_caller_id: Option<String>,
}

impl StagedUpload {
    /// What the upload counts against its uploader's byte quota.
    fn quota_bytes(&self) -> u64 {
        self.committed.as_ref().map_or(self.declared_bytes, |kept| {
            kept.byte_count.max(self.declared_bytes)
        })
    }
}

/// A new upload's declaration.
pub struct NewUpload<'a> {
    pub upload_id: &'a str,
    pub uploaded_by: &'a str,
    pub filename: &'a str,
    pub media: MediaType,
    pub declared_bytes: u64,
    pub now_ms: u64,
}

/// Who a member's live run acts for (`TASK-80.1`): the member the Chief of Staff's run acts
/// for; `None` for every other member's run, a wake, and a member with no run.
pub type ActingFor = dyn Fn(&str) -> Option<String> + Send + Sync;

/// The staging companion store of one office.
pub struct StagingArea {
    durable: PathBuf,
    root: PathBuf,
    lock: Mutex<()>,
    acting_for: RwLock<Option<Box<ActingFor>>>,
}

fn not_found() -> ApiError {
    ApiError::new(ErrorCode::NotFound, "upload not found")
}

fn conflict(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Conflict, message)
}

impl StagingArea {
    pub fn new(durable_root: &Path, office_id: &str) -> Self {
        Self {
            durable: durable_root.to_path_buf(),
            root: durable_root
                .join(crate::companion_paths::OFFICE_LOCAL_DIR)
                .join(office_id)
                .join(STAGING_DIR),
            lock: Mutex::new(()),
            acting_for: RwLock::new(None),
        }
    }

    /// The companion folder (tests and diagnostics).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Reads who each uploader's live run acts for from now on (set when the office composes
    /// its sessions; replaces any earlier reader).
    pub fn set_acting_for(&self, reader: Box<ActingFor>) {
        *self.acting_for.write().unwrap_or_else(|p| p.into_inner()) = Some(reader);
    }

    /// Who `member`'s live run acts for now; `None` while no reader is set.
    fn acting_for(&self, member: &str) -> Option<String> {
        let reader = self.acting_for.read().unwrap_or_else(|p| p.into_inner());
        reader.as_ref().and_then(|read| read(member))
    }

    /// Whether `upload` was made for the member its uploader's run acts for now: always for
    /// an upload that names nobody made by a run that acts for nobody; never across members.
    pub fn made_for_now(&self, upload: &StagedUpload) -> bool {
        upload.acting_for_caller_id == self.acting_for(&upload.uploaded_by)
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The folder of `upload_id`, which must be a canonical UUIDv7 (never a path).
    fn folder(&self, upload_id: &str) -> Option<PathBuf> {
        org_proto::is_uuid_v7(upload_id).then(|| self.root.join(upload_id))
    }

    fn write_meta(&self, upload: &StagedUpload) -> Result<(), OfficeError> {
        let folder = self.root.join(&upload.upload_id);
        let bytes =
            serde_json::to_vec(upload).map_err(|_| OfficeError::Invalid("staged upload meta"))?;
        write_private_atomic(&folder.join(META), &bytes).map_err(OfficeError::storage)
    }

    /// The upload's meta as it is on disk, whoever it belongs to, if its folder is a real one.
    fn load(&self, upload_id: &str) -> Result<Option<StagedUpload>, OfficeError> {
        let Some(folder) = self.folder(upload_id) else {
            return Ok(None);
        };
        match fs::symlink_metadata(&folder) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(OfficeError::storage(error)),
        }
        let file = match org_platform::fs::open_nofollow_ro(&folder.join(META)) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(OfficeError::storage(error)),
        };
        let mut bytes = Vec::new();
        file.take(64 * 1024)
            .read_to_end(&mut bytes)
            .map_err(OfficeError::storage)?;
        Ok(serde_json::from_slice::<StagedUpload>(&bytes)
            .ok()
            .filter(|meta| meta.upload_id == upload_id))
    }

    /// A live upload, whoever it belongs to, for the policy to decide on. `None` when it is
    /// missing or expired.
    pub fn peek(&self, upload_id: &str, now_ms: u64) -> Result<Option<StagedUpload>, OfficeError> {
        let _guard = self.guard();
        Ok(self
            .load(upload_id)?
            .filter(|meta| now_ms < meta.expires_at_ms))
    }

    /// The caller's live upload, or `None` when it is missing, expired or someone else's.
    pub fn get(
        &self,
        caller: &str,
        upload_id: &str,
        now_ms: u64,
    ) -> Result<Option<StagedUpload>, OfficeError> {
        let _guard = self.guard();
        self.get_locked(caller, upload_id, now_ms)
    }

    fn get_locked(
        &self,
        caller: &str,
        upload_id: &str,
        now_ms: u64,
    ) -> Result<Option<StagedUpload>, OfficeError> {
        Ok(self
            .load(upload_id)?
            .filter(|meta| meta.uploaded_by == caller && now_ms < meta.expires_at_ms))
    }

    /// Every upload on disk with its meta; folders without readable meta are listed as `None`.
    fn all(&self) -> Result<Vec<(String, Option<StagedUpload>)>, OfficeError> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(OfficeError::storage(error)),
        };
        let mut found = Vec::new();
        for entry in entries {
            let entry = entry.map_err(OfficeError::storage)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let meta = self.load(&name)?;
            found.push((name, meta));
        }
        Ok(found)
    }

    fn remove_folder(&self, name: &str) -> Result<(), OfficeError> {
        let path = self.root.join(name);
        let removed = match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() => fs::remove_dir_all(&path),
            Ok(_) => fs::remove_file(&path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => Err(error),
        };
        removed.map_err(OfficeError::storage)?;
        sync_dir(&self.root).map_err(OfficeError::storage)
    }

    fn gc_locked(&self, now_ms: u64) -> Result<usize, OfficeError> {
        let mut swept = 0;
        for (name, meta) in self.all()? {
            if meta.is_none_or(|meta| now_ms >= meta.expires_at_ms) {
                self.remove_folder(&name)?;
                swept += 1;
            }
        }
        Ok(swept)
    }

    /// Removes every expired upload, and anything in the folder that is not an upload.
    pub fn gc(&self, now_ms: u64) -> Result<usize, OfficeError> {
        let _guard = self.guard();
        self.gc_locked(now_ms)
    }

    /// What `caller`'s live uploads other than `except` count against the byte quota.
    fn staged_bytes_locked(
        &self,
        caller: &str,
        except: &str,
        now_ms: u64,
    ) -> Result<u64, OfficeError> {
        Ok(self
            .all()?
            .into_iter()
            .filter_map(|(_, meta)| meta)
            .filter(|meta| {
                meta.uploaded_by == caller
                    && meta.upload_id != except
                    && now_ms < meta.expires_at_ms
            })
            .map(|meta| meta.quota_bytes())
            .fold(0u64, u64::saturating_add))
    }

    /// Starts an upload after sweeping expired ones and checking the uploader's quota.
    pub fn begin(
        &self,
        new: &NewUpload<'_>,
    ) -> Result<Result<StagedUpload, ApiError>, OfficeError> {
        // Read before the lock: the reader takes the session registry's.
        let acting_for_caller_id = self.acting_for(new.uploaded_by);
        let _guard = self.guard();
        self.gc_locked(new.now_ms)?;
        let mine: Vec<StagedUpload> = self
            .all()?
            .into_iter()
            .filter_map(|(_, meta)| meta)
            .filter(|meta| meta.uploaded_by == new.uploaded_by)
            .collect();
        if mine.len() >= MAX_STAGED_FILES {
            return Ok(Err(ApiError::new(
                ErrorCode::RateLimited,
                format!("at most {MAX_STAGED_FILES} uploads may wait to be attached; attach or abort some first"),
            )));
        }
        let staged = mine
            .iter()
            .map(StagedUpload::quota_bytes)
            .fold(0u64, u64::saturating_add);
        if staged.saturating_add(new.declared_bytes) > MAX_STAGED_BYTES {
            return Ok(Err(ApiError::new(
                ErrorCode::TooLarge,
                "at most 200 MiB may wait to be attached; attach or abort some uploads first",
            )));
        }
        let Some(folder) = self.folder(new.upload_id) else {
            return Err(OfficeError::Invalid("an upload id is a UUIDv7"));
        };
        if folder.exists() {
            return Ok(Err(ApiError::new(
                ErrorCode::AlreadyExists,
                "upload id in use",
            )));
        }
        private_dir_synced(&self.durable, &folder).map_err(OfficeError::storage)?;
        let upload = StagedUpload {
            upload_id: new.upload_id.to_owned(),
            uploaded_by: new.uploaded_by.to_owned(),
            filename: new.filename.to_owned(),
            declared_mime: new.media.mime().to_owned(),
            declared_bytes: new.declared_bytes,
            received_bytes: 0,
            created_at_ms: new.now_ms,
            expires_at_ms: new.now_ms.saturating_add(STAGED_TTL_MS),
            committed: None,
            acting_for_caller_id,
        };
        drop(private_file(&folder.join(DATA)).map_err(OfficeError::storage)?);
        self.write_meta(&upload)?;
        Ok(Ok(upload))
    }

    /// Appends `data` at `offset`, which must be the bytes received so far. Sending the last
    /// chunk again (same offset, same bytes) is answered as if it were new.
    pub fn append(
        &self,
        caller: &str,
        upload_id: &str,
        offset: u64,
        data: &[u8],
        now_ms: u64,
    ) -> Result<Result<StagedUpload, ApiError>, OfficeError> {
        let _guard = self.guard();
        let Some(mut upload) = self.get_locked(caller, upload_id, now_ms)? else {
            return Ok(Err(not_found()));
        };
        if upload.committed.is_some() {
            return Ok(Err(conflict("the upload is already committed")));
        }
        let end = offset.saturating_add(data.len() as u64);
        if end > upload.declared_bytes {
            return Ok(Err(ApiError::new(
                ErrorCode::TooLarge,
                "the chunk goes past the declared size",
            )));
        }
        let path = self.root.join(upload_id).join(DATA);
        if offset < upload.received_bytes {
            if end <= upload.received_bytes && self.same_bytes(&path, offset, data)? {
                return Ok(Ok(upload));
            }
            return Ok(Err(conflict(format!(
                "the next chunk starts at offset {}",
                upload.received_bytes
            ))));
        }
        if offset > upload.received_bytes {
            return Ok(Err(conflict(format!(
                "the next chunk starts at offset {}",
                upload.received_bytes
            ))));
        }
        let mut file = private_file(&path).map_err(OfficeError::storage)?;
        // Bytes past what the meta records (a crash between write and meta) are dropped.
        file.set_len(upload.received_bytes)
            .and_then(|()| file.seek(SeekFrom::Start(offset)).map(drop))
            .and_then(|()| file.write_all(data))
            .and_then(|()| full_sync(&file))
            .map_err(OfficeError::storage)?;
        upload.received_bytes = end;
        self.write_meta(&upload)?;
        Ok(Ok(upload))
    }

    fn same_bytes(&self, path: &Path, offset: u64, data: &[u8]) -> Result<bool, OfficeError> {
        let mut file = org_platform::fs::open_nofollow_ro(path).map_err(OfficeError::storage)?;
        file.seek(SeekFrom::Start(offset))
            .map_err(OfficeError::storage)?;
        let mut existing = Vec::with_capacity(data.len());
        file.take(data.len() as u64)
            .read_to_end(&mut existing)
            .map_err(OfficeError::storage)?;
        Ok(existing == data)
    }

    fn read_file(&self, upload_id: &str, name: &str, limit: u64) -> Result<Vec<u8>, OfficeError> {
        let path = self.root.join(upload_id).join(name);
        let file = org_platform::fs::open_nofollow_ro(&path).map_err(OfficeError::storage)?;
        if !file.metadata().map_err(OfficeError::storage)?.is_file() {
            return Err(OfficeError::Invalid(
                "a staged upload file is not a regular file",
            ));
        }
        let mut bytes = Vec::new();
        file.take(limit.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(OfficeError::storage)?;
        if bytes.len() as u64 > limit {
            return Err(OfficeError::Invalid(
                "a staged upload is larger than declared",
            ));
        }
        Ok(bytes)
    }

    /// Validates the complete upload with `validate` (outside the lock: decoding can take a
    /// while) and keeps what it returns. Committing again returns the first result.
    pub fn commit(
        &self,
        caller: &str,
        upload_id: &str,
        now_ms: u64,
        validate: impl FnOnce(MediaType, Vec<u8>) -> Result<ValidatedFile, ApiError>,
    ) -> Result<Result<StagedUpload, ApiError>, OfficeError> {
        let (declared, raw) = {
            let _guard = self.guard();
            let Some(upload) = self.get_locked(caller, upload_id, now_ms)? else {
                return Ok(Err(not_found()));
            };
            if upload.committed.is_some() {
                return Ok(Ok(upload));
            }
            if upload.received_bytes != upload.declared_bytes {
                return Ok(Err(conflict(format!(
                    "{} of {} declared bytes have arrived",
                    upload.received_bytes, upload.declared_bytes
                ))));
            }
            let Some(media) = MediaType::from_mime(&upload.declared_mime) else {
                return Err(OfficeError::Invalid(
                    "a staged upload names an unknown type",
                ));
            };
            (
                media,
                self.read_file(upload_id, DATA, upload.declared_bytes)?,
            )
        };
        let kept = match validate(declared, raw) {
            Ok(kept) => kept,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let _guard = self.guard();
        // The upload may have been aborted or committed by another call meanwhile.
        let Some(mut upload) = self.get_locked(caller, upload_id, now_ms)? else {
            return Ok(Err(not_found()));
        };
        if upload.committed.is_some() {
            return Ok(Ok(upload));
        }
        upload.committed = Some(CommittedUpload {
            mime_type: kept.media.mime().to_owned(),
            byte_count: kept.bytes.len() as u64,
            content_sha256: ContentHash::of(&kept.bytes).to_string(),
            width: kept.width,
            height: kept.height,
            page_count: kept.page_count,
        });
        // An image may come out of re-encoding larger than it went in: the uploader's quota
        // holds for what is kept, checked before anything is written.
        let staged = self.staged_bytes_locked(caller, upload_id, now_ms)?;
        if staged.saturating_add(upload.quota_bytes()) > MAX_STAGED_BYTES {
            return Ok(Err(ApiError::new(
                ErrorCode::TooLarge,
                "at most 200 MiB may wait to be attached, and this file grew past it once its \
                 metadata was removed; attach or abort some uploads first",
            )));
        }
        let folder = self.root.join(upload_id);
        write_private_atomic(&folder.join(KEPT), &kept.bytes).map_err(OfficeError::storage)?;
        self.write_meta(&upload)?;
        // The raw bytes are no longer needed: only the validated copy is ever attached.
        match fs::remove_file(folder.join(DATA)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(OfficeError::storage(error)),
        }
        Ok(Ok(upload))
    }

    /// The kept bytes of a committed upload, checked against the recorded hash.
    pub fn kept_bytes(&self, upload: &StagedUpload) -> Result<Vec<u8>, OfficeError> {
        let committed = upload
            .committed
            .as_ref()
            .ok_or(OfficeError::Invalid("the upload is not committed"))?;
        let bytes = self.read_file(&upload.upload_id, KEPT, committed.byte_count)?;
        if ContentHash::of(&bytes).as_str() != committed.content_sha256 {
            return Err(OfficeError::Invalid("a staged upload changed after commit"));
        }
        Ok(bytes)
    }

    /// Removes the caller's upload. Anyone else is told it does not exist.
    pub fn abort(
        &self,
        caller: &str,
        upload_id: &str,
        now_ms: u64,
    ) -> Result<Result<(), ApiError>, OfficeError> {
        let _guard = self.guard();
        if self.get_locked(caller, upload_id, now_ms)?.is_none() {
            return Ok(Err(not_found()));
        }
        self.remove_folder(upload_id)?;
        Ok(Ok(()))
    }

    /// Removes an upload once it is attached.
    pub fn remove(&self, upload_id: &str) -> Result<(), OfficeError> {
        let _guard = self.guard();
        if self.folder(upload_id).is_none() {
            return Ok(());
        }
        self.remove_folder(upload_id)
    }
}
