//! Saved instance + office CLI context (A5 / SES-8).
//!
//! Stores only non-secret selection identifiers (`instance_id`, optional `instance_name`,
//! `office_id`) under the account configuration folder with owner-only `0o600` permissions.
//! Never stores passwords, sessions, tokens, or keys.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const CONTEXT_FILE: &str = "cli-context.json";

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
        Ok(Self {
            instance_id,
            instance_name,
            office_id,
        })
    }
}

pub fn default_context_path() -> Result<PathBuf, String> {
    if let Some(base) = std::env::var_os(crate::human::DEV_ACCOUNT_BASE).filter(|b| !b.is_empty()) {
        return Ok(PathBuf::from(base).join(CONTEXT_FILE));
    }
    let home = crate::home::account_dir().ok_or_else(|| "account home unavailable".to_owned())?;
    Ok(home.join(CONTEXT_FILE))
}

pub fn load_from(path: &Path) -> Result<Option<CliContext>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("could not read CLI context: {e}")),
    };
    let ctx: CliContext =
        serde_json::from_slice(&bytes).map_err(|_| "saved CLI context is unreadable".to_owned())?;
    CliContext::new(ctx.instance_id, ctx.instance_name, ctx.office_id).map(Some)
}

pub fn save_to(path: &Path, context: &CliContext) -> Result<(), String> {
    CliContext::new(
        context.instance_id.clone(),
        context.instance_name.clone(),
        context.office_id.clone(),
    )?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("could not create context folder: {e}"))?;
    }
    let bytes = serde_json::to_vec_pretty(context)
        .map_err(|_| "could not encode CLI context".to_owned())?;
    let tmp = path.with_extension("json.tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| format!("could not write CLI context: {e}"))?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("could not tighten CLI context permissions: {e}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("could not flush CLI context: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("could not install CLI context: {e}"))?;
    Ok(())
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
mod tests {
    use super::*;

    const INST: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607101";
    const OFF1: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607102";
    const OFF2: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607103";

    #[test]
    fn context_round_trips_owner_only_and_enforces_selection_rules() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cli-context.json");
        assert_eq!(load_from(&path).unwrap(), None);

        // Missing both explicit and saved context fails with actionable hint.
        let err = resolve_selection(None, None, None).unwrap_err();
        assert!(err.contains("--office"), "{err}");

        // Invalid non-UUIDv7 office rejected immediately.
        assert!(CliContext::new(INST.into(), None, "not-uuid".into()).is_err());

        let ctx = CliContext::new(INST.into(), Some("dev".into()), OFF1.into()).unwrap();
        save_to(&path, &ctx).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(load_from(&path).unwrap(), Some(ctx.clone()));

        // Saved context resolves when omitted; explicit office overrides.
        assert_eq!(
            resolve_selection(None, None, Some(&ctx)).unwrap(),
            (Some("dev".into()), OFF1.into())
        );
        assert_eq!(
            resolve_selection(Some("other"), Some(OFF2), Some(&ctx)).unwrap(),
            (Some("other".into()), OFF2.into())
        );
        // Mismatched explicit instance without explicit office refuses rather than crossing instances.
        assert!(resolve_selection(Some("other"), None, Some(&ctx)).is_err());

        assert!(clear_at(&path).unwrap());
        assert!(!clear_at(&path).unwrap());
    }
}
