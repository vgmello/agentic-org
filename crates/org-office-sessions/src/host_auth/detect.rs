//! Host runtime CLI auto-detection backed by [`RuntimeRegistry`] (`TASK-271`).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::runtime_registry::{resolve_binary_in_dirs, RuntimeRegistry, RuntimeRegistryError};

/// Auto-detection result for one registered [`org_agent_runtime::AgentRuntime`] on this host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedRuntime {
    pub runtime_id: String,
    pub binary_name: String,
    pub installed: bool,
    pub binary_path: Option<PathBuf>,
    pub host_config_dir: Option<PathBuf>,
}

/// Probes every registered adapter in `registry` against `PATH` and standard user-local bin
/// directories, returning deterministic canonical-ID-ordered detection results.
pub fn detect_installed_runtimes(
    registry: &RuntimeRegistry,
    search_path: Option<&str>,
) -> Vec<DetectedRuntime> {
    let env_path = std::env::var_os("PATH");
    let home = std::env::var_os("HOME").map(PathBuf::from);
    detect_installed_runtimes_from(registry, search_path, env_path.as_deref(), home.as_deref())
}

/// Pure testable variant of [`detect_installed_runtimes`] with explicit `PATH` and `HOME` inputs.
pub fn detect_installed_runtimes_from(
    registry: &RuntimeRegistry,
    search_path: Option<&str>,
    env_path: Option<&OsStr>,
    home: Option<&Path>,
) -> Vec<DetectedRuntime> {
    registry
        .adapters()
        .map(|adapter| {
            let runtime_id = adapter.id().as_str().to_owned();
            let binary_name = adapter.binary_name().to_owned();
            let binary_path = match search_path {
                Some(_) => resolve_binary_in_dirs(&binary_name, search_path, env_path, home).ok(),
                None => registry
                    .resolve_binary(adapter.as_ref(), None)
                    .ok()
                    .or_else(|| resolve_binary_in_dirs(&binary_name, None, env_path, home).ok()),
            };
            let host_config_dir =
                host_config_dir_for_home(adapter.host_profile().host_config_subdir, home);
            DetectedRuntime {
                runtime_id,
                binary_name,
                installed: binary_path.is_some(),
                binary_path,
                host_config_dir,
            }
        })
        .collect()
}

/// Resolves a single runtime ID or alias and verifies its binary exists on the host, failing
/// closed with [`RuntimeRegistryError::UnknownRuntime`] or [`RuntimeRegistryError::BinaryNotFound`].
pub fn require_installed_runtime(
    registry: &RuntimeRegistry,
    runtime_id: &str,
    search_path: Option<&str>,
) -> Result<PathBuf, RuntimeRegistryError> {
    let adapter = registry.resolve(runtime_id)?;
    registry.resolve_binary(adapter.as_ref(), search_path)
}

fn host_config_dir_for_home(
    host_config_subdir: Option<&str>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    let candidate = home?.join(host_config_subdir?);
    if !candidate.is_dir() {
        return None;
    }
    candidate.canonicalize().ok().filter(|p| p.is_dir())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn detects_all_six_registered_runtimes_and_degrades_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        let home = dir.path().join("home");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(home.join(".gemini")).unwrap();
        fs::create_dir_all(home.join(".kimi")).unwrap();
        fs::create_dir_all(home.join(".pi")).unwrap();

        for name in ["claude", "codex", "agy", "gemini", "kimi", "pi-acp"] {
            fs::write(bin.join(name), b"#!/bin/sh\n").unwrap();
        }

        let reg = RuntimeRegistry::production();
        let detected =
            detect_installed_runtimes_from(&reg, Some(bin.to_str().unwrap()), None, Some(&home));
        assert_eq!(detected.len(), 6);
        assert!(detected
            .iter()
            .all(|d| d.installed && d.binary_path.is_some()));

        let gemini = detected.iter().find(|d| d.runtime_id == "gemini").unwrap();
        assert_eq!(
            gemini.host_config_dir.as_deref(),
            Some(home.join(".gemini").canonicalize().unwrap().as_path())
        );
        let claude = detected.iter().find(|d| d.runtime_id == "claude").unwrap();
        // Claude never mounts a host config dir into isolated sessions.
        assert!(claude.host_config_dir.is_none());

        // Missing binaries degrade gracefully in detection and fail closed in `require_installed_runtime`.
        let empty = dir.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        fs::create_dir_all(empty.join("kimi")).unwrap(); // Directory must not count as binary.
        let search = empty.to_str().unwrap();

        let missing = detect_installed_runtimes_from(&reg, Some(search), None, Some(&home));
        assert!(missing
            .iter()
            .all(|d| !d.installed && d.binary_path.is_none()));
        assert!(matches!(
            require_installed_runtime(&reg, "kimi", Some(search)),
            Err(RuntimeRegistryError::BinaryNotFound(bin)) if bin == "kimi"
        ));
        assert!(matches!(
            require_installed_runtime(&reg, "unknown-vendor", Some(search)),
            Err(RuntimeRegistryError::UnknownRuntime(_))
        ));
    }
}
