//! Saved instance + office CLI context (A5 / SES-8).
//!
//! Stores only non-secret selection identifiers (`instance_id`, optional `instance_name`,
//! `office_id`) under the account configuration folder with owner-only `0o600` permissions.
//! Never stores passwords, sessions, tokens, or keys.
//!
//! The folder is `AGENTIC_ORG_CONFIG_DIR` when it is set (an absolute path, else refused),
//! then a development build's `ORG_DEV_ACCOUNT_BASE`, then the account's own folder.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const CONTEXT_FILE: &str = "cli-context.json";

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
        // A folder made here is the user's alone.
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .map_err(|e| format!("could not create context folder: {e}"))?;
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

    /// B-01: `AGENTIC_ORG_CONFIG_DIR` decides where the context lives, before the development
    /// base and the account folder; a relative one is refused, never resolved against the
    /// working directory; an empty one is unset.
    #[test]
    fn the_config_dir_variable_places_the_context_and_a_relative_one_is_refused() {
        fn lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
            move |name| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| OsString::from(value))
            }
        }
        let account = || Some(PathBuf::from("/home/a/acct"));
        let dev = crate::human::DEV_ACCOUNT_BASE;
        let path = |pairs: &[(&str, &str)], development| {
            context_path_in(&lookup(pairs), development, account)
        };
        assert_eq!(
            path(&[(CONFIG_DIR_ENV, "/cfg"), (dev, "/dev-base")], true),
            Ok(PathBuf::from("/cfg/cli-context.json"))
        );
        // The variable is the user's, so a release build honours it too.
        assert_eq!(
            path(&[(CONFIG_DIR_ENV, "/cfg")], false),
            Ok(PathBuf::from("/cfg/cli-context.json"))
        );
        for relative in ["cfg", "./cfg", "../cfg", "~/cfg"] {
            let error = path(&[(CONFIG_DIR_ENV, relative)], true).unwrap_err();
            assert!(error.contains(CONFIG_DIR_ENV), "{relative}: {error}");
        }
        // Empty is unset: the development base, then the account folder.
        assert_eq!(
            path(&[(CONFIG_DIR_ENV, ""), (dev, "/dev-base")], true),
            Ok(PathBuf::from("/dev-base/cli-context.json"))
        );
        assert_eq!(
            path(&[], true),
            Ok(PathBuf::from("/home/a/acct/cli-context.json"))
        );
        // The development base: absolute, and a development build's only.
        assert!(path(&[(dev, "rel")], true).is_err());
        assert!(path(&[(dev, "/dev-base")], false)
            .unwrap_err()
            .contains("release build"));
        assert!(context_path_in(&lookup(&[]), true, || None).is_err());
    }

    /// B-06: a context file edited to carry control, line-breaking or bidirectional
    /// characters (or any name that is not an instance name) fails closed when it is read
    /// and is never written.
    #[test]
    fn a_poisoned_instance_name_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cli-context.json");
        for name in [
            "lab\u{1b}[2J",
            "lab\u{202e}gnp",
            "lab\u{2066}x",
            "lab\u{9b}31m",
            "lab\nnext",
            "Lab",
            "",
            "-lab",
            "a-very-long-instance-name-past-32-bytes",
        ] {
            assert!(
                CliContext::new(INST.into(), Some(name.into()), OFF1.into()).is_err(),
                "{name:?}"
            );
            let poisoned = serde_json::json!({
                "instance_id": INST,
                "instance_name": name,
                "office_id": OFF1,
            });
            fs::write(&path, poisoned.to_string()).unwrap();
            assert!(load_from(&path).is_err(), "{name:?} was loaded");
            let unsaved = CliContext {
                instance_id: INST.into(),
                instance_name: Some(name.into()),
                office_id: OFF1.into(),
            };
            assert!(save_to(&path, &unsaved).is_err(), "{name:?} was saved");
        }
        // A poisoned office ID fails closed the same way.
        let poisoned = serde_json::json!({"instance_id": INST, "office_id": "x\u{202e}y"});
        fs::write(&path, poisoned.to_string()).unwrap();
        assert!(load_from(&path).is_err());
    }
}
