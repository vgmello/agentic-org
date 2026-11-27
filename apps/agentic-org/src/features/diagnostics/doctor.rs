//! `agentic-org doctor`: read-only host, runtime, security, platform, and organization-state
//! integrity diagnostics with actionable remediation hints and non-zero exit on failure.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::{Cell, ListView, Row, Table};
use org_cli_shell::terminal::sanitize;
use serde::Serialize;

use super::doctor_host::{
    check_credentials_dir, check_instance_layout, check_permissions, check_registry_consistency,
    check_release_pin, check_sockets,
};
pub(crate) use super::doctor_types::{CheckStatus, DoctorCheck};
use super::org_integrity_checks::check_org_state;
use super::paths::{
    discover_offices, normalize_lstat_path, read_regular_bounded, resolve_instance,
    DiscoveredOffice,
};
use super::DoctorArgs;
use crate::error::CliError;
use crate::selection;

const APPARMOR_USERNS_SYSCTL: &str = "/proc/sys/kernel/apparmor_restrict_unprivileged_userns";
const APPARMOR_PROFILES_DIR: &str = "/etc/apparmor.d";
const APPARMOR_LOADED_PROFILES: &str = "/sys/kernel/security/apparmor/profiles";
const SCOPED_USERNS_PROFILE: &str = "agentic-org-bwrap";
const RESTRICT_USERNS_PROFILE: &str = "bwrap-userns-restrict";
const RESTRICT_CHILD_PROFILE: &str = "unpriv_bwrap";
const MAX_SYSCTL_BYTES: u64 = 1024;
const MAX_APPARMOR_PROFILE_BYTES: u64 = 64 * 1024;
const MAX_LOADED_PROFILES_BYTES: u64 = 512 * 1024;
const APPARMOR_USERNS_REMEDIATION: &str = "Install the minimal scoped AppArmor profile at /etc/apparmor.d/agentic-org-bwrap via `sudo bash scripts/install.sh --install-bwrap-apparmor` (keep kernel.apparmor_restrict_unprivileged_userns enabled globally).";

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DoctorSummary {
    pub(crate) pass: usize,
    pub(crate) warn: usize,
    pub(crate) fail: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DoctorReport {
    pub(crate) instance: String,
    pub(crate) ok: bool,
    pub(crate) summary: DoctorSummary,
    pub(crate) checks: Vec<DoctorCheck>,
}

pub(crate) fn run(args: DoctorArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let explicit_instance = args.instance.name.is_some();
    let target = selection::select(args.instance.name.as_deref(), args.office.as_deref())?;
    let run_instance = args.org_dir.is_none() || explicit_instance;

    let report = collect_doctor(
        target.instance(),
        args.office.as_deref(),
        args.org_dir.as_deref(),
        run_instance,
    )?;
    render_doctor(&report, ctx)?;
    if !report.ok {
        return Err(CliError::Exited(1));
    }
    Ok(())
}

/// Runs diagnostic checks and returns a structured report (also consumed by `support`).
pub(crate) fn collect_doctor(
    instance_name: Option<&str>,
    office_filter: Option<&str>,
    org_dir: Option<&Path>,
    include_instance_checks: bool,
) -> Result<DoctorReport, CliError> {
    let mut checks = Vec::new();
    let instance_label = instance_name.unwrap_or("default").to_owned();

    if include_instance_checks {
        let paths = resolve_instance(instance_name)?;
        check_instance_layout(&paths, &mut checks);
        check_permissions(&paths, &mut checks);
        check_registry_consistency(&paths, &mut checks);
        check_sockets(&paths, &mut checks);
        check_release_pin(&mut checks);
        check_credentials_dir(&paths, &mut checks);
        let offices = discover_offices(&paths, office_filter);
        check_discovered_offices(&offices, &mut checks);
    }

    if let Some(dir) = org_dir {
        checks.extend(check_org_state(dir, None, "org.integrity"));
    }

    if let Some(check) = check_linux_apparmor_userns() {
        checks.push(check);
    }

    let summary = DoctorSummary {
        pass: checks
            .iter()
            .filter(|c| c.status == CheckStatus::Pass)
            .count(),
        warn: checks
            .iter()
            .filter(|c| c.status == CheckStatus::Warn)
            .count(),
        fail: checks
            .iter()
            .filter(|c| c.status == CheckStatus::Fail)
            .count(),
    };
    Ok(DoctorReport {
        instance: instance_label,
        ok: summary.fail == 0,
        summary,
        checks,
    })
}

fn check_discovered_offices(offices: &[DiscoveredOffice], checks: &mut Vec<DoctorCheck>) {
    for office in offices {
        let Some(state_dir) = &office.durable_state else {
            continue;
        };
        let scope = format!("org.integrity.{}", office.office_id);
        checks.extend(check_org_state(
            state_dir,
            office.office_db.as_deref(),
            &scope,
        ));
    }
}

/// Safe read-only Linux AppArmor/userns diagnostic with zero platform `#[cfg]`.
fn check_linux_apparmor_userns() -> Option<DoctorCheck> {
    check_linux_apparmor_userns_at(
        Path::new(APPARMOR_USERNS_SYSCTL),
        Path::new(APPARMOR_PROFILES_DIR),
        Some(Path::new(APPARMOR_LOADED_PROFILES)),
    )
}

fn legacy_userns_profile_name() -> &'static str {
    SCOPED_USERNS_PROFILE
        .strip_prefix("agentic-org-")
        .unwrap_or(SCOPED_USERNS_PROFILE)
}

#[derive(Debug, Default)]
struct AppArmorDirState {
    active_scoped: Option<(&'static str, PathBuf)>,
    disabled_scoped: Option<PathBuf>,
    invalid_scoped: Option<PathBuf>,
    restrict_active: Option<PathBuf>,
}

#[derive(Debug, Default)]
struct LoadedAppArmorState {
    scoped_profile: Option<(String, String)>,
    restrict_child_loaded: bool,
    legacy_enforced_loaded: bool,
}

enum LoadedProfilesProbe {
    Skipped,
    Unreadable,
    Loaded(LoadedAppArmorState),
}

fn is_disabled_profile(apparmor_dir: &Path, profile_name: &str) -> bool {
    let disable_entry = apparmor_dir.join("disable").join(profile_name);
    fs::symlink_metadata(normalize_lstat_path(&disable_entry)).is_ok()
}

fn is_valid_scoped_userns_profile(content: &str, tool_name: &str) -> bool {
    let mut has_target = false;
    let mut has_userns = false;
    let mut denies_userns = false;
    let mut restricts_caps = false;

    for raw in content.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let first_word = line.split_whitespace().next();
        if first_word != Some("include")
            && line.contains(tool_name)
            && (first_word == Some("profile") || line.starts_with('/'))
        {
            has_target = true;
        }
        if line.contains(RESTRICT_CHILD_PROFILE) {
            restricts_caps = true;
        }
        let mut line_deny = false;
        let mut line_userns = false;
        let mut line_cap = false;
        for token in line.split_whitespace() {
            let clean = token.trim_end_matches([',', ';']);
            match clean {
                "deny" => line_deny = true,
                "userns" => line_userns = true,
                "capability" => line_cap = true,
                _ => {}
            }
        }
        if line_userns {
            if line_deny {
                denies_userns = true;
            } else {
                has_userns = true;
            }
        }
        if line_deny && line_cap {
            restricts_caps = true;
        }
    }

    has_target && has_userns && !denies_userns && !restricts_caps
}

fn inspect_apparmor_dir(apparmor_dir: &Path) -> AppArmorDirState {
    let mut state = AppArmorDirState::default();
    let tool_name = legacy_userns_profile_name();

    let restrict_path = apparmor_dir.join(RESTRICT_USERNS_PROFILE);
    if fs::symlink_metadata(normalize_lstat_path(&restrict_path)).is_ok()
        && !is_disabled_profile(apparmor_dir, RESTRICT_USERNS_PROFILE)
    {
        state.restrict_active = Some(restrict_path);
    }

    for name in [SCOPED_USERNS_PROFILE, tool_name] {
        let path = apparmor_dir.join(name);
        if fs::symlink_metadata(normalize_lstat_path(&path)).is_err() {
            continue;
        }
        if is_disabled_profile(apparmor_dir, name) {
            if state.disabled_scoped.is_none() {
                state.disabled_scoped = Some(path);
            }
            continue;
        }
        let valid = read_regular_bounded(&path, MAX_APPARMOR_PROFILE_BYTES)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .is_some_and(|text| is_valid_scoped_userns_profile(&text, tool_name));
        if valid {
            state.active_scoped = Some((name, path));
            break;
        }
        if state.invalid_scoped.is_none() {
            state.invalid_scoped = Some(path);
        }
    }

    state
}

fn parse_loaded_profile_line(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    let (name, rest) = trimmed.rsplit_once(" (")?;
    let mode = rest.strip_suffix(')')?;
    Some((name.trim(), mode.trim()))
}

fn probe_loaded_profiles(profiles_status_path: Option<&Path>) -> LoadedProfilesProbe {
    let Some(path) = profiles_status_path else {
        return LoadedProfilesProbe::Skipped;
    };
    let bytes = match read_regular_bounded(path, MAX_LOADED_PROFILES_BYTES) {
        Ok(bytes) => bytes,
        Err(_) => return LoadedProfilesProbe::Unreadable,
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return LoadedProfilesProbe::Unreadable;
    };

    let legacy_name = legacy_userns_profile_name();
    let mut loaded = LoadedAppArmorState::default();
    for line in text.lines() {
        let Some((name, mode)) = parse_loaded_profile_line(line) else {
            continue;
        };
        let recognized_mode = matches!(mode, "unconfined" | "complain" | "enforce");
        if name == SCOPED_USERNS_PROFILE && recognized_mode {
            loaded.scoped_profile = Some((name.to_owned(), mode.to_owned()));
        } else if name == RESTRICT_CHILD_PROFILE {
            loaded.restrict_child_loaded = true;
        } else if name == legacy_name && recognized_mode {
            if mode == "enforce" {
                loaded.legacy_enforced_loaded = true;
            } else if loaded.scoped_profile.is_none() {
                loaded.scoped_profile = Some((name.to_owned(), mode.to_owned()));
            }
        }
    }
    LoadedProfilesProbe::Loaded(loaded)
}

/// Pure injectable AppArmor userns diagnostic helper (read-only, bounded, symlink-safe).
pub(super) fn check_linux_apparmor_userns_at(
    sysctl_path: &Path,
    apparmor_dir: &Path,
    profiles_status_path: Option<&Path>,
) -> Option<DoctorCheck> {
    let raw_bytes = match read_regular_bounded(sysctl_path, MAX_SYSCTL_BYTES) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
        Err(e) => {
            return Some(DoctorCheck::warn(
                "platform.apparmor_userns",
                "platform",
                format!("could not read AppArmor userns sysctl: {e}"),
                None,
                Some(sysctl_path.display().to_string()),
            ));
        }
    };
    let Ok(raw) = String::from_utf8(raw_bytes) else {
        return Some(DoctorCheck::warn(
            "platform.apparmor_userns",
            "platform",
            "could not decode AppArmor userns sysctl as UTF-8",
            None,
            Some(sysctl_path.display().to_string()),
        ));
    };

    let value = raw.trim();
    if value == "0" {
        return Some(DoctorCheck::pass(
            "platform.apparmor_userns",
            "platform",
            "unprivileged user namespaces permitted (apparmor_restrict_unprivileged_userns=0)",
            Some(sysctl_path.display().to_string()),
        ));
    }
    if value != "1" {
        return Some(DoctorCheck::warn(
            "platform.apparmor_userns",
            "platform",
            format!("unexpected AppArmor userns sysctl value `{value}`"),
            Some(APPARMOR_USERNS_REMEDIATION.to_owned()),
            Some(sysctl_path.display().to_string()),
        ));
    }

    let dir_state = inspect_apparmor_dir(apparmor_dir);
    let legacy_name = legacy_userns_profile_name();

    if let Some((profile_name, profile_path)) = dir_state.active_scoped {
        if dir_state.restrict_active.is_some() {
            return Some(DoctorCheck::warn(
                "platform.apparmor_userns",
                "platform",
                format!(
                    "scoped bwrap AppArmor profile exists at {}, but conflicting `bwrap-userns-restrict` profile is still active (apparmor_restrict_unprivileged_userns=1)",
                    profile_path.display()
                ),
                Some(APPARMOR_USERNS_REMEDIATION.to_owned()),
                Some(profile_path.display().to_string()),
            ));
        }

        return match probe_loaded_profiles(profiles_status_path) {
            LoadedProfilesProbe::Skipped | LoadedProfilesProbe::Unreadable => {
                Some(DoctorCheck::pass(
                    "platform.apparmor_userns",
                    "platform",
                    format!(
                        "scoped bwrap AppArmor profile present at {} (apparmor_restrict_unprivileged_userns=1)",
                        profile_path.display()
                    ),
                    Some(profile_path.display().to_string()),
                ))
            }
            LoadedProfilesProbe::Loaded(loaded) => {
                if loaded.restrict_child_loaded {
                    Some(DoctorCheck::warn(
                        "platform.apparmor_userns",
                        "platform",
                        format!(
                            "scoped bwrap AppArmor profile present at {}, but `bwrap-userns-restrict` (`unpriv_bwrap`) is still loaded in kernel AppArmor profiles (apparmor_restrict_unprivileged_userns=1)",
                            profile_path.display()
                        ),
                        Some(APPARMOR_USERNS_REMEDIATION.to_owned()),
                        Some(profile_path.display().to_string()),
                    ))
                } else if let Some((loaded_name, loaded_mode)) = loaded.scoped_profile {
                    Some(DoctorCheck::pass(
                        "platform.apparmor_userns",
                        "platform",
                        format!(
                            "scoped bwrap AppArmor profile `{loaded_name}` loaded ({loaded_mode}) from {} (apparmor_restrict_unprivileged_userns=1)",
                            profile_path.display()
                        ),
                        Some(profile_path.display().to_string()),
                    ))
                } else if loaded.legacy_enforced_loaded && profile_name == legacy_name {
                    Some(DoctorCheck::pass(
                        "platform.apparmor_userns",
                        "platform",
                        format!(
                            "scoped bwrap AppArmor profile `{profile_name}` loaded (enforce) from {} (apparmor_restrict_unprivileged_userns=1)",
                            profile_path.display()
                        ),
                        Some(profile_path.display().to_string()),
                    ))
                } else {
                    Some(DoctorCheck::warn(
                        "platform.apparmor_userns",
                        "platform",
                        format!(
                            "scoped bwrap AppArmor profile present at {} but not loaded in kernel AppArmor profiles (apparmor_restrict_unprivileged_userns=1)",
                            profile_path.display()
                        ),
                        Some(format!(
                            "Load the profile with `sudo apparmor_parser -r {}` or run `sudo bash scripts/install.sh --install-bwrap-apparmor`.",
                            profile_path.display()
                        )),
                        Some(profile_path.display().to_string()),
                    ))
                }
            }
        };
    }

    if let Some(disabled_path) = dir_state.disabled_scoped {
        return Some(DoctorCheck::warn(
            "platform.apparmor_userns",
            "platform",
            format!(
                "scoped bwrap AppArmor profile at {} is disabled via disable/ symlink (apparmor_restrict_unprivileged_userns=1)",
                disabled_path.display()
            ),
            Some(APPARMOR_USERNS_REMEDIATION.to_owned()),
            Some(disabled_path.display().to_string()),
        ));
    }

    if let Some(restrict_path) = dir_state.restrict_active {
        return Some(DoctorCheck::warn(
            "platform.apparmor_userns",
            "platform",
            "AppArmor restricts unprivileged user namespaces (apparmor_restrict_unprivileged_userns=1) and only the capability-restricting `bwrap-userns-restrict` profile is active",
            Some(APPARMOR_USERNS_REMEDIATION.to_owned()),
            Some(restrict_path.display().to_string()),
        ));
    }

    if let Some(bad_path) = dir_state.invalid_scoped {
        return Some(DoctorCheck::warn(
            "platform.apparmor_userns",
            "platform",
            format!(
                "AppArmor profile at {} does not permit scoped bwrap user namespaces (apparmor_restrict_unprivileged_userns=1)",
                bad_path.display()
            ),
            Some(APPARMOR_USERNS_REMEDIATION.to_owned()),
            Some(bad_path.display().to_string()),
        ));
    }

    Some(DoctorCheck::warn(
        "platform.apparmor_userns",
        "platform",
        "AppArmor restricts unprivileged user namespaces (apparmor_restrict_unprivileged_userns=1) and no scoped bwrap AppArmor profile is installed",
        Some(APPARMOR_USERNS_REMEDIATION.to_owned()),
        Some(sysctl_path.display().to_string()),
    ))
}

fn render_doctor(report: &DoctorReport, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    if ctx.modes().json {
        return Ok(ctx.write_json(report)?);
    }
    if ctx.modes().quiet {
        let mut out = String::new();
        for check in &report.checks {
            if check.status == CheckStatus::Fail {
                out.push_str(&sanitize(&check.id));
                out.push('\n');
            }
        }
        return if out.is_empty() {
            Ok(())
        } else {
            Ok(ctx.out(&out)?)
        };
    }
    let rows: Vec<Row> = report
        .checks
        .iter()
        .map(|check| {
            let mut detail = check.message.clone();
            if let Some(hint) = &check.remediation {
                detail.push_str(" — fix: ");
                detail.push_str(hint);
            }
            Row {
                id: check.id.clone(),
                cells: vec![
                    Cell::status(check.status.as_str(), check.status.style()),
                    Cell::id(&check.id),
                    Cell::plain(detail),
                ],
            }
        })
        .collect();
    let table = Table {
        headers: vec!["STATUS".into(), "CHECK".into(), "DETAILS".into()],
        rows,
    };
    let json =
        serde_json::to_value(report).map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    let view = ListView {
        empty: "No diagnostic checks ran.",
        total: None,
    };
    ctx.render_table(&table, &json, &view)?;
    Ok(())
}
