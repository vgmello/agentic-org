//! Validate raw request paths once, before route or static-asset selection.

/// A malformed path never reaches authorization, dispatch or a filesystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidPath;

/// Decoded segments; private storage prevents unvalidated path construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestPath {
    segments: Vec<String>,
}

impl RequestPath {
    /// `raw` is the URI path, excluding its query. No path normalization occurs.
    /// Percent escapes are decoded exactly once, including UTF-8 static names.
    pub fn parse(raw: &str) -> Result<Self, InvalidPath> {
        let Some(relative) = raw.strip_prefix('/') else {
            return Err(InvalidPath);
        };
        if relative.is_empty() {
            return Ok(Self {
                segments: Vec::new(),
            });
        }
        let segments = relative
            .split('/')
            .map(decode_segment)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { segments })
    }

    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    /// Whole-segment classification, so /apiary and /_apple are browser paths.
    pub fn is_api(&self) -> bool {
        self.segments.first().is_some_and(|value| value == "api")
    }

    pub fn is_app_asset(&self) -> bool {
        self.segments.first().is_some_and(|value| value == "_app")
    }

    /// An in-memory build key, never a disk path and never decoded again.
    pub fn asset_key(&self) -> String {
        self.segments.join("/")
    }
}

fn hex(value: u8) -> Result<u8, InvalidPath> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(InvalidPath),
    }
}

fn decode_segment(raw: &str) -> Result<String, InvalidPath> {
    let mut decoded = Vec::with_capacity(raw.len());
    let mut input = raw.as_bytes().iter().copied();
    while let Some(byte) = input.next() {
        if matches!(byte, b'?' | b'#') {
            return Err(InvalidPath);
        }
        let byte = if byte == b'%' {
            let high = hex(input.next().ok_or(InvalidPath)?)?;
            let low = hex(input.next().ok_or(InvalidPath)?)?;
            high * 16 + low
        } else {
            byte
        };
        if byte.is_ascii_control() || matches!(byte, b'/' | b'\\') {
            return Err(InvalidPath);
        }
        decoded.push(byte);
    }
    let decoded = String::from_utf8(decoded).map_err(|_| InvalidPath)?;
    if decoded.is_empty() || matches!(decoded.as_str(), "." | "..") {
        return Err(InvalidPath);
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separators_dots_empty_segments_and_invalid_escapes_never_reach_routing() {
        for raw in [
            "api", "//", "/a/", "/a//b", "/.", "/..", "/%2e", "/%2E.", "/.%2e", "/%2e%2E",
            "/a%2fb", "/a%2Fb", "/%5c", "/%5C", "/a\\b", "/%", "/%2", "/%gg", "/%00", "/%0a",
            "/%ff", "/a?b", "/a#b",
        ] {
            assert_eq!(RequestPath::parse(raw), Err(InvalidPath), "{raw}");
        }
    }

    #[test]
    fn percent_decoding_is_once_only_and_never_normalizes() {
        let path = RequestPath::parse("/organizations/%252f/%252e%252e/caf%C3%A9").unwrap();
        assert_eq!(path.segments(), &["organizations", "%2f", "%2e%2e", "café"]);
        assert_eq!(path.asset_key(), "organizations/%2f/%2e%2e/café");
        assert!(RequestPath::parse("/").unwrap().segments().is_empty());
    }

    #[test]
    fn reserved_namespaces_are_whole_decoded_segments() {
        for raw in ["/api", "/api/v1/version", "/api/v2/x", "/%61pi/x"] {
            assert!(RequestPath::parse(raw).unwrap().is_api(), "{raw}");
        }
        for raw in ["/_app", "/_app/missing.js"] {
            assert!(RequestPath::parse(raw).unwrap().is_app_asset(), "{raw}");
        }
        for raw in ["/apiary", "/_apple", "/welcome", "/"] {
            let path = RequestPath::parse(raw).unwrap();
            assert!(!path.is_api() && !path.is_app_asset(), "{raw}");
        }
    }
}
