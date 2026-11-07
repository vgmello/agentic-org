//! Workspace paths: keeping reads inside the root, and mapping a changed file to
//! the part of the model it feeds.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result};

/// Resolves a workspace-relative path to a canonical path inside `root`.
///
/// Refuses absolute paths and `..` components outright, then canonicalizes (which
/// follows symlinks) and refuses anything that ends up outside the canonical root,
/// so a symlink inside the workspace can't be used to reach other files. The path
/// must exist. An empty path resolves to the root itself.
///
/// The check happens at call time; a symlink swapped in afterwards isn't caught.
pub fn resolve_in_workspace(root: impl AsRef<Path>, rel: impl AsRef<Path>) -> Result<PathBuf> {
    let root = root.as_ref();
    let canonical_root = fs::canonicalize(root).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => Error::RootNotFound(root.to_path_buf()),
        _ => Error::Io {
            path: root.to_path_buf(),
            source: e,
        },
    })?;
    resolve_under(&canonical_root, rel.as_ref())
}

/// [`resolve_in_workspace`] for a root that is already canonical.
pub(crate) fn resolve_under(canonical_root: &Path, rel: &Path) -> Result<PathBuf> {
    check_relative(rel)?;
    let joined = canonical_root.join(rel);
    let resolved = fs::canonicalize(&joined).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => Error::NotFound(rel.to_path_buf()),
        _ => Error::Io {
            path: joined.clone(),
            source: e,
        },
    })?;
    if !resolved.starts_with(canonical_root) {
        return Err(Error::OutsideWorkspace(rel.to_path_buf()));
    }
    Ok(resolved)
}

/// Accepts only plain relative paths: no root, drive prefix, `..` or NUL byte.
fn check_relative(rel: &Path) -> Result<()> {
    if rel.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(Error::InvalidPath(rel.to_path_buf()));
    }
    for component in rel.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => return Err(Error::ParentComponent(rel.to_path_buf())),
            Component::RootDir | Component::Prefix(_) => {
                return Err(Error::AbsolutePath(rel.to_path_buf()))
            }
        }
    }
    Ok(())
}

/// What a workspace file or folder feeds in the model. See [`classify`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileKind {
    /// The root, `projects/`, `hr/`, `.claude/` or `.claude/agents/`: a folder
    /// above many model files changed, so re-read everything.
    Workspace,
    /// `projects/<key>/` or `projects/<key>/agents/`: re-read the whole project.
    ProjectDir { project: String },
    /// `projects/<key>/project.json`.
    ProjectProfile { project: String },
    /// `projects/<key>/CLAUDE.md`.
    ProjectBrief { project: String },
    /// `projects/<key>/agents/<id>/`: re-read the whole agent.
    AgentDir { project: String, agent: String },
    /// `projects/<key>/agents/<id>/agent.json`.
    AgentProfile { project: String, agent: String },
    /// `projects/<key>/agents/<id>/tasks/BACKLOG.md` (or its `tasks/` folder).
    Backlog { project: String, agent: String },
    /// `hr/roles/`: re-read the whole catalog.
    RoleCatalog,
    /// `hr/roles/<id>.toml` (not `_*.toml` templates).
    Role { role: String },
    /// `.claude/agents/hr.md`.
    HrAgent,
}

/// Maps a workspace-relative path (forward or platform separators) to the part of
/// the model it feeds, or `None` if the model doesn't read it (journals, memory,
/// `workspace/`, hidden files, absolute paths and anything with `..`).
pub fn classify(rel: impl AsRef<Path>) -> Option<FileKind> {
    let mut parts = Vec::new();
    for component in rel.as_ref().components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?),
            Component::CurDir => {}
            _ => return None,
        }
    }
    let name = |s: &str| (!s.is_empty() && !s.starts_with('.')).then(|| s.to_string());
    match parts.as_slice() {
        [] | ["projects"] | ["hr"] | [".claude"] | [".claude", "agents"] => {
            Some(FileKind::Workspace)
        }
        ["projects", key] | ["projects", key, "agents"] => Some(FileKind::ProjectDir {
            project: name(key)?,
        }),
        ["projects", key, "project.json"] => Some(FileKind::ProjectProfile {
            project: name(key)?,
        }),
        ["projects", key, "CLAUDE.md"] => Some(FileKind::ProjectBrief {
            project: name(key)?,
        }),
        ["projects", key, "agents", id] => Some(FileKind::AgentDir {
            project: name(key)?,
            agent: name(id)?,
        }),
        ["projects", key, "agents", id, "agent.json"] => Some(FileKind::AgentProfile {
            project: name(key)?,
            agent: name(id)?,
        }),
        ["projects", key, "agents", id, "tasks"]
        | ["projects", key, "agents", id, "tasks", "BACKLOG.md"] => Some(FileKind::Backlog {
            project: name(key)?,
            agent: name(id)?,
        }),
        ["hr", "roles"] => Some(FileKind::RoleCatalog),
        ["hr", "roles", file] => {
            let id = file.strip_suffix(".toml")?;
            (!id.starts_with('_')).then_some(())?;
            Some(FileKind::Role { role: name(id)? })
        }
        [".claude", "agents", "hr.md"] => Some(FileKind::HrAgent),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_model_files() {
        let p = || "pay".to_string();
        let a = || "pay-dev-sam".to_string();
        assert_eq!(
            classify("projects/pay"),
            Some(FileKind::ProjectDir { project: p() })
        );
        assert_eq!(
            classify("projects/pay/agents"),
            Some(FileKind::ProjectDir { project: p() })
        );
        assert_eq!(
            classify("projects/pay/project.json"),
            Some(FileKind::ProjectProfile { project: p() })
        );
        assert_eq!(
            classify("./projects/pay/CLAUDE.md"),
            Some(FileKind::ProjectBrief { project: p() })
        );
        assert_eq!(
            classify("projects/pay/agents/pay-dev-sam"),
            Some(FileKind::AgentDir {
                project: p(),
                agent: a()
            })
        );
        assert_eq!(
            classify("projects/pay/agents/pay-dev-sam/agent.json"),
            Some(FileKind::AgentProfile {
                project: p(),
                agent: a()
            })
        );
        assert_eq!(
            classify("projects/pay/agents/pay-dev-sam/tasks/BACKLOG.md"),
            Some(FileKind::Backlog {
                project: p(),
                agent: a()
            })
        );
        assert_eq!(classify("hr/roles"), Some(FileKind::RoleCatalog));
        for ancestor in ["", ".", "projects", "hr", ".claude", ".claude/agents"] {
            assert_eq!(classify(ancestor), Some(FileKind::Workspace), "{ancestor}");
        }
        assert_eq!(
            classify("hr/roles/staff-engineer.toml"),
            Some(FileKind::Role {
                role: "staff-engineer".into()
            })
        );
        assert_eq!(classify(".claude/agents/hr.md"), Some(FileKind::HrAgent));
    }

    #[test]
    fn ignores_everything_else() {
        for path in [
            "projects/.DS_Store",
            "projects/pay/agents/.hidden/agent.json",
            "projects/pay/agents/pay-dev-sam/journal/2026-11-07.md",
            "projects/pay/agents/pay-dev-sam/memory/MEMORY.md",
            "projects/pay/agents/pay-dev-sam/workspace/agent.json",
            "projects/pay/agents/pay-dev-sam/CLAUDE.md",
            "hr/roles/_template.toml",
            "hr/roles/.toml",
            "hr/roles/notes.md",
            "hr/hr.py",
            ".claude/agents/pay-dev-sam.md",
            "/projects/pay/project.json",
            "projects/../projects/pay/project.json",
        ] {
            assert_eq!(classify(path), None, "{path}");
        }
    }

    #[test]
    fn refuses_parent_and_absolute_paths_before_touching_the_disk() {
        let root = Path::new("/nonexistent-org-core-root");
        assert!(matches!(
            resolve_under(root, Path::new("../x")),
            Err(Error::ParentComponent(_))
        ));
        assert!(matches!(
            resolve_under(root, Path::new("a/../../x")),
            Err(Error::ParentComponent(_))
        ));
        assert!(matches!(
            resolve_under(root, Path::new("/etc/passwd")),
            Err(Error::AbsolutePath(_))
        ));
        assert!(matches!(
            resolve_under(root, Path::new("a\0b")),
            Err(Error::InvalidPath(_))
        ));
        assert!(matches!(
            resolve_in_workspace(root, "x"),
            Err(Error::RootNotFound(_))
        ));
    }
}
