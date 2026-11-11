use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use hyper::Uri;

/// Public browser origin validated without DNS resolution. This is startup
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
            || !value.is_ascii()
            || value
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control() || byte == b'#')
        {
            return Err("invalid browser origin");
        }
        let uri: Uri = value.parse().map_err(|_| "invalid browser origin")?;
        let https = match uri.scheme_str() {
            Some("https") => true,
            Some("http") => false,
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
            Some(IpAddr::V6(ip)) => serialize_ipv6_host(ip),
            None if !raw_host.starts_with('[') && canonical_domain(bare_host) => {
                bare_host.to_owned()
            }
            None => return Err("invalid browser origin host"),
        };
        let scheme = if https { "https" } else { "http" };
        let suffix = if port == if https { 443 } else { 80 } {
            String::new()
        } else {
            format!(":{port}")
        };
        let serialized = format!("{scheme}://{host}{suffix}");
        if value.strip_suffix('/').unwrap_or(value) != serialized {
            return Err("browser origin must already be canonical");
        }
        Ok(Self {
            serialized,
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

    pub(crate) fn authority(&self) -> &str {
        &self.serialized[if self.https {
            "https://".len()
        } else {
            "http://".len()
        }..]
    }

    pub(crate) fn validate_https(&self, listener: SocketAddr) -> Result<(), &'static str> {
        if !self.is_https() || self.port != listener.port() {
            return Err("HTTPS origin must use HTTPS and the listener's port");
        }
        Ok(())
    }

    pub(crate) fn validate_http_loopback(&self, listener: SocketAddr) -> Result<(), &'static str> {
        if self.https
            || !matches!(
                listener.ip(),
                IpAddr::V4(Ipv4Addr::LOCALHOST) | IpAddr::V6(Ipv6Addr::LOCALHOST)
            )
            || self.ip != Some(listener.ip())
            || self.port != listener.port()
        {
            return Err("HTTP origin must be the listener's exact loopback IP literal and port");
        }
        Ok(())
    }
}

fn canonical_domain(host: &str) -> bool {
    let last = host.rsplit('.').next().unwrap_or_default();
    // Browsers interpret a numeric final label as an IPv4 address. The IpAddr
    // parser already accepted a canonical dotted quad, so aliases fail here.
    host.len() <= 253
        && !last.starts_with("0x")
        && !last.bytes().all(|byte| byte.is_ascii_digit())
        && host.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && label.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'-' | b'_')
                })
        })
}

fn serialize_ipv6_host(ip: Ipv6Addr) -> String {
    let segments = ip.segments();
    let (mut longest_start, mut longest_len) = (0, 0);
    let mut zeroes = 0;
    for (index, piece) in segments.into_iter().enumerate() {
        if piece == 0 {
            zeroes += 1;
            if zeroes > longest_len {
                longest_start = index + 1 - zeroes;
                longest_len = zeroes;
            }
        } else {
            zeroes = 0;
        }
    }
    // WHATWG uses hex for IPv4-mapped addresses, where Rust Display uses a dotted tail.
    let pieces = segments.map(|piece| format!("{piece:x}"));
    if longest_len < 2 {
        format!("[{}]", pieces.join(":"))
    } else {
        format!(
            "[{}::{}]",
            pieces[..longest_start].join(":"),
            pieces[longest_start + longest_len..].join(":"),
        )
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
    fn plaintext_startup_rejects_unapproved_bind_addresses_even_with_matching_origins() {
        for address in ["127.0.0.2:4321", "0.0.0.0:4321"] {
            let origin = ConfiguredOrigin::parse(&format!("http://{address}")).unwrap();
            assert!(
                origin
                    .validate_http_loopback(address.parse().unwrap())
                    .is_err(),
                "{address}"
            );
        }
    }

    #[test]
    fn serialized_origin_already_matches_the_browser_origin() {
        for (input, expected) in [
            ("http://127.0.0.1/", "http://127.0.0.1"),
            ("https://example.test", "https://example.test"),
            ("http://[::1]:4321", "http://[::1]:4321"),
            ("https://[::ffff:7f00:1]", "https://[::ffff:7f00:1]"),
            (
                "https://[2001:db8::1:0:0:1]:8443",
                "https://[2001:db8::1:0:0:1]:8443",
            ),
            (
                "https://xn--bcher-kva.example",
                "https://xn--bcher-kva.example",
            ),
            ("https://a_b.example", "https://a_b.example"),
            ("https://example.test:8443", "https://example.test:8443"),
        ] {
            assert_eq!(ConfiguredOrigin::parse(input).unwrap().as_str(), expected);
        }
    }

    #[test]
    fn noncanonical_origin_spellings_are_refused() {
        for invalid in [
            "HTTP://127.0.0.1",
            "https://EXAMPLE.TEST",
            "https://example.test.",
            "https://example..test",
            "https://a!b.test",
            "https://bücher.example",
            "http://127.0.0.1:80/",
            "https://example.test:443",
            "https://example.test:08443",
            "http://127.1:4321",
            "http://2130706433:4321",
            "http://0x7f.0.0.1:4321",
            "http://0177.0.0.1:4321",
            "http://127.000.000.001:4321",
            "https://foo.123",
            "https://foo.0x",
            "http://[0:0:0:0:0:0:0:1]:4321",
            "https://[2001:DB8::1]",
            "https://[::ffff:127.0.0.1]",
            "https://[2001:db8:0:0:1::1]:8443",
            "https://[fe80::1%25en0]",
        ] {
            assert!(ConfiguredOrigin::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn dns_name_and_label_lengths_match_the_listen_origin_contract() {
        let full_label = "a".repeat(63);
        let origin = |last: usize| {
            format!(
                "https://{full_label}.{full_label}.{full_label}.{}",
                "b".repeat(last)
            )
        };
        assert!(ConfiguredOrigin::parse(&origin(61)).is_ok());
        assert!(ConfiguredOrigin::parse(&origin(62)).is_err());
        assert!(ConfiguredOrigin::parse(&format!("https://{}a.test", full_label)).is_err());
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
            "https://example.test:65536",
            "https://example.test:99999",
            "https://",
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
