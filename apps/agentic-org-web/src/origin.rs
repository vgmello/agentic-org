//! The gateway's use of [`ListenOrigin`] (X-02 §2, §10): the configured browser-facing origin is
//! parsed by `ListenOrigin::parse` and checked against the bound listener by
//! `ListenOrigin::check`, never by rules of the gateway's own. This module only maps a refusal to
//! the gateway's start error and reads the origin's authority for the `Host` comparison.

use std::io;
use std::net::SocketAddr;

use org_host_control::listen::{ListenMode, ListenOrigin, OriginScheme};

/// Refuse to start unless `origin` fits the listener bound at `listener` in `mode`. Runs before
/// any runtime or application initialization. The error carries the [`OriginError`] as its
/// source: `PermissionDenied` for `http_loopback`, `InvalidInput` for `https`.
///
/// [`OriginError`]: org_host_control::listen::OriginError
pub(crate) fn check_listener(
    origin: &ListenOrigin,
    mode: ListenMode,
    listener: SocketAddr,
) -> io::Result<()> {
    origin
        .check(mode, listener.ip(), listener.port())
        .map_err(|error| {
            let kind = match mode {
                ListenMode::HttpLoopback => io::ErrorKind::PermissionDenied,
                ListenMode::Https => io::ErrorKind::InvalidInput,
            };
            io::Error::new(kind, error)
        })
}

/// Whether the origin is `https`.
pub(crate) fn is_https(origin: &ListenOrigin) -> bool {
    origin.scheme() == OriginScheme::Https
}

/// The origin's authority, `host[:port]`: what browsers send in `Host` for it.
pub(crate) fn authority(origin: &ListenOrigin) -> &str {
    let serialized = origin.as_str();
    serialized
        .split_once("://")
        .map_or(serialized, |(_, authority)| authority)
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use org_host_control::listen::OriginError;
    use serde_json::Value;

    use super::*;

    /// X-02 §10: the gateway runs `ListenOrigin`'s vectors through its use of it.
    const VECTORS: &str =
        include_str!("../../../crates/org-host-control/test-vectors/listen-origin.json");

    /// The start error's kind and the `ListenOrigin` refusal it carries.
    fn refusal(result: io::Result<()>) -> Option<(io::ErrorKind, OriginError)> {
        result.err().map(|error| {
            let reason = error
                .get_ref()
                .and_then(|source| source.downcast_ref::<OriginError>())
                .copied()
                .unwrap_or_else(|| panic!("not an origin refusal: {error}"));
            (error.kind(), reason)
        })
    }

    fn loopback(origin: &str, listener: &str) -> io::Result<()> {
        check_listener(
            &ListenOrigin::parse(origin).unwrap(),
            ListenMode::HttpLoopback,
            listener.parse().unwrap(),
        )
    }

    fn https(origin: &str, listener: &str) -> io::Result<()> {
        check_listener(
            &ListenOrigin::parse(origin).unwrap(),
            ListenMode::Https,
            listener.parse().unwrap(),
        )
    }

    #[test]
    fn plaintext_origin_is_the_exact_bound_ip_and_port_never_a_dns_name() {
        use io::ErrorKind::PermissionDenied;
        use OriginError::*;
        assert!(loopback("http://127.0.0.1:4321", "127.0.0.1:4321").is_ok());
        for (origin, reason) in [
            ("http://localhost:4321", NotBoundLiteral),
            ("http://x.localhost:4321", NotBoundLiteral),
            ("http://[::1]:4321", NotBoundLiteral),
            ("http://127.0.0.2:4321", NotBoundLiteral),
            ("http://127.0.0.1:4322", PortMismatch),
            ("https://127.0.0.1:4321", SchemeMismatch),
        ] {
            assert_eq!(
                refusal(loopback(origin, "127.0.0.1:4321")),
                Some((PermissionDenied, reason)),
                "{origin}"
            );
        }
        assert!(loopback("http://[::1]:4321", "[::1]:4321").is_ok());
        assert_eq!(
            refusal(loopback("http://127.0.0.1:4321", "[::1]:4321")),
            Some((PermissionDenied, NotBoundLiteral))
        );
    }

    #[test]
    fn plaintext_startup_rejects_unapproved_bind_addresses_even_with_matching_origins() {
        // B1-05 A-10: the 127.0.0.2 case runs here, on the check function, because macOS
        // configures only 127.0.0.1 on lo0. 0.0.0.0 also runs with a real listener.
        for address in ["127.0.0.2:4321", "0.0.0.0:4321"] {
            assert_eq!(
                refusal(loopback(&format!("http://{address}"), address)),
                Some((
                    io::ErrorKind::PermissionDenied,
                    OriginError::LoopbackAddress
                )),
                "{address}"
            );
        }
    }

    #[test]
    fn https_origin_needs_https_and_the_listener_port_on_any_address() {
        use io::ErrorKind::InvalidInput;
        assert!(https("https://gateway.test:8443", "0.0.0.0:8443").is_ok());
        assert!(https("https://gateway.test", "192.168.1.10:443").is_ok());
        assert_eq!(
            refusal(https("https://gateway.test", "0.0.0.0:8443")),
            Some((InvalidInput, OriginError::PortMismatch))
        );
        assert_eq!(
            refusal(https("http://gateway.test:8443", "0.0.0.0:8443")),
            Some((InvalidInput, OriginError::SchemeMismatch))
        );
    }

    #[test]
    fn listen_origin_vectors_run_through_the_gateway() {
        let doc: Value = serde_json::from_str(VECTORS).unwrap();
        let form = doc["form"].as_array().unwrap();
        assert!(form.len() > 80);
        for row in form {
            let input = row["input"].as_str().unwrap();
            let want = row["result"].as_str().unwrap();
            match ListenOrigin::parse(input) {
                Ok(origin) => {
                    assert_eq!(want, "accepted", "{input:?}");
                    let canonical = row["origin"].as_str().unwrap();
                    assert_eq!(origin.as_str(), canonical, "{input:?}");
                    // The gateway's own reads of it rebuild the same origin.
                    let scheme = if is_https(&origin) { "https" } else { "http" };
                    assert_eq!(
                        format!("{scheme}://{}", authority(&origin)),
                        canonical,
                        "{input:?}"
                    );
                }
                Err(error) => assert_eq!(error.code(), want, "{input:?}"),
            }
        }
        let fit = doc["fit"].as_array().unwrap();
        assert!(fit.len() > 20);
        for row in fit {
            let origin = ListenOrigin::parse(row["origin"].as_str().unwrap()).unwrap();
            let mode: ListenMode = serde_json::from_value(row["scheme"].clone()).unwrap();
            let address: IpAddr = row["address"].as_str().unwrap().parse().unwrap();
            let port = u16::try_from(row["port"].as_u64().unwrap()).unwrap();
            let got = match refusal(check_listener(
                &origin,
                mode,
                SocketAddr::new(address, port),
            )) {
                None => "accepted",
                Some((kind, reason)) => {
                    let start_kind = match mode {
                        ListenMode::HttpLoopback => io::ErrorKind::PermissionDenied,
                        ListenMode::Https => io::ErrorKind::InvalidInput,
                    };
                    assert_eq!(kind, start_kind, "{row}");
                    reason.code()
                }
            };
            assert_eq!(got, row["result"].as_str().unwrap(), "{row}");
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
            assert_eq!(ListenOrigin::parse(input).unwrap().as_str(), expected);
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
            assert!(ListenOrigin::parse(invalid).is_err(), "{invalid}");
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
        assert!(ListenOrigin::parse(&origin(61)).is_ok());
        assert!(ListenOrigin::parse(&origin(62)).is_err());
        assert!(ListenOrigin::parse(&format!("https://{}a.test", full_label)).is_err());
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
            assert!(ListenOrigin::parse(invalid).is_err(), "{invalid}");
        }
    }
}
