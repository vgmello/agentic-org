//! Usage endpoint resolution and strict URL, authority, IP, and header validation.

use super::*;

/// Validated canonical HTTPS authority extracted by [`validate_usage_endpoint_url`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedEndpoint {
    pub host: String,
    pub port: u16,
}

impl ValidatedEndpoint {
    /// Canonical `https://host` (or `https://host:port` for non-443) destination origin.
    pub fn origin(&self) -> String {
        let host_disp = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        if self.port == 443 {
            format!("https://{host_disp}")
        } else {
            format!("https://{host_disp}:{}", self.port)
        }
    }
}

pub(super) fn default_provider_origin(provider: &str) -> Option<&'static str> {
    match canonical_provider_namespace(provider).as_str() {
        "anthropic" => Some("https://api.anthropic.com"),
        "openai" => Some("https://chatgpt.com"),
        _ => None,
    }
}

pub(super) fn transport_error_msg(err: UsageTransportError) -> String {
    match err {
        UsageTransportError::InvalidEndpoint(m)
        | UsageTransportError::InvalidHeader(m)
        | UsageTransportError::Unavailable(m) => m,
        UsageTransportError::DeadlineExceeded => "usage collection deadline exceeded".into(),
    }
}

pub(super) fn resolve_provider_endpoint(
    provider: &str,
    configured: Option<&str>,
) -> Result<(String, Vec<(String, String)>), SubscriptionUsageCollectOutcome> {
    let canonical = canonical_provider_namespace(provider);
    let custom = configured.map(str::trim).filter(|u| !u.is_empty());
    match canonical.as_str() {
        "anthropic" => {
            let url = match custom {
                Some(base) => join_usage_path(base, "/api/oauth/usage")?,
                None => DEFAULT_ANTHROPIC_USAGE_URL.to_owned(),
            };
            Ok((
                url,
                vec![("anthropic-beta".to_owned(), "oauth-2025-04-20".to_owned())],
            ))
        }
        "openai" => {
            let url = match custom {
                Some(base) => join_usage_path(base, "/backend-api/wham/usage")?,
                None => DEFAULT_OPENAI_USAGE_URL.to_owned(),
            };
            Ok((url, Vec::new()))
        }
        "google" => match custom {
            Some(base) => Ok((join_usage_path(base, "/usage")?, Vec::new())),
            None => Err(SubscriptionUsageCollectOutcome::Unsupported {
                issue_code: "PROVIDER_USAGE_API_UNSUPPORTED".into(),
                message: "Google Antigravity subscriptions do not expose a default usage API endpoint; configure an HTTPS usage endpoint or view API-equivalent ledger estimates.".into(),
            }),
        },
        _ => Err(SubscriptionUsageCollectOutcome::Unsupported {
            issue_code: "PROVIDER_UNSUPPORTED".into(),
            message: format!("Provider `{provider}` has no subscription usage collector."),
        }),
    }
}

pub(super) fn resolve_expected_origin(conn: &ProviderConnection) -> Result<String, ApiError> {
    let provider = canonical_provider_namespace(&conn.provider_type);
    let (url, _) = match resolve_provider_endpoint(&provider, conn.endpoint_url.as_deref()) {
        Ok(pair) => pair,
        Err(SubscriptionUsageCollectOutcome::Unavailable { message, .. }) => {
            return Err(ApiError::new(ErrorCode::InvalidRequest, message));
        }
        Err(_) => {
            return Ok(format!("provider:{provider}"));
        }
    };
    let ep = validate_usage_endpoint_url(&url)
        .map_err(|err| ApiError::new(ErrorCode::InvalidRequest, transport_error_msg(err)))?;
    Ok(ep.origin())
}

pub(super) fn join_usage_path(
    base: &str,
    suffix: &str,
) -> Result<String, SubscriptionUsageCollectOutcome> {
    if base.contains('#') {
        return Err(SubscriptionUsageCollectOutcome::Unavailable {
            issue_code: "INVALID_ENDPOINT".into(),
            message: "usage endpoint URL must not include a fragment".into(),
        });
    }
    let (before_query, query) = match base.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (base, None),
    };
    let trimmed = before_query.trim_end_matches('/');
    let joined_path = if trimmed.ends_with(suffix) {
        trimmed.to_owned()
    } else {
        format!("{trimmed}{suffix}")
    };
    Ok(match query {
        Some(q) => format!("{joined_path}?{q}"),
        None => joined_path,
    })
}

pub(super) fn classify_http_and_parse(
    provider: &str,
    resp: HttpUsageResponse,
    token: &str,
    expected_subscription_identity: Option<&str>,
) -> SubscriptionUsageCollectOutcome {
    match resp.status {
        200..=299 => {
            parse_provider_payload(provider, &resp.body, token, expected_subscription_identity)
        }
        401 | 403 => SubscriptionUsageCollectOutcome::AuthRequired {
            issue_code: "AUTH_REJECTED".into(),
            message:
                "Provider rejected subscription usage credentials; re-authentication required."
                    .into(),
        },
        404 | 501 => SubscriptionUsageCollectOutcome::Unsupported {
            issue_code: "USAGE_ENDPOINT_UNSUPPORTED".into(),
            message: "Provider does not expose a subscription usage endpoint for this account."
                .into(),
        },
        429 => SubscriptionUsageCollectOutcome::Unavailable {
            issue_code: "RATE_LIMITED".into(),
            message: "Provider usage endpoint rate-limited the collection request; retry later."
                .into(),
        },
        code => SubscriptionUsageCollectOutcome::Unavailable {
            issue_code: "PROVIDER_UNAVAILABLE".into(),
            message: redact_secrets(
                &format!("Provider usage endpoint returned HTTP {code}."),
                &[token],
            ),
        },
    }
}

/// Validates that a usage API endpoint URL is HTTPS, canonical (no trailing dot so `--resolve`
/// matches the URL host byte-for-byte), non-local, non-private, has no userinfo, has no spaces,
/// quotes, backslashes, or control characters, and rejects non-canonical IPv4/IPv6 literals.
pub fn validate_usage_endpoint_url(raw: &str) -> Result<ValidatedEndpoint, UsageTransportError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 2048 || !is_safe_curl_url_chars(trimmed) {
        return Err(UsageTransportError::InvalidEndpoint(
            "usage endpoint URL is empty, too long, or contains disallowed characters".into(),
        ));
    }
    let Some(rest) = trimmed.strip_prefix("https://") else {
        return Err(UsageTransportError::InvalidEndpoint(
            "usage endpoint URL must use https://".into(),
        ));
    };
    if rest.contains('#') {
        return Err(UsageTransportError::InvalidEndpoint(
            "usage endpoint URL must not include a fragment".into(),
        ));
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return Err(UsageTransportError::InvalidEndpoint(
            "usage endpoint URL must include a host and must not embed credentials".into(),
        ));
    }
    let (host, port) = split_strict_authority(authority)?;
    let host_lower = host.to_ascii_lowercase();
    if host_lower.ends_with('.') || host_lower.starts_with('.') {
        return Err(UsageTransportError::InvalidEndpoint(
            "usage endpoint host must be canonical without leading or trailing dots".into(),
        ));
    }
    if host_lower == "localhost"
        || host_lower.ends_with(".localhost")
        || host_lower.ends_with(".local")
    {
        return Err(UsageTransportError::InvalidEndpoint(
            "usage endpoint URL cannot target localhost or link-local names".into(),
        ));
    }
    validate_host_ip(&host_lower)?;
    Ok(ValidatedEndpoint {
        host: host_lower,
        port,
    })
}

pub(super) fn validate_host_ip(host_lower: &str) -> Result<(), UsageTransportError> {
    if let Ok(ip) = host_lower.parse::<IpAddr>() {
        if is_disallowed_ip(ip) {
            return Err(UsageTransportError::InvalidEndpoint(
                "usage endpoint URL cannot target loopback, private, or link-local addresses"
                    .into(),
            ));
        }
    } else if looks_like_noncanonical_ip_literal(host_lower) {
        return Err(UsageTransportError::InvalidEndpoint(
            "usage endpoint URL cannot use non-canonical numeric IP literals".into(),
        ));
    }
    Ok(())
}

pub(super) fn split_strict_authority(authority: &str) -> Result<(&str, u16), UsageTransportError> {
    if let Some(rest) = authority.strip_prefix('[') {
        let Some(end) = rest.find(']') else {
            return Err(UsageTransportError::InvalidEndpoint(
                "malformed bracketed IPv6 host".into(),
            ));
        };
        let host = &rest[..end];
        let port = parse_optional_port(&rest[end + 1..])?;
        if host.is_empty() || host.contains('%') || host.parse::<std::net::Ipv6Addr>().is_err() {
            return Err(UsageTransportError::InvalidEndpoint(
                "invalid IPv6 host literal".into(),
            ));
        }
        return Ok((host, port));
    }
    if authority.contains('[') || authority.contains(']') {
        return Err(UsageTransportError::InvalidEndpoint(
            "malformed authority brackets".into(),
        ));
    }
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (h, parse_port_digits(p)?),
        None => (authority, 443),
    };
    if host.is_empty()
        || host
            .bytes()
            .any(|b| !(b.is_ascii_alphanumeric() || b == b'-' || b == b'.'))
    {
        return Err(UsageTransportError::InvalidEndpoint(
            "usage endpoint host contains invalid characters".into(),
        ));
    }
    Ok((host, port))
}

pub(super) fn parse_optional_port(after: &str) -> Result<u16, UsageTransportError> {
    if after.is_empty() {
        return Ok(443);
    }
    let Some(digits) = after.strip_prefix(':') else {
        return Err(UsageTransportError::InvalidEndpoint(
            "unexpected trailing characters after host".into(),
        ));
    };
    parse_port_digits(digits)
}

pub(super) fn parse_port_digits(digits: &str) -> Result<u16, UsageTransportError> {
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(UsageTransportError::InvalidEndpoint(
            "invalid port in usage endpoint URL".into(),
        ));
    }
    let port: u16 = digits.parse().map_err(|_| {
        UsageTransportError::InvalidEndpoint("port out of range in usage endpoint URL".into())
    })?;
    if port == 0 {
        return Err(UsageTransportError::InvalidEndpoint(
            "port 0 is invalid in usage endpoint URL".into(),
        ));
    }
    Ok(port)
}

pub(super) fn looks_like_noncanonical_ip_literal(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.is_empty() || parts.iter().any(|p| p.is_empty()) {
        return true;
    }
    parts.iter().all(|part| {
        if let Some(hex) = part.strip_prefix("0x") {
            !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit())
        } else {
            part.bytes().all(|b| b.is_ascii_digit())
        }
    })
}

pub(super) fn is_disallowed_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_disallowed_v4(v4),
        IpAddr::V6(v6) => is_disallowed_v6(v6),
    }
}

pub(super) fn is_disallowed_v4(v4: std::net::Ipv4Addr) -> bool {
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_multicast()
        || v4.is_broadcast()
        || (v4.octets()[0] == 100 && (v4.octets()[1] & 0b1100_0000) == 64)
}

pub(super) fn is_disallowed_v6(v6: std::net::Ipv6Addr) -> bool {
    let segs = v6.segments();
    let ipv4_compat = segs[0..6] == [0; 6] && (segs[6] != 0 || segs[7] > 1);
    let ipv4_compat_disallowed = ipv4_compat && {
        let b6 = segs[6].to_be_bytes();
        let b7 = segs[7].to_be_bytes();
        is_disallowed_v4(std::net::Ipv4Addr::new(b6[0], b6[1], b7[0], b7[1]))
    };
    v6.is_loopback()
        || v6.is_unspecified()
        || v6.is_multicast()
        || (segs[0] & 0xfe00) == 0xfc00
        || (segs[0] & 0xffc0) == 0xfe80
        || ipv4_compat_disallowed
        || v6.to_ipv4_mapped().is_some_and(is_disallowed_v4)
}

pub(super) fn is_valid_bearer_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= MAX_USAGE_SECRET_BYTES
        && token
            .bytes()
            .all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\')
}

pub(super) fn is_safe_header_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub(super) fn is_safe_curl_url_chars(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| (0x21..=0x7e).contains(&b) && b != b'"' && b != b'\\')
}

pub(super) fn is_safe_curl_header_value(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| (0x20..=0x7e).contains(&b) && b != b'"' && b != b'\\')
}
