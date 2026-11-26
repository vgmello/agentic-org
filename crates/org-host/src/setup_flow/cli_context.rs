//! Saved instance + office CLI context (A5 / SES-8).
//!
//! Stores only non-secret selection identifiers (`instance_id`, optional `instance_name`,
//! `office_id`) under the account configuration folder with owner-only `0o600` permissions.
//! Never stores passwords, sessions, tokens, or keys.
//!
//! The folder is `AGENTIC_ORG_CONFIG_DIR` when it is set (an absolute path, else refused),
//! then a development build's `ORG_DEV_ACCOUNT_BASE`, then the account's own folder.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use org_platform::fs::{ELOOP, O_CLOEXEC, O_NOFOLLOW};
use serde::{Deserialize, Serialize};

const CONTEXT_FILE: &str = "cli-context.json";

/// The most of a saved context that is read: three identifiers need far less.
pub const MAX_CLI_CONTEXT_BYTES: u64 = 64 * 1024;

/// How many fresh temporary names [`save_to`] tries before it gives up.
const TEMP_ATTEMPTS: u32 = 16;

/// Tells this process's temporary names apart.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The operator command line's configuration folder (an absolute path), where the saved
/// context lives: the same variable as `org_cli_shell::paths::CONFIG_DIR_ENV`.
pub const CONFIG_DIR_ENV: &str = "AGENTIC_ORG_CONFIG_DIR";

/// Non-secret saved selection binding a person's terminal commands to one instance and office.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliContext {
    pub instance_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_name: Option<String>,
    pub office_id: String,
}

impl CliContext {
    pub fn new(
        instance_id: String,
        instance_name: Option<String>,
        office_id: String,
    ) -> Result<Self, String> {
        if !org_proto::is_uuid_v7(&instance_id) {
            return Err("instance_id must be a canonical UUIDv7".into());
        }
        if !org_proto::is_uuid_v7(&office_id) {
            return Err("office_id must be a canonical UUIDv7".into());
        }
        // A saved name is an instance's name (lowercase letters, digits, hyphens): no
        // control, line-breaking or bidirectional character from an edited file ever reaches
        // a terminal or a selection (B-06).
        if let Some(name) = &instance_name {
            if crate::instance::Name::parse(Some(name)).is_err() {
                return Err(
                    "instance_name must be an instance name: 1-32 lowercase ASCII \
                     letters, digits or hyphens, starting with a letter or digit"
                        .into(),
                );
            }
        }
        Ok(Self {
            instance_id,
            instance_name,
            office_id,
        })
    }
}

/// The saved context's file: under `AGENTIC_ORG_CONFIG_DIR` when it is set (an absolute
/// path, else an error: never a file relative to the working directory), else under a
/// development build's `ORG_DEV_ACCOUNT_BASE` (absolute; a release build refuses it), else
/// in the account's folder. An empty variable is unset.
pub fn default_context_path() -> Result<PathBuf, String> {
    context_path_in(
        &|name| std::env::var_os(name),
        cfg!(debug_assertions),
        crate::home::account_dir,
    )
}

fn context_path_in(
    env: &dyn Fn(&str) -> Option<OsString>,
    development: bool,
    account_dir: impl FnOnce() -> Option<PathBuf>,
) -> Result<PathBuf, String> {
    let set = |name: &str| {
        env(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    if let Some(dir) = set(CONFIG_DIR_ENV) {
        if !dir.is_absolute() {
            return Err(format!(
                "{CONFIG_DIR_ENV} must be an absolute path; set it to one, or unset it"
            ));
        }
        return Ok(dir.join(CONTEXT_FILE));
    }
    let dev_base = crate::human::DEV_ACCOUNT_BASE;
    if let Some(base) = set(dev_base) {
        if !development {
            return Err(format!(
                "{dev_base} is a development setting; this release build refuses it"
            ));
        }
        if !base.is_absolute() {
            return Err(format!("{dev_base} must be an absolute path"));
        }
        return Ok(base.join(CONTEXT_FILE));
    }
    let home = account_dir().ok_or_else(|| "account home unavailable".to_owned())?;
    Ok(home.join(CONTEXT_FILE))
}

/// The saved context at `path`, `None` when there is none. The file is opened without
/// following a symbolic link or blocking on a FIFO (`O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC`),
/// must be a regular file on the opened handle (`fstat`), and is read bounded
/// ([`MAX_CLI_CONTEXT_BYTES`]); a link, a special file, a larger file or an invalid context
/// fails closed.
pub fn load_from(path: &Path) -> Result<Option<CliContext>, String> {
    let file = match org_platform::fs::open_nofollow_ro(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) if e.raw_os_error() == Some(ELOOP) || e.kind() == io::ErrorKind::InvalidInput => {
            return Err(NOT_REGULAR.to_owned())
        }
        Err(e) => return Err(format!("could not read CLI context: {e}")),
    };
    let bytes = read_bounded(file)?;
    let ctx: CliContext =
        serde_json::from_slice(&bytes).map_err(|_| "saved CLI context is unreadable".to_owned())?;
    CliContext::new(ctx.instance_id, ctx.instance_name, ctx.office_id).map(Some)
}

const NOT_REGULAR: &str =
    "saved CLI context is not a regular file (a symbolic link or special file is refused)";

fn read_bounded(file: File) -> Result<Vec<u8>, String> {
    let metadata = file
        .metadata()
        .map_err(|e| format!("could not read CLI context: {e}"))?;
    if !metadata.is_file() {
        return Err(NOT_REGULAR.to_owned());
    }
    let mut bytes = Vec::new();
    file.take(MAX_CLI_CONTEXT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("could not read CLI context: {e}"))?;
    if bytes.len() as u64 > MAX_CLI_CONTEXT_BYTES {
        return Err("saved CLI context is larger than 64 KiB".to_owned());
    }
    Ok(bytes)
}

/// Saves `context` at `path` atomically: written to a fresh temporary file in the same
/// folder (a unique name, created new with `O_NOFOLLOW | O_CLOEXEC`, mode `0600` set on the
/// descriptor), flushed to disk, then renamed over `path`. A temporary file is removed when
/// anything fails, and no name a planted link could sit on in advance is ever opened.
pub fn save_to(path: &Path, context: &CliContext) -> Result<(), String> {
    CliContext::new(
        context.instance_id.clone(),
        context.instance_name.clone(),
        context.office_id.clone(),
    )?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| "the CLI context path has no folder".to_owned())?;
    // A folder made here is the user's alone.
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)
        .map_err(|e| format!("could not create context folder: {e}"))?;
    let bytes = serde_json::to_vec_pretty(context)
        .map_err(|_| "could not encode CLI context".to_owned())?;
    let (tmp, mut file) = create_temp(parent)?;
    let written = file
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("could not tighten CLI context permissions: {e}"))
        .and_then(|()| {
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(|e| format!("could not flush CLI context: {e}"))
        })
        .and_then(|()| {
            fs::rename(&tmp, path).map_err(|e| format!("could not install CLI context: {e}"))
        });
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

/// A new, empty temporary file in `dir` with a name no other writer uses: the process ID, a
/// per-process counter and the time. An existing entry under the name (a leftover, a
/// planted link) is never opened: the next name is tried.
fn create_temp(dir: &Path) -> Result<(PathBuf, File), String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    for _ in 0..TEMP_ATTEMPTS {
        let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = dir.join(format!(
            ".{CONTEXT_FILE}.{}.{count}.{nanos}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW | O_CLOEXEC)
            .open(&tmp)
        {
            Ok(file) => return Ok((tmp, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("could not write CLI context: {e}")),
        }
    }
    Err("could not write CLI context: no free temporary name".to_owned())
}

pub fn clear_at(path: &Path) -> Result<bool, String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("could not clear CLI context: {e}")),
    }
}

/// Resolves `(instance_name, office_id)` from explicit flags or saved context, failing closed
/// with actionable guidance when neither provides a selected office (`doc-5` A5 / `SES-8`).
pub fn resolve_selection(
    explicit_instance: Option<&str>,
    explicit_office: Option<&str>,
    saved: Option<&CliContext>,
) -> Result<(Option<String>, String), String> {
    match (explicit_instance, explicit_office, saved) {
        (inst, Some(office), _) => {
            if !org_proto::is_uuid_v7(office) {
                return Err(format!(
                    "office ID `{office}` must be a canonical UUIDv7; run `agentic-org office list` to inspect offices"
                ));
            }
            let name = inst
                .map(str::to_owned)
                .or_else(|| saved.and_then(|s| s.instance_name.clone()));
            Ok((name, office.to_owned()))
        }
        (Some(inst), None, Some(saved))
            if saved.instance_name.as_deref() == Some(inst) =>
        {
            Ok((Some(inst.to_owned()), saved.office_id.clone()))
        }
        (None, None, Some(saved)) => {
            Ok((saved.instance_name.clone(), saved.office_id.clone()))
        }
        _ => Err(
            "no office selected: pass --office <office_id> [--name <instance>] or save a default with `agentic-org context use --office <office_id> [--name <instance>]`"
                .into(),
        ),
    }
}

#[cfg(test)]
mod tests;
