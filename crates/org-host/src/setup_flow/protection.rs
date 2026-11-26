//! Step 4's folder modes, read back from disk, and the disk-encryption report (ENC-3).

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use org_platform::disk::{EncryptionState, Wording};

use super::output::fail;

/// Folder and file modes under `root`, read back from disk: the paths whose mode is not
/// 700 (folders) or 600 (files), and the paths whose mode could not be read or whose
/// contents could not be listed. Symbolic links are reported, never followed. Only a root
/// that cannot be read is an error: what cannot be read inside it is reported, and never
/// refuses setup (X5 review F5).
///
/// The trees agents and Git write ([`written_by_agents_or_git`]) are checked at their own
/// folder only, which must be 700 like every other: what an agent's tools or Git make
/// inside it takes modes of their own (a checkout keeps a file's executable bit, a build
/// tool sets its own), and the 700 folder already keeps it private (TASK-155). Everything
/// else, the host's and offices' security records included, is checked file by file.
pub(crate) type Modes = (Vec<(PathBuf, u32)>, Vec<PathBuf>);

pub(crate) fn mode_exceptions(root: &Path) -> Result<Modes, String> {
    let mut exceptions = Vec::new();
    let mut unreadable = Vec::new();
    let mut pending = vec![root.to_owned()];
    while let Some(path) = pending.pop() {
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) if path == root => return Err(fail("reading modes", e)),
            Err(_) => {
                unreadable.push(path);
                continue;
            }
        };
        let mode = meta.permissions().mode() & 0o7777;
        let expected = if meta.is_dir() { 0o700 } else { 0o600 };
        if meta.file_type().is_symlink() || mode != expected {
            exceptions.push((path.clone(), mode));
        }
        if meta.is_dir() && !written_by_agents_or_git(&path, root) {
            match std::fs::read_dir(&path) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(entry) => pending.push(entry.path()),
                            Err(_) => unreadable.push(path.clone()),
                        }
                    }
                }
                Err(e) if path == root => return Err(fail("reading modes", e)),
                Err(_) => unreadable.push(path),
            }
        }
    }
    exceptions.sort();
    unreadable.sort();
    unreadable.dedup();
    Ok((exceptions, unreadable))
}

/// Whether `path`, below the instance `root`, is a folder whose contents agents or Git write
/// (the `doc-35` layout), which [`mode_exceptions`] checks at the folder only:
///
/// - an agent's private home, `…/durable/local/offices/<office>/agents/<employee>/home`
///   (its `runtime` and `tmp` folders, which the office writes, are checked in full);
/// - Git's trees in an organization's runtime root (a sibling of `durable`, never below
///   it): an office's bare repository caches, `runtime/<office>/repositories`, and a task
///   worktree's folder, `runtime/<office>/teams/…/agents/<employee>/[projects/<project>/]
///   tasks/<task>/repositories`.
///
/// The office's own record of where a repository lives
/// (`durable/local/offices/<office>/repositories`) is not one of them.
pub(crate) fn written_by_agents_or_git(path: &Path, root: &Path) -> bool {
    let Ok(below) = path.strip_prefix(root) else {
        return false;
    };
    let parts: Vec<&str> = below
        .components()
        .map(|part| part.as_os_str().to_str().unwrap_or(""))
        .collect();
    let agent_home = matches!(
        parts.as_slice(),
        [.., "durable", "local", "offices", _, "agents", _, "home"]
    );
    let git = parts.last() == Some(&"repositories")
        && parts
            .iter()
            .position(|part| *part == "runtime")
            .filter(|at| !parts[..*at].contains(&"durable"))
            .is_some_and(|at| match &parts[at + 1..] {
                [_office, "repositories"] => true,
                [_office, "teams", rest @ ..] => {
                    rest.contains(&"agents") && matches!(rest, [.., "tasks", _, "repositories"])
                }
                _ => false,
            });
    agent_home || git
}

/// The ENC-3 lines: disk encryption, the modes actually found and the isolation mode.
/// `wording` is what this system calls its disk encryption and where a person turns it on
/// (`org_platform::disk::wording`).
pub(crate) fn protection_report(
    encryption: &EncryptionState,
    wording: Wording,
    user: &str,
    exceptions: &[(PathBuf, u32)],
    unreadable: &[PathBuf],
) -> Vec<String> {
    let Wording { name, setting } = wording;
    let mut lines = vec![match encryption {
        EncryptionState::On => format!("Disk encryption ({name}): on."),
        EncryptionState::Off => format!(
            "Disk encryption is off. Agentic Org does not encrypt its data; turn on {setting}."
        ),
        EncryptionState::Unknown(why) => format!(
            "Disk encryption could not be determined ({why}). Agentic Org does not encrypt \
             its data; check {setting}."
        ),
    }];
    if exceptions.is_empty() && unreadable.is_empty() {
        lines.push(format!(
            "File permissions: the instance folder and everything in it are readable only by \
             {user} (folders 700, files 600). Agent homes and Git checkouts are checked at \
             their own folder, whose 700 keeps what agents and Git write inside private."
        ));
    } else if !exceptions.is_empty() {
        lines.push(format!(
            "File permissions: {} path(s) in the instance folder are not private to {user}:",
            exceptions.len()
        ));
        for (path, mode) in exceptions.iter().take(10) {
            lines.push(format!("  {:o} {}", mode, path.display()));
        }
    }
    if !unreadable.is_empty() {
        lines.push(format!(
            "File permissions: {} path(s) in the instance folder could not be read, so their \
             modes are not known:",
            unreadable.len()
        ));
        for path in unreadable.iter().take(10) {
            lines.push(format!("  {}", path.display()));
        }
    }
    lines.push(format!(
        "Agents run as {user} (same-user mode): only the tool sandbox separates agents from \
         the server's data."
    ));
    lines
}

#[cfg(test)]
#[path = "protection_tests.rs"]
mod tests;
