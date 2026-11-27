//! Durable file I/O: price/usage/checkpoint/subscription-usage loading, bounded snapshot
//! reads, JSONL tail repair and private atomic writes.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use org_proto::costs::{ModelPriceView, UsageRecordView};

use org_accounting::AccountingError;
use org_accounting::{StoredSubscriptionUsageSnapshot, UsageStreamCheckpoint};
use org_office::private_fs::{full_sync, private_dir_synced, sync_dir, write_private_atomic};

pub(super) fn sync_usage_dir_and_parent(dir: &Path) -> Result<(), AccountingError> {
    sync_dir(dir).map_err(AccountingError::storage)?;
    if let Some(parent) = dir.parent() {
        sync_dir(parent).map_err(AccountingError::storage)?;
    }
    Ok(())
}

pub(super) fn write_durable_subscription_usage(
    durable_org_root: &Path,
    dir: &Path,
    snap: &StoredSubscriptionUsageSnapshot,
) -> Result<(), AccountingError> {
    private_dir_synced(durable_org_root, dir).map_err(AccountingError::storage)?;
    sync_usage_dir_and_parent(dir)?;
    let path = dir.join(format!("{}.json", snap.connection_id));
    let bytes = serde_json::to_vec_pretty(snap).map_err(AccountingError::storage)?;
    write_atomic(&path, &bytes)
}

const MAX_DURABLE_SNAPSHOT_FILE_BYTES: u64 = 64 * 1024;

fn read_bounded_snapshot_file(path: &Path) -> Result<Vec<u8>, AccountingError> {
    use std::io::Read;
    let file = fs::File::open(path).map_err(AccountingError::storage)?;
    let mut bytes = Vec::new();
    file.take(MAX_DURABLE_SNAPSHOT_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(AccountingError::storage)?;
    if bytes.len() as u64 > MAX_DURABLE_SNAPSHOT_FILE_BYTES {
        return Err(AccountingError::Invalid(
            "durable subscription usage snapshot file exceeds size limit",
        ));
    }
    Ok(bytes)
}

pub(super) fn read_durable_snapshot_if_present(
    dir: &Path,
    connection_id: &str,
) -> Result<Option<StoredSubscriptionUsageSnapshot>, AccountingError> {
    let path = dir.join(format!("{connection_id}.json"));
    match fs::File::open(&path) {
        Ok(_) => {
            let bytes = read_bounded_snapshot_file(&path)?;
            let snap: StoredSubscriptionUsageSnapshot =
                serde_json::from_slice(&bytes).map_err(AccountingError::storage)?;
            Ok(Some(snap))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(AccountingError::storage(err)),
    }
}

pub(super) fn load_durable_subscription_usage(
    dir: &Path,
    org_id: &str,
    office_id: &str,
) -> Result<Vec<StoredSubscriptionUsageSnapshot>, AccountingError> {
    let entries = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(AccountingError::storage(err)),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.map_err(AccountingError::storage)?.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
            files.push(path);
        }
    }
    files.sort();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for path in files {
        let bytes = read_bounded_snapshot_file(&path)?;
        let snap: StoredSubscriptionUsageSnapshot =
            serde_json::from_slice(&bytes).map_err(AccountingError::storage)?;
        let expected_name = format!("{}.json", snap.connection_id);
        let actual_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if snap.organization_id != org_id
            || snap.office_id != office_id
            || actual_name != expected_name
            || org_accounting::validate_stored_snapshot(&snap).is_err()
            || !seen.insert(snap.connection_id.clone())
        {
            return Err(AccountingError::Invalid(
                "invalid or mismatched durable subscription usage snapshot file",
            ));
        }
        out.push(snap);
    }
    Ok(out)
}

pub(super) fn load_durable_prices(
    root: &Path,
    flush_files_and_dirs: bool,
) -> Result<Vec<ModelPriceView>, AccountingError> {
    let dir = root.join("prices");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).map_err(AccountingError::storage)? {
        let path = entry.map_err(AccountingError::storage)?.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            if flush_files_and_dirs {
                let f = fs::File::open(&path).map_err(AccountingError::storage)?;
                full_sync(&f).map_err(AccountingError::storage)?;
            }
            let bytes = fs::read(&path).map_err(AccountingError::storage)?;
            out.push(serde_json::from_slice(&bytes).map_err(AccountingError::storage)?);
        }
    }
    if flush_files_and_dirs {
        private_dir_synced(root, &dir).map_err(AccountingError::storage)?;
    }
    Ok(out)
}

fn load_usage_jsonl_file_into(
    root: &Path,
    path: &Path,
    org_id: &str,
    flush_files_and_dirs: bool,
    seen: &mut BTreeMap<String, UsageRecordView>,
    out: &mut Vec<UsageRecordView>,
) -> Result<(), AccountingError> {
    if flush_files_and_dirs {
        let f = fs::File::open(path).map_err(AccountingError::storage)?;
        full_sync(&f).map_err(AccountingError::storage)?;
        if let Some(parent) = path.parent() {
            private_dir_synced(root, parent).map_err(AccountingError::storage)?;
        }
    }
    for rec in read_and_repair_usage_jsonl_file(path)? {
        let expected_sha = org_accounting::compute_usage_payload_sha256(org_id, &rec);
        if rec.payload_sha256 != expected_sha {
            return Err(AccountingError::Invalid(
                "durable usage record payload_sha256 mismatch",
            ));
        }
        match seen.get(&rec.id) {
            Some(existing) if existing == &rec => {}
            Some(_) => {
                return Err(AccountingError::Invalid(
                    "conflicting duplicate usage record id in durable JSONL",
                ));
            }
            None => {
                seen.insert(rec.id.clone(), rec.clone());
                out.push(rec);
            }
        }
    }
    Ok(())
}

fn validate_durable_usage_invariants(records: &[UsageRecordView]) -> Result<(), AccountingError> {
    let mut seen_supersedes: BTreeSet<String> = BTreeSet::new();
    let mut seen_source_events: BTreeSet<(String, String, String)> = BTreeSet::new();
    let mut seen_sequences: BTreeSet<(String, String, u64)> = BTreeSet::new();
    for rec in records {
        if !org_proto::is_uuid_v7(&rec.id) {
            return Err(AccountingError::Invalid(
                "durable usage record id is not a canonical UUIDv7",
            ));
        }
        if !seen_source_events.insert((
            rec.origin_office_id.clone(),
            rec.producer_id.clone(),
            rec.source_event_id.clone(),
        )) {
            return Err(AccountingError::Invalid(
                "conflicting duplicate usage source_event_id in durable JSONL",
            ));
        }
        if !seen_sequences.insert((
            rec.origin_office_id.clone(),
            rec.producer_id.clone(),
            rec.source_sequence,
        )) {
            return Err(AccountingError::Invalid(
                "conflicting duplicate usage source_sequence in durable JSONL",
            ));
        }
        if let Some(parent_id) = rec.supersedes_id.as_deref() {
            if !seen_supersedes.insert(parent_id.to_owned()) {
                return Err(AccountingError::Invalid(
                    "duplicate durable usage correction for the same parent id",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn load_durable_usage_records(
    root: &Path,
    org_id: &str,
    flush_files_and_dirs: bool,
) -> Result<Vec<UsageRecordView>, AccountingError> {
    let usage_dir = root.join("usage");
    if !usage_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_files_with_ext(&usage_dir, "jsonl", &mut files)?;
    files.sort();
    let mut out = Vec::new();
    let mut seen: BTreeMap<String, UsageRecordView> = BTreeMap::new();
    for path in files {
        load_usage_jsonl_file_into(
            root,
            &path,
            org_id,
            flush_files_and_dirs,
            &mut seen,
            &mut out,
        )?;
    }
    validate_durable_usage_invariants(&out)?;
    out.sort_by(|a, b| {
        a.recorded_at
            .cmp(&b.recorded_at)
            .then_with(|| a.source_sequence.cmp(&b.source_sequence))
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(out)
}

/// O(1) check before appending: verifies the last byte is `\n` and ONLY inspects/repairs the
/// trailing fragment when the file lacks a terminating newline (`TASK-97` / Pass 2 Finding #9).
pub(super) fn ensure_jsonl_ends_with_newline(path: &Path) -> Result<(), AccountingError> {
    let mut f = fs::File::open(path).map_err(AccountingError::storage)?;
    let len = f.metadata().map_err(AccountingError::storage)?.len();
    if len == 0 {
        return Ok(());
    }
    f.seek(SeekFrom::End(-1))
        .map_err(AccountingError::storage)?;
    let mut last = [0u8; 1];
    f.read_exact(&mut last).map_err(AccountingError::storage)?;
    if last[0] == b'\n' {
        return Ok(());
    }
    drop(f);
    let _ = read_and_repair_usage_jsonl_file(path)?;
    Ok(())
}

/// Reads all complete usage records from `path`, durably repairing any non-newline-terminated
/// trailing fragment (`TASK-97` / Finding #4):
/// - Complete JSON without trailing `\n`: appends `\n` + `full_sync`.
/// - Crash-torn invalid suffix without trailing `\n`: truncates back to the last `\n` + `full_sync`.
/// - Newline-terminated corrupt line anywhere in the file: fails closed.
fn read_and_repair_usage_jsonl_file(path: &Path) -> Result<Vec<UsageRecordView>, AccountingError> {
    let raw = fs::read(path).map_err(AccountingError::storage)?;
    let (complete_part, tail_part) = match raw.iter().rposition(|&b| b == b'\n') {
        Some(pos) => (&raw[..=pos], &raw[pos + 1..]),
        None => (&[][..], raw.as_slice()),
    };
    let mut out = Vec::new();
    for line in complete_part.split(|&b| b == b'\n') {
        let trimmed = trim_ascii_whitespace(line);
        if !trimmed.is_empty() {
            out.push(parse_usage_jsonl_line(trimmed)?);
        }
    }
    if !tail_part.is_empty() {
        let trimmed_tail = trim_ascii_whitespace(tail_part);
        if trimmed_tail.is_empty() {
            truncate_and_sync(path, complete_part.len() as u64)?;
        } else if let Ok(rec) = parse_usage_jsonl_line(trimmed_tail) {
            append_newline_and_sync(path)?;
            out.push(rec);
        } else {
            eprintln!(
                "agentic-org-office: truncating torn trailing JSONL fragment in {}",
                path.display()
            );
            truncate_and_sync(path, complete_part.len() as u64)?;
        }
    }
    Ok(out)
}

fn truncate_and_sync(path: &Path, len: u64) -> Result<(), AccountingError> {
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(AccountingError::storage)?;
    file.set_len(len).map_err(AccountingError::storage)?;
    full_sync(&file).map_err(AccountingError::storage)
}

fn append_newline_and_sync(path: &Path) -> Result<(), AccountingError> {
    let mut file = open_private_append(path)?;
    file.write_all(b"\n").map_err(AccountingError::storage)?;
    full_sync(&file).map_err(AccountingError::storage)
}

fn trim_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |i| i + 1);
    &bytes[start..end]
}

fn parse_usage_jsonl_line(bytes: &[u8]) -> Result<UsageRecordView, AccountingError> {
    let text = std::str::from_utf8(bytes).map_err(AccountingError::storage)?;
    serde_json::from_str(text).map_err(AccountingError::storage)
}

pub(super) fn load_durable_checkpoints(
    root: &Path,
) -> Result<Vec<UsageStreamCheckpoint>, AccountingError> {
    let cp_dir = root.join("checkpoints");
    if !cp_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_files_with_ext(&cp_dir, "json", &mut files)?;
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for path in files {
        let bytes = fs::read(&path).map_err(AccountingError::storage)?;
        let cp: UsageStreamCheckpoint =
            serde_json::from_slice(&bytes).map_err(AccountingError::storage)?;
        let expected_path = cp_dir
            .join(safe_segment(&cp.origin_office_id))
            .join(format!("{}.json", safe_segment(&cp.producer_id)));
        if path != expected_path {
            return Err(AccountingError::Invalid(
                "durable usage checkpoint path does not match canonical stream path",
            ));
        }
        let stream_key = (
            cp.organization_id.clone(),
            cp.origin_office_id.clone(),
            cp.producer_id.clone(),
        );
        if !seen.insert(stream_key) {
            return Err(AccountingError::Invalid(
                "duplicate durable usage checkpoint for stream identity",
            ));
        }
        out.push(cp);
    }
    Ok(out)
}

fn collect_files_with_ext(
    dir: &Path,
    ext: &str,
    out: &mut Vec<PathBuf>,
) -> Result<(), AccountingError> {
    for entry in fs::read_dir(dir).map_err(AccountingError::storage)? {
        let path = entry.map_err(AccountingError::storage)?.path();
        if path.is_dir() {
            collect_files_with_ext(&path, ext, out)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some(ext) {
            out.push(path);
        }
    }
    Ok(())
}

/// Collision-free, case-insensitive-filesystem-safe path component encoding (`TASK-97` / Pass 13).
/// Keeps lowercase `[a-z0-9-]` literal and hex-escapes every other byte as `_xx`, with a SHA-256
/// suffix if the encoded component would exceed 180 bytes.
pub(super) fn safe_segment(raw: &str) -> String {
    let mut encoded = String::with_capacity(raw.len());
    for &b in raw.as_bytes() {
        if b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' {
            encoded.push(b as char);
        } else {
            use std::fmt::Write;
            let _ = write!(&mut encoded, "_{b:02x}");
        }
    }
    if encoded.len() <= 180 {
        return encoded;
    }
    let mut hash_hex = String::with_capacity(64);
    for b in org_store::Sha256::of(raw.as_bytes()).as_bytes() {
        use std::fmt::Write;
        let _ = write!(&mut hash_hex, "{b:02x}");
    }
    encoded.truncate(112);
    format!("{encoded}__{hash_hex}")
}

pub(super) fn open_private_append(path: &Path) -> Result<fs::File, AccountingError> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .map_err(AccountingError::storage)
}

pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), AccountingError> {
    write_private_atomic(path, bytes).map_err(AccountingError::storage)
}
