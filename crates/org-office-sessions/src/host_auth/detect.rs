//! Host runtime CLI auto-detection backed by [`RuntimeRegistry`] (`TASK-271`).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::runtime_registry::{RuntimeRegistry, RuntimeRegistryError};

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
///
/// When an explicit `binary_override` is registered for a runtime, ONLY that override is validated
/// (failing closed without falling back to `PATH`). Otherwise resolution uses strictly
/// `(search_path, env_path, home)` and never reads ambient process `PATH` or `HOME`.
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
            let binary_path = registry
                .resolve_binary_from(adapter.as_ref(), search_path, env_path, home)
                .ok();
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
    let home = home.filter(|h| h.is_absolute())?;
    let candidate = home.join(host_config_subdir?);
    if !candidate.is_dir() {
        return None;
    }
    candidate.canonicalize().ok().filter(|p| p.is_dir())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::sync::Arc;

    use super::*;

    fn write_exec(path: &Path, mode: u32) {
        fs::write(path, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn detects_all_six_registered_runtimes_and_degrades_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        let home = dir.path().join("home");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(home.join(".gemini")).unwrap();
        fs::create_dir_all(home.join(".codex")).unwrap();
        fs::create_dir_all(home.join(".kimi")).unwrap();
        fs::create_dir_all(home.join(".pi")).unwrap();

        for name in ["claude", "codex", "agy", "gemini", "kimi", "pi-acp"] {
            write_exec(&bin.join(name), 0o755);
        }

        let reg = RuntimeRegistry::production();
        let detected =
            detect_installed_runtimes_from(&reg, Some(bin.to_str().unwrap()), None, Some(&home));
        assert_eq!(detected.len(), 6);
        assert!(detected
            .iter()
            .all(|d| d.installed && d.binary_path.is_some()));

        let agy = detected.iter().find(|d| d.runtime_id == "agy").unwrap();
        assert_eq!(
            agy.host_config_dir.as_deref(),
            Some(home.join(".gemini").canonicalize().unwrap().as_path())
        );
        let codex = detected.iter().find(|d| d.runtime_id == "codex").unwrap();
        assert_eq!(
            codex.host_config_dir.as_deref(),
            Some(home.join(".codex").canonicalize().unwrap().as_path())
        );
        // Vault-only runtimes (`claude`, `gemini`, `kimi`, `pi`) NEVER mount host config dirs.
        for vault_only in ["claude", "gemini", "kimi", "pi"] {
            let entry = detected
                .iter()
                .find(|d| d.runtime_id == vault_only)
                .unwrap();
            assert!(
                entry.host_config_dir.is_none(),
                "{vault_only} must not expose host_config_dir"
            );
        }

        // Missing binaries, directories, and 0644 files degrade cleanly and fail closed.
        let empty = dir.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        fs::create_dir_all(empty.join("kimi")).unwrap(); // Directory must not count as binary.
        write_exec(&empty.join("gemini"), 0o644); // Non-executable file must not count as binary.
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
            require_installed_runtime(&reg, "gemini", Some(search)),
            Err(RuntimeRegistryError::BinaryNotFound(bin)) if bin == "gemini"
        ));
        assert!(matches!(
            require_installed_runtime(&reg, "unknown-vendor", Some(search)),
            Err(RuntimeRegistryError::UnknownRuntime(_))
        ));
    }

    #[test]
    fn explicit_binary_override_fails_closed_without_path_fallback_and_ignores_ambient_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let path_bin = root.join("bin");
        fs::create_dir_all(&path_bin).unwrap();
        write_exec(&path_bin.join("gemini"), 0o755);
        write_exec(&path_bin.join("kimi"), 0o755);

        let non_exec_override = root.join("bad-gemini");
        write_exec(&non_exec_override, 0o644);
        let dir_override = root.join("dir-kimi");
        fs::create_dir_all(&dir_override).unwrap();
        let good_override = root.join("custom-pi");
        write_exec(&good_override, 0o755);

        let mut reg = RuntimeRegistry::empty();
        reg.register(
            Arc::new(org_agent_runtime_acp::GeminiRuntime::new()),
            Some(non_exec_override),
        );
        reg.register(
            Arc::new(org_agent_runtime_acp::KimiRuntime::new()),
            Some(dir_override),
        );
        reg.register(
            Arc::new(org_agent_runtime_acp::PiRuntime::new()),
            Some(good_override.clone()),
        );

        // Even though valid `gemini` and `kimi` executables exist in `search_path` and `env_path`,
        // their invalid explicit overrides MUST fail closed (NO fallback to PATH).
        for (search_path, env_path) in [
            (Some(path_bin.to_str().unwrap()), None),
            (None, Some(path_bin.as_os_str())),
        ] {
            let detected = detect_installed_runtimes_from(&reg, search_path, env_path, None);
            let gemini = detected.iter().find(|d| d.runtime_id == "gemini").unwrap();
            assert!(!gemini.installed && gemini.binary_path.is_none());
            let kimi = detected.iter().find(|d| d.runtime_id == "kimi").unwrap();
            assert!(!kimi.installed && kimi.binary_path.is_none());
            let pi = detected.iter().find(|d| d.runtime_id == "pi").unwrap();
            assert!(pi.installed);
            assert_eq!(pi.binary_path.as_deref(), Some(good_override.as_path()));
        }

        // Empty/relative `search_path` segments never resolve CWD.
        let prod = RuntimeRegistry::production();
        let rel = detect_installed_runtimes_from(&prod, Some(":.:./bin"), None, None);
        assert!(rel.iter().all(|d| !d.installed && d.binary_path.is_none()));
    }
}
