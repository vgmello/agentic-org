//! Read-only Linux AppArmor unprivileged user-namespace diagnostics for `agentic-org doctor`.
//!
//! Leaf sibling module of `diagnostics` (`doctor -> doctor_apparmor -> {doctor_types, paths}`)
//! with zero platform `#[cfg]` conditionals and bounded, symlink-safe reads.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use org_cli_shell::terminal::sanitize;

use super::doctor_types::DoctorCheck;
use super::paths::{normalize_lstat_path, read_regular_bounded};

const CHECK_ID: &str = "platform.apparmor_userns";
const CHECK_CATEGORY: &str = "platform";
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

/// Safe read-only Linux AppArmor/userns diagnostic with zero platform `#[cfg]`.
pub(super) fn check_linux_apparmor_userns() -> Option<DoctorCheck> {
    check_linux_apparmor_userns_at(
        Path::new(APPARMOR_USERNS_SYSCTL),
        Path::new(APPARMOR_PROFILES_DIR),
        Some(Path::new(APPARMOR_LOADED_PROFILES)),
    )
}

/// Pure injectable AppArmor userns diagnostic helper (read-only, bounded, symlink-safe).
pub(super) fn check_linux_apparmor_userns_at(
    sysctl_path: &Path,
    apparmor_dir: &Path,
    profiles_status_path: Option<&Path>,
) -> Option<DoctorCheck> {
    match read_userns_sysctl(sysctl_path) {
        SysctlState::Absent => None,
        SysctlState::Unrestricted => Some(apparmor_pass(
            "unprivileged user namespaces permitted (apparmor_restrict_unprivileged_userns=0)",
            sysctl_path,
        )),
        SysctlState::Warning(check) => Some(check),
        SysctlState::Restricted => {
            let dir_state = inspect_apparmor_dir(apparmor_dir);
            Some(evaluate_dir_state(
                sysctl_path,
                dir_state,
                profiles_status_path,
            ))
        }
    }
}

fn legacy_userns_profile_name() -> &'static str {
    SCOPED_USERNS_PROFILE
        .strip_prefix("agentic-org-")
        .unwrap_or(SCOPED_USERNS_PROFILE)
}

fn apparmor_pass(message: impl Into<String>, path: &Path) -> DoctorCheck {
    DoctorCheck::pass(
        CHECK_ID,
        CHECK_CATEGORY,
        message,
        Some(path.display().to_string()),
    )
}

fn apparmor_warn(
    message: impl Into<String>,
    remediation: Option<String>,
    path: &Path,
) -> DoctorCheck {
    DoctorCheck::warn(
        CHECK_ID,
        CHECK_CATEGORY,
        message,
        remediation,
        Some(path.display().to_string()),
    )
}

enum SysctlState {
    Absent,
    Unrestricted,
    Restricted,
    Warning(DoctorCheck),
}

fn parse_sysctl_value(raw: &str, sysctl_path: &Path) -> SysctlState {
    match raw.trim() {
        "0" => SysctlState::Unrestricted,
        "1" => SysctlState::Restricted,
        value => {
            let safe_value = sanitize(value);
            SysctlState::Warning(apparmor_warn(
                format!("unexpected AppArmor userns sysctl value `{safe_value}`"),
                Some(APPARMOR_USERNS_REMEDIATION.to_owned()),
                sysctl_path,
            ))
        }
    }
}

fn read_userns_sysctl(sysctl_path: &Path) -> SysctlState {
    let raw_bytes = match read_regular_bounded(sysctl_path, MAX_SYSCTL_BYTES) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return SysctlState::Absent,
        Err(e) => {
            return SysctlState::Warning(apparmor_warn(
                format!("could not read AppArmor userns sysctl: {e}"),
                None,
                sysctl_path,
            ));
        }
    };
    let Ok(raw) = String::from_utf8(raw_bytes) else {
        return SysctlState::Warning(apparmor_warn(
            "could not decode AppArmor userns sysctl as UTF-8",
            None,
            sysctl_path,
        ));
    };
    parse_sysctl_value(&raw, sysctl_path)
}

#[derive(Debug, Default)]
struct ProfileScan {
    has_unconfined_target: bool,
    has_confined_target: bool,
    has_userns: bool,
    denies_userns: bool,
    restricts_caps: bool,
}

impl ProfileScan {
    fn is_valid(&self) -> bool {
        self.has_unconfined_target
            && !self.has_confined_target
            && self.has_userns
            && !self.denies_userns
            && !self.restricts_caps
    }
}

#[derive(Debug)]
enum ProfileEntryState {
    Absent,
    Disabled(PathBuf),
    Valid(PathBuf),
    Invalid(PathBuf),
}

#[derive(Debug)]
struct AppArmorDirState {
    scoped: ProfileEntryState,
    legacy: ProfileEntryState,
    restrict_active: Option<PathBuf>,
}

impl AppArmorDirState {
    fn first_disabled(&self) -> Option<&Path> {
        match (&self.scoped, &self.legacy) {
            (ProfileEntryState::Disabled(p), _) | (_, ProfileEntryState::Disabled(p)) => {
                Some(p.as_path())
            }
            _ => None,
        }
    }

    fn first_invalid(&self) -> Option<&Path> {
        match (&self.scoped, &self.legacy) {
            (ProfileEntryState::Invalid(p), _) | (_, ProfileEntryState::Invalid(p)) => {
                Some(p.as_path())
            }
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
struct LoadedAppArmorState {
    scoped_profile: Option<(String, String)>,
    legacy_profile: Option<(String, String)>,
    restrict_child_loaded: bool,
    legacy_enforced_loaded: bool,
    scoped_enforced_loaded: bool,
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

fn is_header_option_token(tok: &str) -> bool {
    tok == "{" || tok.starts_with('(') || tok.contains('=')
}

fn header_attachment_token(line: &str) -> Option<&str> {
    if line.ends_with([',', ';']) {
        return None;
    }
    let mut pos = line
        .split_whitespace()
        .take_while(|tok| !is_header_option_token(tok))
        .map(|tok| tok.trim_end_matches('{'));
    let first = pos.next()?;
    if first == "profile" {
        let second = pos.next()?;
        let third = pos.next();
        if pos.next().is_some() {
            return None;
        }
        return third.or(Some(second));
    }
    pos.next().is_none().then_some(first)
}

fn attachment_targets_tool(token: &str, tool_name: &str) -> bool {
    let unquoted = token
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(token);
    unquoted.starts_with('/')
        && !unquoted.contains("..")
        && unquoted
            .strip_suffix(tool_name)
            .is_some_and(|prefix| prefix.ends_with('/'))
}

fn header_has_unconfined_flag(line: &str) -> bool {
    let mut rest = line.trim_end().trim_end_matches('{');
    while let Some(open) = rest.find('(') {
        let prefix = rest[..open].trim_end();
        let after = &rest[open + 1..];
        let Some(close) = after.find(')') else {
            break;
        };
        let is_flag_list = prefix
            .strip_suffix('=')
            .is_none_or(|key| key.split_whitespace().last() == Some("flags"));
        let inner = &after[..close];
        if is_flag_list && inner.split([',', ' ', '\t']).any(|f| f == "unconfined") {
            return true;
        }
        rest = &after[close + 1..];
    }
    false
}

fn scan_profile_header(line: &str, tool_name: &str, scan: &mut ProfileScan) {
    let Some(attachment) = header_attachment_token(line) else {
        return;
    };
    if !attachment_targets_tool(attachment, tool_name) {
        return;
    }
    if header_has_unconfined_flag(line) {
        scan.has_unconfined_target = true;
    } else {
        scan.has_confined_target = true;
    }
}

fn scan_rule_tokens(line: &str, scan: &mut ProfileScan) {
    let mut has_deny = false;
    let mut has_userns = false;
    let mut has_cap = false;
    for token in line.split_whitespace() {
        let clean = token.trim_end_matches([',', ';']);
        has_deny |= clean == "deny";
        has_userns |= clean == "userns";
        has_cap |= clean == "capability";
    }
    scan.denies_userns |= has_userns && has_deny;
    scan.has_userns |= has_userns && !has_deny;
    scan.restricts_caps |= has_deny && has_cap;
}

fn scan_profile_line(line: &str, tool_name: &str, scan: &mut ProfileScan) {
    scan_profile_header(line, tool_name, scan);
    if line.contains(RESTRICT_CHILD_PROFILE) {
        scan.restricts_caps = true;
    }
    scan_rule_tokens(line, scan);
}

fn is_valid_scoped_userns_profile(content: &str, tool_name: &str) -> bool {
    let mut scan = ProfileScan::default();
    for raw in content.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if !line.is_empty() {
            scan_profile_line(line, tool_name, &mut scan);
        }
    }
    scan.is_valid()
}

fn inspect_profile_entry(apparmor_dir: &Path, name: &str, tool_name: &str) -> ProfileEntryState {
    let path = apparmor_dir.join(name);
    if fs::symlink_metadata(normalize_lstat_path(&path)).is_err() {
        return ProfileEntryState::Absent;
    }
    if is_disabled_profile(apparmor_dir, name) {
        return ProfileEntryState::Disabled(path);
    }
    let valid = read_regular_bounded(&path, MAX_APPARMOR_PROFILE_BYTES)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .is_some_and(|text| is_valid_scoped_userns_profile(&text, tool_name));
    if valid {
        ProfileEntryState::Valid(path)
    } else {
        ProfileEntryState::Invalid(path)
    }
}

fn inspect_apparmor_dir(apparmor_dir: &Path) -> AppArmorDirState {
    let tool_name = legacy_userns_profile_name();
    let restrict_path = apparmor_dir.join(RESTRICT_USERNS_PROFILE);
    let restrict_active = (fs::symlink_metadata(normalize_lstat_path(&restrict_path)).is_ok()
        && !is_disabled_profile(apparmor_dir, RESTRICT_USERNS_PROFILE))
    .then_some(restrict_path);

    AppArmorDirState {
        scoped: inspect_profile_entry(apparmor_dir, SCOPED_USERNS_PROFILE, tool_name),
        legacy: inspect_profile_entry(apparmor_dir, tool_name, tool_name),
        restrict_active,
    }
}

fn parse_loaded_profile_line(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    let (name, rest) = trimmed.rsplit_once(" (")?;
    let mode = rest.strip_suffix(')')?;
    Some((name.trim(), mode.trim()))
}

fn is_restrict_child_profile(name: &str) -> bool {
    name == RESTRICT_CHILD_PROFILE
        || name
            .strip_suffix(RESTRICT_CHILD_PROFILE)
            .is_some_and(|prefix| prefix.ends_with("//"))
}

fn record_profile_mode(
    allowed: &mut Option<(String, String)>,
    enforced: &mut bool,
    name: &str,
    mode: &str,
) {
    match mode {
        "unconfined" | "complain" => *allowed = Some((name.to_owned(), mode.to_owned())),
        "enforce" => *enforced = true,
        _ => {}
    }
}

fn record_loaded_profile(
    loaded: &mut LoadedAppArmorState,
    name: &str,
    mode: &str,
    legacy_name: &str,
) {
    if is_restrict_child_profile(name) {
        loaded.restrict_child_loaded = true;
    } else if name == SCOPED_USERNS_PROFILE {
        record_profile_mode(
            &mut loaded.scoped_profile,
            &mut loaded.scoped_enforced_loaded,
            name,
            mode,
        );
    } else if name == legacy_name {
        record_profile_mode(
            &mut loaded.legacy_profile,
            &mut loaded.legacy_enforced_loaded,
            name,
            mode,
        );
    }
}

fn probe_loaded_profiles(profiles_status_path: Option<&Path>) -> LoadedProfilesProbe {
    let Some(path) = profiles_status_path else {
        return LoadedProfilesProbe::Skipped;
    };
    let Ok(bytes) = read_regular_bounded(path, MAX_LOADED_PROFILES_BYTES) else {
        return LoadedProfilesProbe::Unreadable;
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return LoadedProfilesProbe::Unreadable;
    };

    let legacy_name = legacy_userns_profile_name();
    let mut loaded = LoadedAppArmorState::default();
    for line in text.lines() {
        if let Some((name, mode)) = parse_loaded_profile_line(line) {
            record_loaded_profile(&mut loaded, name, mode, legacy_name);
        }
    }
    LoadedProfilesProbe::Loaded(loaded)
}

fn evaluate_loaded_conflict(
    profile_path: &Path,
    loaded: &LoadedAppArmorState,
) -> Option<DoctorCheck> {
    if loaded.restrict_child_loaded {
        return Some(apparmor_warn(
            format!(
                "scoped bwrap AppArmor profile present at {}, but `{RESTRICT_USERNS_PROFILE}` (`{RESTRICT_CHILD_PROFILE}`) is still loaded in kernel AppArmor profiles (apparmor_restrict_unprivileged_userns=1)",
                profile_path.display()
            ),
            Some(APPARMOR_USERNS_REMEDIATION.to_owned()),
            profile_path,
        ));
    }
    if loaded.legacy_enforced_loaded {
        let legacy_name = legacy_userns_profile_name();
        return Some(apparmor_warn(
            format!(
                "scoped bwrap AppArmor profile present at {}, but conflicting enforcing `{legacy_name}` profile is loaded in kernel AppArmor profiles and shadows it (apparmor_restrict_unprivileged_userns=1)",
                profile_path.display()
            ),
            Some(APPARMOR_USERNS_REMEDIATION.to_owned()),
            profile_path,
        ));
    }
    if loaded.scoped_enforced_loaded {
        return Some(apparmor_warn(
            format!(
                "scoped bwrap AppArmor profile `{SCOPED_USERNS_PROFILE}` is loaded in enforce mode instead of unconfined (apparmor_restrict_unprivileged_userns=1)"
            ),
            Some(format!(
                "Reload the unconfined profile with `sudo apparmor_parser -r {}` or run `sudo bash scripts/install.sh --install-bwrap-apparmor`.",
                profile_path.display()
            )),
            profile_path,
        ));
    }
    None
}

fn evaluate_loaded_state(profile_path: &Path, loaded: LoadedAppArmorState) -> DoctorCheck {
    if let Some(conflict) = evaluate_loaded_conflict(profile_path, &loaded) {
        return conflict;
    }
    if let Some((loaded_name, loaded_mode)) = loaded.scoped_profile.or(loaded.legacy_profile) {
        return apparmor_pass(
            format!(
                "scoped bwrap AppArmor profile `{loaded_name}` loaded ({loaded_mode}) from {} (apparmor_restrict_unprivileged_userns=1)",
                profile_path.display()
            ),
            profile_path,
        );
    }
    apparmor_warn(
        format!(
            "scoped bwrap AppArmor profile present at {} but not loaded in kernel AppArmor profiles (apparmor_restrict_unprivileged_userns=1)",
            profile_path.display()
        ),
        Some(format!(
            "Load the profile with `sudo apparmor_parser -r {}` or run `sudo bash scripts/install.sh --install-bwrap-apparmor`.",
            profile_path.display()
        )),
        profile_path,
    )
}

fn evaluate_loaded_profiles(
    profile_path: &Path,
    profiles_status_path: Option<&Path>,
) -> DoctorCheck {
    match probe_loaded_profiles(profiles_status_path) {
        LoadedProfilesProbe::Skipped | LoadedProfilesProbe::Unreadable => apparmor_pass(
            format!(
                "scoped bwrap AppArmor profile present at {} (apparmor_restrict_unprivileged_userns=1)",
                profile_path.display()
            ),
            profile_path,
        ),
        LoadedProfilesProbe::Loaded(loaded) => evaluate_loaded_state(profile_path, loaded),
    }
}

fn evaluate_active_profile(
    profile_path: &Path,
    restrict_active: Option<&Path>,
    shadowing_legacy: Option<&Path>,
    profiles_status_path: Option<&Path>,
) -> DoctorCheck {
    if restrict_active.is_some() {
        return apparmor_warn(
            format!(
                "scoped bwrap AppArmor profile exists at {}, but conflicting `{RESTRICT_USERNS_PROFILE}` profile is still active (apparmor_restrict_unprivileged_userns=1)",
                profile_path.display()
            ),
            Some(APPARMOR_USERNS_REMEDIATION.to_owned()),
            profile_path,
        );
    }
    if let Some(legacy_path) = shadowing_legacy {
        return apparmor_warn(
            format!(
                "scoped bwrap AppArmor profile exists at {}, but active conflicting profile at {} shadows it (apparmor_restrict_unprivileged_userns=1)",
                profile_path.display(),
                legacy_path.display()
            ),
            Some(format!(
                "Disable or replace the conflicting profile at {} and keep {} loaded (keep kernel.apparmor_restrict_unprivileged_userns enabled globally).",
                legacy_path.display(),
                profile_path.display()
            )),
            legacy_path,
        );
    }
    evaluate_loaded_profiles(profile_path, profiles_status_path)
}

fn evaluate_missing_profile(sysctl_path: &Path, dir_state: AppArmorDirState) -> DoctorCheck {
    if let Some(disabled_path) = dir_state.first_disabled() {
        return apparmor_warn(
            format!(
                "scoped bwrap AppArmor profile at {} is disabled via disable/ symlink (apparmor_restrict_unprivileged_userns=1)",
                disabled_path.display()
            ),
            Some(APPARMOR_USERNS_REMEDIATION.to_owned()),
            disabled_path,
        );
    }
    if let Some(restrict_path) = &dir_state.restrict_active {
        return apparmor_warn(
            "AppArmor restricts unprivileged user namespaces (apparmor_restrict_unprivileged_userns=1) and only the capability-restricting `bwrap-userns-restrict` profile is active",
            Some(APPARMOR_USERNS_REMEDIATION.to_owned()),
            restrict_path,
        );
    }
    if let Some(bad_path) = dir_state.first_invalid() {
        return apparmor_warn(
            format!(
                "AppArmor profile at {} does not permit scoped bwrap user namespaces (apparmor_restrict_unprivileged_userns=1)",
                bad_path.display()
            ),
            Some(APPARMOR_USERNS_REMEDIATION.to_owned()),
            bad_path,
        );
    }
    apparmor_warn(
        "AppArmor restricts unprivileged user namespaces (apparmor_restrict_unprivileged_userns=1) and no scoped bwrap AppArmor profile is installed",
        Some(APPARMOR_USERNS_REMEDIATION.to_owned()),
        sysctl_path,
    )
}

fn evaluate_dir_state(
    sysctl_path: &Path,
    dir_state: AppArmorDirState,
    profiles_status_path: Option<&Path>,
) -> DoctorCheck {
    if let ProfileEntryState::Valid(ref scoped_path) = dir_state.scoped {
        let shadowing_legacy = match &dir_state.legacy {
            ProfileEntryState::Invalid(path) => Some(path.as_path()),
            _ => None,
        };
        return evaluate_active_profile(
            scoped_path,
            dir_state.restrict_active.as_deref(),
            shadowing_legacy,
            profiles_status_path,
        );
    }
    if let ProfileEntryState::Valid(ref legacy_path) = dir_state.legacy {
        return evaluate_active_profile(
            legacy_path,
            dir_state.restrict_active.as_deref(),
            None,
            profiles_status_path,
        );
    }
    evaluate_missing_profile(sysctl_path, dir_state)
}
