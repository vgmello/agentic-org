//! SSRF-hardened GitHub API endpoint validation, repository slug validation, and App config types.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::error::GitHubError;

/// Default public GitHub REST API base URL.
pub const DEFAULT_GITHUB_API_BASE_URL: &str = "https://api.github.com";

const MAX_URL_BYTES: usize = 512;
const MAX_ACCOUNT_HINT_BYTES: usize = 128;

/// Validated canonical GitHub REST API base URL (HTTPS-only, SSRF-hardened).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedApiBaseUrl {
    host: String,
    port: u16,
    path_prefix: String,
    canonical: String,
}

impl ValidatedApiBaseUrl {
    /// Parses and validates an HTTPS GitHub API base URL (defaulting to `https://api.github.com`
    /// when `raw` is `None` or blank).
    ///
    /// Refuses plaintext HTTP, embedded credentials (`@`), queries, fragments, non-canonical IP
    /// literals, localhost, link-local, RFC1918 private, CGNAT, ULA, multicast, and unspecified
    /// addresses.
    pub fn parse_or_default(raw: Option<&str>) -> Result<Self, GitHubError> {
        let target = raw
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_GITHUB_API_BASE_URL);
        Self::parse(target)
    }

    /// Parses and validates an explicit HTTPS GitHub API base URL.
    pub fn parse(raw: &str) -> Result<Self, GitHubError> {
        if raw.chars().any(char::is_control) {
            return Err(GitHubError::InvalidEndpoint(
                "GitHub API base URL must not contain control characters".into(),
            ));
        }
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.len() > MAX_URL_BYTES || !is_safe_url_chars(trimmed) {
            return Err(GitHubError::InvalidEndpoint(
                "GitHub API base URL is empty, too long, or contains disallowed characters".into(),
            ));
        }
        let Some(rest) = trimmed.strip_prefix("https://") else {
            return Err(GitHubError::InvalidEndpoint(
                "GitHub API base URL must use https://".into(),
            ));
        };
        if rest.contains('?') || rest.contains('#') {
            return Err(GitHubError::InvalidEndpoint(
                "GitHub API base URL must not include query parameters or fragments".into(),
            ));
        }
        let (authority, raw_path) = match rest.find('/') {
            Some(idx) => (&rest[..idx], &rest[idx..]),
            None => (rest, ""),
        };
        if authority.is_empty() || authority.contains('@') {
            return Err(GitHubError::InvalidEndpoint(
                "GitHub API base URL must include a host and must not embed credentials".into(),
            ));
        }
        let (host_raw, port) = split_strict_authority(authority)?;
        let host = host_raw.to_ascii_lowercase();
        validate_public_host(&host)?;
        let path_prefix = validate_path_prefix(raw_path)?;
        let host_disp = if host.contains(':') {
            format!("[{host}]")
        } else {
            host.clone()
        };
        let canonical = if port == 443 {
            format!("https://{host_disp}{path_prefix}")
        } else {
            format!("https://{host_disp}:{port}{path_prefix}")
        };
        Ok(Self {
            host,
            port,
            path_prefix,
            canonical,
        })
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn as_str(&self) -> &str {
        &self.canonical
    }

    /// Joins an API endpoint path starting with `/` (e.g. `/app/installations/1/access_tokens`).
    pub fn endpoint(&self, api_path: &str) -> Result<String, GitHubError> {
        if !api_path.starts_with('/') || !is_safe_url_chars(api_path) {
            return Err(GitHubError::InvalidEndpoint(
                "GitHub API endpoint path is malformed".into(),
            ));
        }
        Ok(format!("{}{}", self.canonical, api_path))
    }
}

/// Strictly validated GitHub repository `owner/repo` slug (zero traversal or control characters).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedRepoSlug {
    owner: String,
    repo: String,
}

impl ValidatedRepoSlug {
    pub fn new(owner: &str, repo: &str) -> Result<Self, GitHubError> {
        if owner.chars().any(char::is_control) || repo.chars().any(char::is_control) {
            return Err(GitHubError::InvalidConfig(
                "repository slug must not contain control characters".into(),
            ));
        }
        let owner = owner.trim();
        let repo = repo.trim();
        validate_owner_segment(owner)?;
        validate_repo_segment(repo)?;
        Ok(Self {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
        })
    }

    /// Parses `"owner/repo"` into a validated slug.
    pub fn parse_full_name(full_name: &str) -> Result<Self, GitHubError> {
        if full_name.chars().any(char::is_control) {
            return Err(GitHubError::InvalidConfig(
                "repository slug must not contain control characters".into(),
            ));
        }
        let trimmed = full_name.trim();
        let (owner, repo) = trimmed.split_once('/').ok_or_else(|| {
            GitHubError::InvalidConfig("repository slug must have the form `owner/repo`".into())
        })?;
        if repo.contains('/') {
            return Err(GitHubError::InvalidConfig(
                "repository slug must contain exactly one `/`".into(),
            ));
        }
        Self::new(owner, repo)
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn repo(&self) -> &str {
        &self.repo
    }

    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }

    /// Case-insensitive GitHub `owner/repo` equality.
    pub fn matches_full_name(&self, other: &str) -> bool {
        self.full_name().eq_ignore_ascii_case(other.trim())
    }
}

/// Validated non-secret GitHub App installation metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubAppConfig {
    pub app_id: u64,
    pub installation_id: u64,
    pub api_base_url: ValidatedApiBaseUrl,
    pub account_hint: Option<String>,
}

impl GitHubAppConfig {
    pub fn new(
        app_id: u64,
        installation_id: u64,
        api_base_url: Option<&str>,
        account_hint: Option<&str>,
    ) -> Result<Self, GitHubError> {
        if app_id == 0 {
            return Err(GitHubError::InvalidConfig(
                "app_id must be a positive integer".into(),
            ));
        }
        if installation_id == 0 {
            return Err(GitHubError::InvalidConfig(
                "installation_id must be a positive integer".into(),
            ));
        }
        let api_base_url = ValidatedApiBaseUrl::parse_or_default(api_base_url)?;
        let account_hint = validate_account_hint(account_hint)?;
        Ok(Self {
            app_id,
            installation_id,
            api_base_url,
            account_hint,
        })
    }
}

/// Validates an optional non-secret account hint label.
pub fn validate_account_hint(raw: Option<&str>) -> Result<Option<String>, GitHubError> {
    let Some(value) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if value.len() > MAX_ACCOUNT_HINT_BYTES || value.chars().any(char::is_control) {
        return Err(GitHubError::InvalidConfig(
            "account_hint must be at most 128 bytes with no control characters".into(),
        ));
    }
    Ok(Some(value.to_owned()))
}

/// Validates a GitHub-originated HTML/diff/details URL: strictly `https://`, no userinfo, no
/// control chars, bounded length. Returns `None` for any disallowed scheme (`javascript:`, `http:`).
pub fn validate_html_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 2048 || !is_safe_url_chars(trimmed) {
        return None;
    }
    let rest = trimmed.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let (host, _) = split_strict_authority(authority).ok()?;
    let lower = host.to_ascii_lowercase();
    validate_public_host(&lower).ok()?;
    Some(trimmed.to_owned())
}

fn validate_owner_segment(owner: &str) -> Result<(), GitHubError> {
    let valid = !owner.is_empty()
        && owner.len() <= 39
        && !owner.starts_with('-')
        && !owner.ends_with('-')
        && !owner.contains("--")
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if !valid {
        return Err(GitHubError::InvalidConfig(
            "GitHub owner must be 1..=39 ASCII alphanumerics or single hyphens".into(),
        ));
    }
    Ok(())
}

fn validate_repo_segment(repo: &str) -> Result<(), GitHubError> {
    let valid = !repo.is_empty()
        && repo.len() <= 100
        && repo != "."
        && repo != ".."
        && !repo.contains("..")
        && !repo.starts_with('.')
        && !repo.ends_with('.')
        && repo
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if !valid {
        return Err(GitHubError::InvalidConfig(
            "GitHub repo must be 1..=100 safe ASCII characters with no traversal".into(),
        ));
    }
    Ok(())
}

fn validate_path_prefix(raw_path: &str) -> Result<String, GitHubError> {
    if raw_path.is_empty() || raw_path == "/" {
        return Ok(String::new());
    }
    let trimmed = raw_path.trim_end_matches('/');
    if trimmed.contains("..") || trimmed.contains("//") || trimmed.contains('%') {
        return Err(GitHubError::InvalidEndpoint(
            "GitHub API base URL path prefix cannot contain traversal or encoded segments".into(),
        ));
    }
    Ok(trimmed.to_owned())
}

fn validate_public_host(host_lower: &str) -> Result<(), GitHubError> {
    if host_lower.is_empty() || host_lower.starts_with('.') || host_lower.ends_with('.') {
        return Err(GitHubError::InvalidEndpoint(
            "GitHub API host must be canonical without leading or trailing dots".into(),
        ));
    }
    if host_lower == "localhost"
        || host_lower.ends_with(".localhost")
        || host_lower.ends_with(".local")
        || host_lower.ends_with(".internal")
    {
        return Err(GitHubError::InvalidEndpoint(
            "GitHub API base URL cannot target localhost or internal hostnames".into(),
        ));
    }
    if let Ok(ip) = host_lower.parse::<IpAddr>() {
        if is_disallowed_ip(ip) {
            return Err(GitHubError::InvalidEndpoint(
                "GitHub API base URL cannot target loopback, private, or link-local addresses"
                    .into(),
            ));
        }
    } else if looks_like_noncanonical_ip_literal(host_lower) {
        return Err(GitHubError::InvalidEndpoint(
            "GitHub API base URL cannot use non-canonical numeric IP literals".into(),
        ));
    }
    Ok(())
}

fn split_strict_authority(authority: &str) -> Result<(&str, u16), GitHubError> {
    if let Some(rest) = authority.strip_prefix('[') {
        let end = rest
            .find(']')
            .ok_or_else(|| GitHubError::InvalidEndpoint("malformed bracketed IPv6 host".into()))?;
        let host = &rest[..end];
        let port = parse_optional_port(&rest[end + 1..])?;
        if host.is_empty() || host.contains('%') || host.parse::<Ipv6Addr>().is_err() {
            return Err(GitHubError::InvalidEndpoint(
                "invalid IPv6 host literal".into(),
            ));
        }
        return Ok((host, port));
    }
    if authority.contains('[') || authority.contains(']') {
        return Err(GitHubError::InvalidEndpoint(
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
        return Err(GitHubError::InvalidEndpoint(
            "GitHub API host contains invalid characters".into(),
        ));
    }
    Ok((host, port))
}

fn parse_optional_port(after: &str) -> Result<u16, GitHubError> {
    if after.is_empty() {
        return Ok(443);
    }
    let digits = after.strip_prefix(':').ok_or_else(|| {
        GitHubError::InvalidEndpoint("unexpected trailing characters after host".into())
    })?;
    parse_port_digits(digits)
}

fn parse_port_digits(digits: &str) -> Result<u16, GitHubError> {
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(GitHubError::InvalidEndpoint(
            "invalid port in GitHub API URL".into(),
        ));
    }
    let port: u16 = digits
        .parse()
        .map_err(|_| GitHubError::InvalidEndpoint("port out of range in GitHub API URL".into()))?;
    if port == 0 {
        return Err(GitHubError::InvalidEndpoint(
            "port 0 is invalid in GitHub API URL".into(),
        ));
    }
    Ok(port)
}

fn looks_like_noncanonical_ip_literal(host: &str) -> bool {
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

/// Returns `true` when `ip` is loopback, RFC1918 private, link-local, CGNAT, ULA, unspecified,
/// multicast, broadcast, or documentation/special-use (SSRF blocklist).
pub fn is_disallowed_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_disallowed_v4(v4),
        IpAddr::V6(v6) => is_disallowed_v6(v6),
    }
}

fn is_disallowed_v4(v4: Ipv4Addr) -> bool {
    let [a, b, c, _] = v4.octets();
    let special = matches!(
        (a, b, c),
        (0, _, _)
            | (100, 64..=127, _)
            | (192, 0, 0)
            | (192, 0, 2)
            | (198, 18..=19, _)
            | (198, 51, 100)
            | (203, 0, 113)
            | (240..=255, _, _)
    );
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_multicast()
        || v4.is_broadcast()
        || special
}

fn is_disallowed_v6(v6: Ipv6Addr) -> bool {
    let segs = v6.segments();
    v6.is_loopback()
        || v6.is_unspecified()
        || v6.is_multicast()
        || is_disallowed_v6_prefix(segs)
        || is_disallowed_v6_embedded_v4(v6, segs)
}

fn is_disallowed_v6_prefix(segs: [u16; 8]) -> bool {
    let unique_local = (segs[0] & 0xfe00) == 0xfc00;
    let link_local = (segs[0] & 0xffc0) == 0xfe80;
    let deprecated_site_local = (segs[0] & 0xffc0) == 0xfec0;
    let nat64 = segs[0..6] == [0x0064, 0xff9b, 0, 0, 0, 0] || segs[0..3] == [0x0064, 0xff9b, 1];
    let discard = segs[0..4] == [0x0100, 0, 0, 0];
    let ipv4_translated = segs[0..6] == [0, 0, 0, 0, 0xffff, 0];
    let six_to_four = segs[0] == 0x2002;
    let teredo = segs[0] == 0x2001 && segs[1] == 0x0000;
    let documentation = segs[0] == 0x2001 && segs[1] == 0x0db8;
    unique_local
        || link_local
        || deprecated_site_local
        || nat64
        || discard
        || ipv4_translated
        || six_to_four
        || teredo
        || documentation
}

fn is_disallowed_v6_embedded_v4(v6: Ipv6Addr, segs: [u16; 8]) -> bool {
    if v6.to_ipv4_mapped().is_some_and(is_disallowed_v4) {
        return true;
    }
    let ipv4_compat = segs[0..6] == [0; 6] && (segs[6] != 0 || segs[7] > 1);
    if !ipv4_compat {
        return false;
    }
    let b6 = segs[6].to_be_bytes();
    let b7 = segs[7].to_be_bytes();
    is_disallowed_v4(Ipv4Addr::new(b6[0], b6[1], b7[0], b7[1]))
}

pub(crate) fn is_safe_url_chars(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| (0x21..=0x7e).contains(&b) && b != b'"' && b != b'\\')
}
