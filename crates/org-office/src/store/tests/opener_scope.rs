//! Plain, private input shared by inventory construction and the policy visitor.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SourceKind {
    Rust,
    Config,
}

pub(super) struct SourceScope {
    pub root: PathBuf,
    pub files: BTreeMap<PathBuf, SourceKind>,
}

impl SourceScope {
    pub fn require_rust(&self, path: &Path) -> PathBuf {
        let path = self.require(path);
        assert_eq!(
            self.files[&path],
            SourceKind::Rust,
            "not a Rust source: {}",
            path.display()
        );
        path
    }

    pub fn require(&self, path: &Path) -> PathBuf {
        let relative = path
            .strip_prefix(&self.root)
            .expect("policy path escapes repository");
        let mut prefix = self.root.clone();
        for part in relative.components() {
            prefix.push(part);
            assert!(
                !std::fs::symlink_metadata(&prefix)
                    .expect("missing policy path")
                    .file_type()
                    .is_symlink(),
                "policy path contains a symlink: {}",
                prefix.display()
            );
        }
        let path = path.canonicalize().expect("missing policy input");
        assert!(
            self.files.contains_key(&path) && path.is_file(),
            "policy input is outside tracked inventory: {}",
            path.display()
        );
        path
    }
}
