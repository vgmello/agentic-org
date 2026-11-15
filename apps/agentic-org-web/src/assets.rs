//! Immutable startup build bytes. HTTP requests never read assets from disk.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use base64::prelude::*;
use hyper::body::Bytes;
use hyper::header::HeaderValue;
use sha2::{Digest, Sha256};

use crate::csp;

pub const MAX_BUILD_BYTES: usize = 64 * 1024 * 1024;
const MAX_BUILD_FILES: usize = 4096;
const MAX_KEY_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildError {
    TooLarge,
    TooManyFiles,
    InvalidKey,
    DuplicateKey,
    MissingEntry,
    InvalidEntry,
    InvalidHtml,
    MissingInlineScript,
    InlineEventHandler,
    JavascriptUrl,
    CspMetaTag,
    InvalidPolicy,
    UnreadableBuild,
    UnsafeBuildEntry,
}

struct BuildFile {
    bytes: Bytes,
    html_policy: Option<HeaderValue>,
    etag: Option<HeaderValue>,
}

/// Loaded build files and their validated HTML policies.
pub struct BuildFiles {
    files: BTreeMap<String, BuildFile>,
    total_bytes: usize,
}

impl BuildFiles {
    /// Read a release's directory once at startup, with no request-path filesystem access.
    pub fn from_directory(root: &Path) -> Result<Self, BuildError> {
        let kind = fs::symlink_metadata(root)
            .map_err(|_| BuildError::UnreadableBuild)?
            .file_type();
        if !kind.is_dir() {
            return Err(BuildError::UnsafeBuildEntry);
        }
        let mut files = Vec::new();
        let mut pending = vec![(root.to_path_buf(), String::new())];
        let mut entries = 0;
        let mut remaining = MAX_BUILD_BYTES;
        while let Some((directory, prefix)) = pending.pop() {
            for entry in fs::read_dir(directory).map_err(|_| BuildError::UnreadableBuild)? {
                let entry = entry.map_err(|_| BuildError::UnreadableBuild)?;
                entries += 1;
                if entries > MAX_BUILD_FILES {
                    return Err(BuildError::TooManyFiles);
                }
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| BuildError::InvalidKey)?;
                let key = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}/{name}")
                };
                if !valid_key(&key) {
                    return Err(BuildError::InvalidKey);
                }
                let kind = entry.file_type().map_err(|_| BuildError::UnreadableBuild)?;
                if kind.is_dir() {
                    pending.push((entry.path(), key));
                } else if kind.is_file() {
                    let file = File::open(entry.path()).map_err(|_| BuildError::UnreadableBuild)?;
                    let metadata = file.metadata().map_err(|_| BuildError::UnreadableBuild)?;
                    if !metadata.is_file() {
                        return Err(BuildError::UnsafeBuildEntry);
                    }
                    if metadata.len() > remaining as u64 {
                        return Err(BuildError::TooLarge);
                    }
                    let mut bytes = Vec::new();
                    file.take(remaining as u64 + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|_| BuildError::UnreadableBuild)?;
                    remaining = remaining
                        .checked_sub(bytes.len())
                        .ok_or(BuildError::TooLarge)?;
                    files.push((key, bytes));
                } else {
                    return Err(BuildError::UnsafeBuildEntry);
                }
            }
        }
        Self::from_files(files)
    }

    /// Takes ownership of one complete build; future requests share these bytes.
    /// Relative keys are literal filenames, not URL-encoded request paths.
    pub fn from_files(
        files: impl IntoIterator<Item = (String, Vec<u8>)>,
    ) -> Result<Self, BuildError> {
        Self::with_limits(files, MAX_BUILD_BYTES, MAX_BUILD_FILES)
    }

    fn with_limits(
        files: impl IntoIterator<Item = (String, Vec<u8>)>,
        max_bytes: usize,
        max_files: usize,
    ) -> Result<Self, BuildError> {
        let mut loaded = BTreeMap::new();
        let mut total_bytes = 0usize;
        for (key, bytes) in files {
            if !valid_key(&key) {
                return Err(BuildError::InvalidKey);
            }
            if loaded.len() >= max_files {
                return Err(BuildError::TooManyFiles);
            }
            total_bytes = total_bytes
                .checked_add(bytes.len())
                .filter(|total| *total <= max_bytes)
                .ok_or(BuildError::TooLarge)?;
            let html_policy = if is_html(&key) {
                let error = if key == "index.html" {
                    BuildError::InvalidEntry
                } else {
                    BuildError::InvalidHtml
                };
                let html = std::str::from_utf8(&bytes).map_err(|_| error)?;
                if html.is_empty() {
                    return Err(error);
                }
                Some(csp::html_policy(html)?)
            } else {
                None
            };
            let etag = (html_policy.is_none() && !is_json(&key)).then(|| {
                let encoded = BASE64_STANDARD.encode(Sha256::digest(&bytes));
                HeaderValue::from_str(&format!("\"{encoded}\"")).expect("ASCII ETag")
            });
            let file = BuildFile {
                bytes: Bytes::from(bytes),
                html_policy,
                etag,
            };
            if loaded.insert(key, file).is_some() {
                return Err(BuildError::DuplicateKey);
            }
        }
        if !loaded.contains_key("index.html") {
            return Err(BuildError::MissingEntry);
        }
        Ok(Self {
            files: loaded,
            total_bytes,
        })
    }

    pub fn file(&self, key: &str) -> Option<&Bytes> {
        self.files.get(key).map(|file| &file.bytes)
    }

    pub fn files(&self) -> impl Iterator<Item = (&str, &Bytes)> {
        self.files
            .iter()
            .map(|(key, file)| (key.as_str(), &file.bytes))
    }

    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    pub(crate) fn etag(&self, key: &str) -> Option<&HeaderValue> {
        self.files.get(key)?.etag.as_ref()
    }

    /// The startup-computed policy for one loaded file and the listener's transport mode.
    pub fn content_security_policy(&self, key: &str, https: bool) -> Option<HeaderValue> {
        match self.files.get(key)?.html_policy.as_ref() {
            Some(policy) if https => {
                let mut value = policy.as_bytes().to_vec();
                value.extend_from_slice(b"; upgrade-insecure-requests");
                Some(HeaderValue::from_bytes(&value).expect("appended a fixed ASCII directive"))
            }
            Some(policy) => Some(policy.clone()),
            None => Some(csp::non_html_policy()),
        }
    }
}

pub(crate) fn is_html(key: &str) -> bool {
    key.rsplit_once('.').is_some_and(|(_, extension)| {
        extension.eq_ignore_ascii_case("html") || extension.eq_ignore_ascii_case("htm")
    })
}

pub(crate) fn is_json(key: &str) -> bool {
    key.rsplit_once('.').is_some_and(|(_, extension)| {
        matches!(
            extension.to_ascii_lowercase().as_str(),
            "json" | "map" | "webmanifest"
        )
    })
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_KEY_BYTES
        && !key
            .bytes()
            .any(|byte| byte.is_ascii_control() || matches!(byte, b'\\' | b'?' | b'#'))
        && key
            .split('/')
            .all(|part| !part.is_empty() && !matches!(part, "." | ".."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> (String, Vec<u8>) {
        (
            "index.html".into(),
            b"<html><script>BUILD_A</script></html>".to_vec(),
        )
    }

    #[test]
    fn filesystem_loader_owns_the_original_build_and_rejects_symlinks_and_oversize() {
        use std::os::unix::fs::symlink;

        let build = tempfile::tempdir().unwrap();
        let entry_path = build.path().join("index.html");
        fs::write(&entry_path, entry().1).unwrap();
        let nested = build.path().join("_app");
        fs::create_dir(&nested).unwrap();
        let asset = nested.join("asset.js");
        fs::write(&asset, b"BUILD_A").unwrap();
        let files = BuildFiles::from_directory(build.path()).unwrap();
        fs::write(&asset, b"BUILD_B").unwrap();
        fs::write(&entry_path, b"<script>BUILD_B</script>").unwrap();
        assert_eq!(files.file("_app/asset.js").unwrap().as_ref(), b"BUILD_A");
        assert_eq!(files.file("index.html").unwrap().as_ref(), entry().1);

        let unsafe_path = nested.join("link.js");
        symlink(&asset, &unsafe_path).unwrap();
        assert!(matches!(
            BuildFiles::from_directory(build.path()),
            Err(BuildError::UnsafeBuildEntry)
        ));
        fs::remove_file(unsafe_path).unwrap();
        File::options()
            .write(true)
            .open(asset)
            .unwrap()
            .set_len(MAX_BUILD_BYTES as u64 + 1)
            .unwrap();
        assert!(matches!(
            BuildFiles::from_directory(build.path()),
            Err(BuildError::TooLarge)
        ));
    }

    #[test]
    fn every_html_file_is_validated_and_https_adds_only_its_html_directive() {
        assert!(matches!(
            BuildFiles::from_files([
                entry(),
                (
                    "other.HTM".into(),
                    b"<html>no inline bootstrap</html>".to_vec()
                ),
            ]),
            Err(BuildError::MissingInlineScript)
        ));
        let files = BuildFiles::from_files([entry(), ("style.css".into(), vec![])]).unwrap();
        let http = files.content_security_policy("index.html", false).unwrap();
        let https = files.content_security_policy("index.html", true).unwrap();
        assert_eq!(
            https.to_str().unwrap(),
            format!("{}; upgrade-insecure-requests", http.to_str().unwrap())
        );
        assert_eq!(
            files.content_security_policy("style.css", true),
            files.content_security_policy("style.css", false)
        );
        assert!(files
            .content_security_policy("missing.html", false)
            .is_none());
    }

    #[test]
    fn snapshot_owns_every_file_and_counts_aggregate_bytes() {
        let original = b"export const build = 'A';".to_vec();
        let mut supplied = original.clone();
        let files = BuildFiles::from_files([
            entry(),
            ("_app/immutable/a.js".into(), supplied.clone()),
            ("literal%2f.txt".into(), b"once-only name".to_vec()),
        ])
        .unwrap();
        supplied.fill(b'X');
        assert_eq!(
            files.file("_app/immutable/a.js").unwrap().as_ref(),
            original
        );
        assert!(files.file("literal%2f.txt").is_some());
        assert_eq!(files.total_bytes(), entry().1.len() + original.len() + 14);
        assert_eq!(files.files().count(), 3);
    }

    #[test]
    fn byte_and_file_limits_include_the_entry_and_empty_files() {
        let size = entry().1.len();
        assert!(BuildFiles::with_limits([entry()], size, 1).is_ok());
        assert!(matches!(
            BuildFiles::with_limits([entry()], size - 1, 1),
            Err(BuildError::TooLarge)
        ));
        assert!(matches!(
            BuildFiles::with_limits([entry(), ("a.js".into(), vec![0])], size, 2),
            Err(BuildError::TooLarge)
        ));
        assert!(matches!(
            BuildFiles::with_limits([entry(), ("empty".into(), vec![])], size, 1),
            Err(BuildError::TooManyFiles)
        ));
        assert_eq!(MAX_BUILD_BYTES, 67_108_864);
    }

    #[test]
    fn invalid_or_ambiguous_builds_are_refused_before_serving() {
        for key in [
            "", "/x", "a/", "a//b", "../x", "a/../b", "./x", "a\\b", "a?b", "a#b", "a\0b",
        ] {
            assert!(
                matches!(
                    BuildFiles::from_files([entry(), (key.into(), vec![])]),
                    Err(BuildError::InvalidKey)
                ),
                "{key:?}"
            );
        }
        assert!(matches!(
            BuildFiles::from_files([entry(), entry()]),
            Err(BuildError::DuplicateKey)
        ));
        assert!(matches!(
            BuildFiles::from_files([]),
            Err(BuildError::MissingEntry)
        ));
        assert!(matches!(
            BuildFiles::from_files([("index.html".into(), vec![])]),
            Err(BuildError::InvalidEntry)
        ));
        assert!(matches!(
            BuildFiles::from_files([("index.html".into(), vec![255])]),
            Err(BuildError::InvalidEntry)
        ));
    }
}
