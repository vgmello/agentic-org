//! Host CLI runtime auto-detection for onboarding, setup, and operator diagnostics (`TASK-271`).
//!
//! Probes the canonical Top-6 CLIs ([`org_proto::runtimes::KNOWN_CLI_RUNTIMES`]: `claude`,
//! `codex`, `agy`, `gemini`, `kimi`, `pi`) across `PATH` and standard user-local / Homebrew bin
//! directories without invoking any external binary.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};

use org_proto::runtimes::{known_cli_runtime, DetectedCliRuntime, KNOWN_CLI_RUNTIMES};

/// Errors resolving a single requested host runtime binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeDiscoveryError {
    /// Requested runtime identifier or alias is not recognized.
    UnknownRuntime(String),
    /// Runtime is known, but its CLI executable is not installed on the search path.
    BinaryNotFound {
        runtime_id: String,
        binary_name: String,
    },
}

impl fmt::Display for RuntimeDiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownRuntime(id) => write!(
                f,
                "unknown agent runtime `{id}` (supported: claude, codex, agy, gemini, kimi, pi)"
            ),
            Self::BinaryNotFound {
                runtime_id,
                binary_name,
            } => write!(
                f,
                "runtime `{runtime_id}` executable `{binary_name}` not found on PATH or standard user bin directories"
            ),
        }
    }
}

impl std::error::Error for RuntimeDiscoveryError {}

/// Auto-detects all Top-6 supported CLI runtimes on the current host.
pub fn detect_host_runtimes(search_path: Option<&str>) -> Vec<DetectedCliRuntime> {
    let env_path = std::env::var_os("PATH");
    let home = std::env::var_os("HOME").map(PathBuf::from);
    detect_host_runtimes_from(search_path, env_path.as_deref(), home.as_deref())
}

/// Pure testable variant of [`detect_host_runtimes`] with explicit `PATH` and `HOME` inputs.
pub fn detect_host_runtimes_from(
    search_path: Option<&str>,
    env_path: Option<&OsStr>,
    home: Option<&Path>,
) -> Vec<DetectedCliRuntime> {
    KNOWN_CLI_RUNTIMES
        .iter()
        .map(|rt| {
            let found = find_binary_on_host(rt.binary_name, search_path, env_path, home);
            DetectedCliRuntime {
                runtime_id: rt.id.to_owned(),
                display_name: rt.display_name.to_owned(),
                binary_name: rt.binary_name.to_owned(),
                protocol: rt.protocol.to_owned(),
                provider_type: rt.provider_type.to_owned(),
                installed: found.is_some(),
                binary_path: found.map(|p| p.display().to_string()),
            }
        })
        .collect()
}

/// Resolves a specific runtime ID or alias to its canonical executable path, failing closed when
/// unknown or missing.
pub fn resolve_host_runtime_binary(
    runtime_id: &str,
    search_path: Option<&str>,
) -> Result<PathBuf, RuntimeDiscoveryError> {
    let env_path = std::env::var_os("PATH");
    let home = std::env::var_os("HOME").map(PathBuf::from);
    resolve_host_runtime_binary_from(
        runtime_id,
        search_path,
        env_path.as_deref(),
        home.as_deref(),
    )
}

/// Pure testable variant of [`resolve_host_runtime_binary`].
pub fn resolve_host_runtime_binary_from(
    runtime_id: &str,
    search_path: Option<&str>,
    env_path: Option<&OsStr>,
    home: Option<&Path>,
) -> Result<PathBuf, RuntimeDiscoveryError> {
    let Some(rt) = known_cli_runtime(runtime_id) else {
        return Err(RuntimeDiscoveryError::UnknownRuntime(runtime_id.to_owned()));
    };
    find_binary_on_host(rt.binary_name, search_path, env_path, home).ok_or_else(|| {
        RuntimeDiscoveryError::BinaryNotFound {
            runtime_id: rt.id.to_owned(),
            binary_name: rt.binary_name.to_owned(),
        }
    })
}

/// Formats a concise one-line onboarding/diagnostics summary of detected CLI runtimes.
pub fn format_detected_runtimes_summary(detected: &[DetectedCliRuntime]) -> String {
    let installed: Vec<&str> = detected
        .iter()
        .filter(|d| d.installed)
        .map(|d| d.runtime_id.as_str())
        .collect();
    if installed.is_empty() {
        "none detected (install claude, codex, agy, gemini, kimi, or pi-acp)".to_owned()
    } else {
        format!(
            "{}/{} installed ({})",
            installed.len(),
            detected.len(),
            installed.join(", ")
        )
    }
}

fn fallback_user_bin_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home {
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".bun/bin"));
        dirs.push(home.join(".cargo/bin"));
        dirs.push(home.join(".npm-global/bin"));
    }
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs
}

fn find_in_dirs(dirs: impl IntoIterator<Item = PathBuf>, binary: &str) -> Option<PathBuf> {
    dirs.into_iter()
        .map(|dir| dir.join(binary))
        .filter(|candidate| candidate.is_file())
        .find_map(|candidate| candidate.canonicalize().ok().filter(|p| p.is_file()))
}

fn find_binary_on_host(
    binary: &str,
    explicit_search_path: Option<&str>,
    env_path: Option<&OsStr>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    let primary: Vec<PathBuf> = match explicit_search_path {
        Some(raw) => std::env::split_paths(raw).collect(),
        None => env_path
            .map(std::env::split_paths)
            .into_iter()
            .flatten()
            .collect(),
    };
    if let Some(found) = find_in_dirs(primary, binary) {
        return Some(found);
    }
    if explicit_search_path.is_none() {
        return find_in_dirs(fallback_user_bin_dirs(home), binary);
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn detects_all_top_six_cli_runtimes_and_resolves_aliases() {
        let dir = tempfile::tempdir().unwrap();
        let path_bin = dir.path().join("bin");
        let home = dir.path().join("home");
        let local_bin = home.join(".local/bin");
        fs::create_dir_all(&path_bin).unwrap();
        fs::create_dir_all(&local_bin).unwrap();

        for bin in ["claude", "codex", "agy", "gemini"] {
            fs::write(path_bin.join(bin), b"#!/bin/sh\n").unwrap();
        }
        // kimi and pi-acp live in ~/.local/bin to exercise fallback discovery.
        for bin in ["kimi", "pi-acp"] {
            fs::write(local_bin.join(bin), b"#!/bin/sh\n").unwrap();
        }

        let detected = detect_host_runtimes_from(None, Some(path_bin.as_os_str()), Some(&home));
        assert_eq!(detected.len(), 6);
        assert!(detected
            .iter()
            .all(|d| d.installed && d.binary_path.is_some()));
        assert_eq!(
            format_detected_runtimes_summary(&detected),
            "6/6 installed (claude, codex, agy, gemini, kimi, pi)"
        );

        let pi_path = resolve_host_runtime_binary_from(
            "pi-coding-agent",
            None,
            Some(path_bin.as_os_str()),
            Some(&home),
        )
        .expect("pi alias resolves");
        assert_eq!(pi_path, local_bin.join("pi-acp").canonicalize().unwrap());

        let gemini_path = resolve_host_runtime_binary_from(
            "gemini-cli",
            None,
            Some(path_bin.as_os_str()),
            Some(&home),
        )
        .expect("gemini alias resolves");
        assert_eq!(gemini_path, path_bin.join("gemini").canonicalize().unwrap());
    }

    #[test]
    fn degrades_cleanly_when_binaries_are_missing_and_refuses_unknown_or_dir_entries() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        // A directory named `gemini` must NEVER be mistaken for an executable.
        fs::create_dir_all(bin_dir.join("gemini")).unwrap();
        // Only install `kimi` inside the explicit search path.
        fs::write(bin_dir.join("kimi"), b"#!/bin/sh\n").unwrap();

        let search = bin_dir.to_str().unwrap();
        let detected = detect_host_runtimes_from(Some(search), None, None);
        let installed: Vec<&str> = detected
            .iter()
            .filter(|d| d.installed)
            .map(|d| d.runtime_id.as_str())
            .collect();
        assert_eq!(installed, vec!["kimi"]);
        assert_eq!(
            format_detected_runtimes_summary(&detected),
            "1/6 installed (kimi)"
        );

        assert!(matches!(
            resolve_host_runtime_binary_from("gemini", Some(search), None, None),
            Err(RuntimeDiscoveryError::BinaryNotFound {
                runtime_id,
                binary_name,
            }) if runtime_id == "gemini" && binary_name == "gemini"
        ));
        assert!(matches!(
            resolve_host_runtime_binary_from("nonexistent", Some(search), None, None),
            Err(RuntimeDiscoveryError::UnknownRuntime(_))
        ));

        let empty_dir = dir.path().join("empty");
        fs::create_dir_all(&empty_dir).unwrap();
        let none = detect_host_runtimes_from(Some(empty_dir.to_str().unwrap()), None, None);
        assert!(none.iter().all(|d| !d.installed));
        assert!(format_detected_runtimes_summary(&none).contains("none detected"));
    }
}
