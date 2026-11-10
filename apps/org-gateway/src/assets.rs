//! Immutable startup build bytes. HTTP requests never read assets from disk.

use std::collections::BTreeMap;

use hyper::body::Bytes;
use hyper::header::HeaderValue;

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
}

struct BuildFile {
    bytes: Bytes,
    html_policy: Option<HeaderValue>,
}

/// Loaded build files and their validated HTML policies.
pub struct BuildFiles {
    files: BTreeMap<String, BuildFile>,
    total_bytes: usize,
}

impl BuildFiles {
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
            let file = BuildFile {
                bytes: Bytes::from(bytes),
                html_policy,
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

fn is_html(key: &str) -> bool {
    key.rsplit_once('.').is_some_and(|(_, extension)| {
        extension.eq_ignore_ascii_case("html") || extension.eq_ignore_ascii_case("htm")
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
