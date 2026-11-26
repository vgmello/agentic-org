//! Reading the imported tree without a checkout: `git ls-tree` entries, their mode and
//! size bounds, and blob reads through `git cat-file --batch`.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::Stdio;

use org_store::DurablePath;

use super::git::{
    base_git_command, sanitize_git_stderr, wait_with_timeout, GIT_TIMEOUT, MAX_GIT_STDERR_BYTES,
};
use super::{
    ImportError, MAX_IMPORT_DEPTH, MAX_IMPORT_FILES, MAX_IMPORT_FILE_BYTES, MAX_IMPORT_TOTAL_BYTES,
};

#[derive(Debug)]
pub(crate) struct TreeBlob {
    durable: DurablePath,
    oid: String,
    size: u64,
}

struct RawTreeEntry {
    mode: String,
    kind: String,
    oid: String,
    size: u64,
    path: String,
}

fn decode_ls_tree_raw(raw: &[u8]) -> Result<Vec<RawTreeEntry>, ImportError> {
    let mut entries = Vec::new();
    for record in raw.split(|&b| b == 0).filter(|r| !r.is_empty()) {
        let Some(tab) = record.iter().position(|&b| b == b'\t') else {
            return Err(ImportError::GitFailed("malformed git tree entry".into()));
        };
        let meta = std::str::from_utf8(&record[..tab])
            .map_err(|_| ImportError::GitFailed("non-utf8 git tree metadata".into()))?;
        let path_str =
            std::str::from_utf8(&record[tab + 1..]).map_err(|_| ImportError::InvalidFile {
                path: format!("state/{}", String::from_utf8_lossy(&record[tab + 1..])),
                reason: "file path is not valid UTF-8".into(),
            })?;
        let parts: Vec<&str> = meta.split_whitespace().collect();
        if parts.len() != 4 {
            return Err(ImportError::GitFailed("malformed git tree header".into()));
        }
        let size = if parts[3] == "-" {
            0
        } else {
            parts[3]
                .parse::<u64>()
                .map_err(|_| ImportError::GitFailed("invalid git blob size".into()))?
        };
        entries.push(RawTreeEntry {
            mode: parts[0].to_owned(),
            kind: parts[1].to_owned(),
            oid: parts[2].to_owned(),
            size,
            path: path_str.to_owned(),
        });
    }
    Ok(entries)
}

fn select_state_prefix(entries: &[RawTreeEntry]) -> Result<&'static str, ImportError> {
    if entries.iter().any(|e| e.path == "organization.yaml") {
        Ok("")
    } else if entries.iter().any(|e| e.path == "state/organization.yaml") {
        Ok("state/")
    } else {
        Err(ImportError::InvalidFile {
            path: "state/organization.yaml".into(),
            reason: "the organization record is missing".into(),
        })
    }
}

fn check_entry_mode(entry: &RawTreeEntry, durable_text: &str) -> Result<(), ImportError> {
    if entry.mode == "120000" {
        return Err(ImportError::InvalidFile {
            path: durable_text.to_owned(),
            reason: "symlinks are not permitted in organization state".into(),
        });
    }
    if entry.mode == "160000" {
        return Err(ImportError::InvalidFile {
            path: durable_text.to_owned(),
            reason: "submodules are not permitted in organization state".into(),
        });
    }
    if entry.kind != "blob" || (entry.mode != "100644" && entry.mode != "100755") {
        return Err(ImportError::InvalidFile {
            path: durable_text.to_owned(),
            reason: "only regular files are permitted in organization state".into(),
        });
    }
    Ok(())
}

fn check_entry_bounds(
    rel: &str,
    durable_text: &str,
    size: u64,
    count: usize,
    total_bytes: &mut u64,
) -> Result<(), ImportError> {
    if rel.split('/').count() > MAX_IMPORT_DEPTH {
        return Err(ImportError::InvalidFile {
            path: durable_text.to_owned(),
            reason: "directory nesting exceeds maximum allowed depth".into(),
        });
    }
    if count >= MAX_IMPORT_FILES {
        return Err(ImportError::InvalidFile {
            path: durable_text.to_owned(),
            reason: "organization state exceeds maximum file count limit".into(),
        });
    }
    if size > MAX_IMPORT_FILE_BYTES {
        return Err(ImportError::InvalidFile {
            path: durable_text.to_owned(),
            reason: "file exceeds the single-file import size limit".into(),
        });
    }
    *total_bytes = total_bytes.saturating_add(size);
    if *total_bytes > MAX_IMPORT_TOTAL_BYTES {
        return Err(ImportError::InvalidFile {
            path: durable_text.to_owned(),
            reason: "organization state exceeds total import size limit".into(),
        });
    }
    Ok(())
}

pub(crate) fn parse_ls_tree_entries(raw: &[u8]) -> Result<Vec<TreeBlob>, ImportError> {
    let entries = decode_ls_tree_raw(raw)?;
    let prefix = select_state_prefix(&entries)?;
    let mut blobs = Vec::new();
    let mut total_bytes: u64 = 0;
    for entry in entries {
        let rel = if prefix.is_empty() {
            entry.path.as_str()
        } else {
            entry
                .path
                .strip_prefix(prefix)
                .ok_or_else(|| ImportError::InvalidFile {
                    path: entry.path.clone(),
                    reason: "unrecognized file outside organization state/ directory".into(),
                })?
        };
        let durable_text = format!("state/{rel}");
        check_entry_mode(&entry, &durable_text)?;
        check_entry_bounds(
            rel,
            &durable_text,
            entry.size,
            blobs.len(),
            &mut total_bytes,
        )?;
        let durable = DurablePath::parse(&durable_text).map_err(|_| ImportError::InvalidFile {
            path: durable_text.clone(),
            reason: "invalid organization state file path".into(),
        })?;
        blobs.push(TreeBlob {
            durable,
            oid: entry.oid,
            size: entry.size,
        });
    }
    Ok(blobs)
}

pub(crate) fn read_git_blobs(
    repo_dir: &Path,
    blobs: &[TreeBlob],
    staging: &Path,
) -> Result<BTreeMap<DurablePath, Vec<u8>>, ImportError> {
    let mut cmd = base_git_command();
    cmd.current_dir(repo_dir)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| ImportError::GitFailed(format!("failed to start git cat-file: {e}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| ImportError::GitFailed("git cat-file stdin unavailable".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ImportError::GitFailed("git cat-file stdout unavailable".into()))?;
    let stderr = child.stderr.take();

    let oids: Vec<String> = blobs.iter().map(|b| b.oid.clone()).collect();
    let writer = std::thread::spawn(move || {
        for oid in oids {
            if writeln!(stdin, "{oid}").is_err() {
                break;
            }
        }
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stderr {
            let _ = s.take(MAX_GIT_STDERR_BYTES).read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    });

    let mut reader = BufReader::new(stdout);
    let mut files = BTreeMap::new();
    for blob in blobs {
        let mut header = String::new();
        let n = reader
            .read_line(&mut header)
            .map_err(|_| ImportError::GitFailed("failed reading git object header".into()))?;
        if n == 0 {
            return Err(ImportError::GitFailed(
                "unexpected end of git cat-file output".into(),
            ));
        }
        let parts: Vec<&str> = header.split_whitespace().collect();
        if parts.len() != 3 || parts[1] != "blob" {
            return Err(ImportError::InvalidFile {
                path: blob.durable.as_str().to_owned(),
                reason: "git object is not a readable blob".into(),
            });
        }
        let actual_size = parts[2]
            .parse::<u64>()
            .map_err(|_| ImportError::GitFailed("invalid git object length".into()))?;
        if actual_size != blob.size || actual_size > MAX_IMPORT_FILE_BYTES {
            return Err(ImportError::InvalidFile {
                path: blob.durable.as_str().to_owned(),
                reason: "git blob size mismatch or exceeds limit".into(),
            });
        }
        let mut content = vec![0u8; actual_size as usize];
        reader
            .read_exact(&mut content)
            .map_err(|_| ImportError::GitFailed("truncated git blob stream".into()))?;
        let mut lf = [0u8; 1];
        reader
            .read_exact(&mut lf)
            .map_err(|_| ImportError::GitFailed("missing git object delimiter".into()))?;
        files.insert(blob.durable.clone(), content);
    }

    let _ = writer.join();
    let status = match wait_with_timeout(&mut child, GIT_TIMEOUT) {
        Ok(s) => s,
        Err(true) => return Err(ImportError::GitFailed("git cat-file timed out".into())),
        Err(false) => return Err(ImportError::GitFailed("git cat-file failed".into())),
    };
    let err_text = err_thread.join().unwrap_or_default();
    if !status.success() {
        return Err(ImportError::GitFailed(sanitize_git_stderr(
            &err_text,
            staging,
            "failed to read repository objects",
        )));
    }
    Ok(files)
}
