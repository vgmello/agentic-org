use std::net::{IpAddr, SocketAddr};

use hyper::Uri;

/// Public browser origin, canonicalized without DNS resolution. This is startup
/// configuration; it is never inferred from a browser's Host/Origin headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredOrigin {
    serialized: String,
    ip: Option<IpAddr>,
    port: u16,
    https: bool,
}

impl ConfiguredOrigin {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        if value.len() > 4096
            || value
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control() || byte == b'#')
        {
            return Err("invalid browser origin");
        }
        let uri: Uri = value.parse().map_err(|_| "invalid browser origin")?;
        let https = match uri.scheme_str() {
            Some(scheme) if scheme.eq_ignore_ascii_case("https") => true,
            Some(scheme) if scheme.eq_ignore_ascii_case("http") => false,
            _ => return Err("browser origin requires HTTP or HTTPS"),
        };
        if uri
            .path_and_query()
            .is_some_and(|path| path.as_str() != "/")
        {
            return Err("browser origin cannot contain a path or query");
        }
        let authority = uri
            .authority()
            .ok_or("browser origin requires an authority")?;
        if authority.as_str().contains(['@', '%']) {
            return Err("invalid browser origin authority");
        }
        let raw_host = authority.host();
        let suffix = &authority.as_str()[raw_host.len()..];
        let port = match suffix {
            "" => {
                if https {
                    443
                } else {
                    80
                }
            }
            suffix => suffix
                .strip_prefix(':')
                .filter(|port| !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()))
                .ok_or("invalid browser origin port")?
                .parse::<u16>()
                .map_err(|_| "invalid browser origin port")?,
        };
        if port == 0 {
            return Err("browser origin port must be nonzero");
        }
        let bare_host = raw_host
            .strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .unwrap_or(raw_host);
        if bare_host.is_empty() {
            return Err("browser origin requires a host");
        }
        let ip = bare_host.parse::<IpAddr>().ok();
        let host = match ip {
            Some(IpAddr::V4(ip)) => ip.to_string(),
            Some(IpAddr::V6(ip)) => format!("[{ip}]"),
            None => {
                if !bare_host.is_ascii() || bare_host.contains(':') || raw_host.starts_with('[') {
                    return Err("invalid browser origin host");
                }
                bare_host.to_ascii_lowercase()
            }
        };
        let scheme = if https { "https" } else { "http" };
        let suffix = if port == if https { 443 } else { 80 } {
            String::new()
        } else {
            format!(":{port}")
        };
        Ok(Self {
            serialized: format!("{scheme}://{host}{suffix}"),
            ip,
            port,
            https,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.serialized
    }

    pub(crate) fn is_https(&self) -> bool {
        self.https
    }

    pub(crate) fn validate_http_loopback(&self, listener: SocketAddr) -> Result<(), &'static str> {
        if self.https
            || !listener.ip().is_loopback()
            || self.ip != Some(listener.ip())
            || self.port != listener.port()
        {
            return Err("HTTP origin must be the listener's exact loopback IP literal and port");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plaintext_origin_is_the_exact_bound_ip_and_port_never_a_dns_name() {
        let v4: SocketAddr = "127.0.0.1:4321".parse().unwrap();
        assert!(ConfiguredOrigin::parse("http://127.0.0.1:4321")
            .unwrap()
            .validate_http_loopback(v4)
            .is_ok());
        for origin in [
            "http://localhost:4321",
            "http://LOCALHOST:4321",
            "http://localhost.:4321",
            "http://x.localhost:4321",
            "http://[::1]:4321",
            "http://127.0.0.2:4321",
            "http://127.0.0.1:4322",
            "https://127.0.0.1:4321",
        ] {
            assert!(
                ConfiguredOrigin::parse(origin)
                    .unwrap()
                    .validate_http_loopback(v4)
                    .is_err(),
                "{origin}"
            );
        }
        let v6: SocketAddr = "[::1]:4321".parse().unwrap();
        assert!(ConfiguredOrigin::parse("http://[::1]:4321")
            .unwrap()
            .validate_http_loopback(v6)
            .is_ok());
        assert!(ConfiguredOrigin::parse("http://127.0.0.1:4321")
            .unwrap()
            .validate_http_loopback(v6)
            .is_err());
    }

    #[test]
    fn serialized_origin_matches_browser_ip_and_default_port_canonicalization() {
        for (input, expected) in [
            ("http://127.0.0.1:80/", "http://127.0.0.1"),
            ("https://EXAMPLE.TEST:443/", "https://example.test"),
            ("http://[0:0:0:0:0:0:0:1]:4321", "http://[::1]:4321"),
            ("https://example.test:8443", "https://example.test:8443"),
        ] {
            assert_eq!(ConfiguredOrigin::parse(input).unwrap().as_str(), expected);
        }
    }

    #[test]
    fn origin_has_no_credentials_path_query_fragment_or_invalid_port() {
        for invalid in [
            "/",
            "ftp://example.test",
            "https://user@example.test",
            "https://%6cocalhost",
            "https://example.test/a",
            "https://example.test?x=1",
            "https://example.test/#x",
            "http://127.0.0.1:0",
            "https://example.test:99999",
            "https://example.test:+443",
            "https://example.test:-1",
            "https://example.test:abc",
            "https://example.test:",
            "https://[bad]",
        ] {
            assert!(ConfiguredOrigin::parse(invalid).is_err(), "{invalid}");
        }
    }
}
