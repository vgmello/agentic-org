//! Consumer-owned [`GitHubHttpClient`] port and SSRF-hardened production HTTPS adapter.
//!
//! The production adapter (`SystemGitHubHttpClient`) mirrors `org-subscription-usage`'s hardened
//! `/usr/bin/curl` invocation: `--disable` first, cleared child environment, proxies disabled,
//! redirects disabled (`--no-location`), DNS resolved and validated against the SSRF IP blocklist
//! before pinning via `--resolve`, credentials passed strictly over stdin (`--config -`) in a
//! zeroizing buffer, finite deadlines, and strict `<= 2 MiB` bounded stdout reading.

use std::fmt;
use std::io::{Read, Write};
use std::net::{IpAddr, ToSocketAddrs};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use zeroize::Zeroizing;

use crate::config::{is_disallowed_ip, is_safe_url_chars, ValidatedApiBaseUrl};
use crate::error::GitHubError;

/// Strict maximum HTTP response body size accepted from GitHub APIs (2 MiB).
pub const MAX_GITHUB_HTTP_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

/// Default finite timeout for one GitHub API request.
pub const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(10);

const MAX_DNS_TIMEOUT: Duration = Duration::from_millis(1_500);
const MAX_CONCURRENT_DNS_LOOKUPS: usize = 16;

static ACTIVE_DNS_LOOKUPS: AtomicUsize = AtomicUsize::new(0);

struct DnsPermit;

impl DnsPermit {
    fn try_acquire() -> Option<Self> {
        let mut cur = ACTIVE_DNS_LOOKUPS.load(Ordering::Acquire);
        loop {
            if cur >= MAX_CONCURRENT_DNS_LOOKUPS {
                return None;
            }
            match ACTIVE_DNS_LOOKUPS.compare_exchange_weak(
                cur,
                cur + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(Self),
                Err(next) => cur = next,
            }
        }
    }
}

impl Drop for DnsPermit {
    fn drop(&mut self) {
        ACTIVE_DNS_LOOKUPS.fetch_sub(1, Ordering::Release);
    }
}

/// HTTP verb supported by [`GitHubHttpClient`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}

impl HttpMethod {
    fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

/// Outbound GitHub REST API request. Bearer tokens are zeroized on drop and redacted in `Debug`.
#[derive(Clone)]
pub struct GitHubHttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub bearer_token: Zeroizing<String>,
    pub json_body: Option<String>,
    pub timeout: Duration,
}

impl fmt::Debug for GitHubHttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitHubHttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("bearer_token", &"<redacted>")
            .field("has_body", &self.json_body.is_some())
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Bounded HTTP response returned by [`GitHubHttpClient`].
#[derive(Clone, PartialEq, Eq)]
pub struct GitHubHttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl fmt::Debug for GitHubHttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitHubHttpResponse")
            .field("status", &self.status)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// Consumer-owned HTTPS transport port for GitHub App REST API calls.
pub trait GitHubHttpClient: Send + Sync {
    fn send(&self, req: &GitHubHttpRequest) -> Result<GitHubHttpResponse, GitHubError>;
}

/// Production SSRF-hardened GitHub HTTPS client invoking `/usr/bin/curl`.
#[derive(Debug, Default)]
pub struct SystemGitHubHttpClient;

impl GitHubHttpClient for SystemGitHubHttpClient {
    fn send(&self, req: &GitHubHttpRequest) -> Result<GitHubHttpResponse, GitHubError> {
        let base_origin = extract_origin_for_validation(&req.url)?;
        let validated = ValidatedApiBaseUrl::parse(&base_origin)?;
        validate_bearer_token(req.bearer_token.as_str())?;
        let deadline = Instant::now()
            + req
                .timeout
                .clamp(Duration::from_millis(200), Duration::from_secs(30));
        let pinned_ip = resolve_and_validate_ip(&validated, deadline)?;
        let rem = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or(GitHubError::Timeout)?;
        let max_secs = format!("{:.3}", rem.as_secs_f64().clamp(0.1, 30.0));
        let connect_secs = format!("{:.3}", rem.as_secs_f64().clamp(0.1, 5.0));
        let config_body = build_curl_stdin_config(req, &validated, pinned_ip)?;

        let mut child = Command::new("/usr/bin/curl")
            .env_clear()
            .args([
                "--disable",
                "--silent",
                "--show-error",
                "--globoff",
                "--noproxy",
                "*",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--no-location",
                "--connect-timeout",
                &connect_secs,
                "--max-time",
                &max_secs,
                "--max-filesize",
                &MAX_GITHUB_HTTP_RESPONSE_BYTES.to_string(),
                "--write-out",
                "\n%{http_code}",
                "--config",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| {
                GitHubError::Transport("HTTPS transport executable is unavailable".into())
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            if Instant::now() >= deadline {
                drop(stdin);
                kill_and_reap(&mut child);
                return Err(GitHubError::Timeout);
            }
            if stdin.write_all(config_body.as_bytes()).is_err() {
                drop(stdin);
                kill_and_reap(&mut child);
                return Err(GitHubError::Transport(
                    "failed writing GitHub HTTPS request".into(),
                ));
            }
        }

        let raw_stdout = read_bounded_stdout(&mut child)?;
        let status = child.wait().map_err(|_| {
            GitHubError::Transport("failed waiting for GitHub HTTPS request".into())
        })?;
        if Instant::now() >= deadline {
            return Err(GitHubError::Timeout);
        }
        if !status.success() {
            return Err(GitHubError::Transport(
                "GitHub API request failed or timed out".into(),
            ));
        }
        parse_curl_output(&raw_stdout)
    }
}

fn extract_origin_for_validation(url: &str) -> Result<String, GitHubError> {
    let trimmed = url.trim();
    if trimmed.len() > 2048 || !is_safe_url_chars(trimmed) {
        return Err(GitHubError::InvalidEndpoint(
            "GitHub request URL is too long or contains disallowed characters".into(),
        ));
    }
    let rest = trimmed.strip_prefix("https://").ok_or_else(|| {
        GitHubError::InvalidEndpoint("GitHub request URL must use https://".into())
    })?;
    if rest.contains('#') {
        return Err(GitHubError::InvalidEndpoint(
            "GitHub request URL must not contain fragments".into(),
        ));
    }
    let authority = rest.split(['/', '?']).next().unwrap_or("");
    Ok(format!("https://{authority}"))
}

fn validate_bearer_token(token: &str) -> Result<(), GitHubError> {
    if token.is_empty()
        || token.len() > 8192
        || !token
            .bytes()
            .all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\')
    {
        return Err(GitHubError::InvalidConfig(
            "bearer token contains disallowed header characters".into(),
        ));
    }
    Ok(())
}

fn resolve_and_validate_ip(
    ep: &ValidatedApiBaseUrl,
    deadline: Instant,
) -> Result<IpAddr, GitHubError> {
    if let Ok(ip) = ep.host().parse::<IpAddr>() {
        if is_disallowed_ip(ip) {
            return Err(GitHubError::InvalidEndpoint(
                "GitHub API host cannot target private or loopback IP".into(),
            ));
        }
        return Ok(ip);
    }
    let rem = deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or(GitHubError::Timeout)?;
    let permit = DnsPermit::try_acquire()
        .ok_or_else(|| GitHubError::Transport("DNS resolver concurrency limit reached".into()))?;
    let host = ep.host().to_owned();
    let port = ep.port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _permit = permit;
        let res = (host.as_str(), port)
            .to_socket_addrs()
            .map(|iter| iter.map(|sa| sa.ip()).collect::<Vec<_>>());
        let _ = tx.send(res);
    });
    let addrs = rx
        .recv_timeout(rem.min(MAX_DNS_TIMEOUT))
        .map_err(|_| {
            if Instant::now() >= deadline {
                GitHubError::Timeout
            } else {
                GitHubError::Transport("DNS lookup timed out".into())
            }
        })?
        .map_err(|_| GitHubError::Transport("could not resolve GitHub API host".into()))?;
    if addrs.is_empty() {
        return Err(GitHubError::Transport(
            "GitHub API host resolved to no addresses".into(),
        ));
    }
    if addrs.iter().any(|ip| is_disallowed_ip(*ip)) {
        return Err(GitHubError::InvalidEndpoint(
            "GitHub API host resolved to a disallowed private or loopback IP".into(),
        ));
    }
    Ok(addrs[0])
}

fn escape_curl_value(raw: &str) -> Result<String, GitHubError> {
    if raw.chars().any(char::is_control) {
        return Err(GitHubError::InvalidConfig(
            "request payload contains control characters".into(),
        ));
    }
    let mut out = String::with_capacity(raw.len() + 8);
    for ch in raw.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c => out.push(c),
        }
    }
    Ok(out)
}

fn build_curl_stdin_config(
    req: &GitHubHttpRequest,
    ep: &ValidatedApiBaseUrl,
    pinned_ip: IpAddr,
) -> Result<Zeroizing<String>, GitHubError> {
    let mut cfg = Zeroizing::new(String::with_capacity(512));
    cfg.push_str("request = \"");
    cfg.push_str(req.method.as_str());
    cfg.push_str("\"\nurl = \"");
    cfg.push_str(req.url.trim());
    cfg.push_str("\"\n");
    if ep.host().parse::<IpAddr>().is_err() {
        let ip_str = match pinned_ip {
            IpAddr::V4(v4) => v4.to_string(),
            IpAddr::V6(v6) => format!("[{v6}]"),
        };
        cfg.push_str(&format!(
            "resolve = \"{}:{}:{}\"\n",
            ep.host(),
            ep.port(),
            ip_str
        ));
    }
    cfg.push_str("header = \"Authorization: Bearer ");
    cfg.push_str(req.bearer_token.as_str());
    cfg.push_str("\"\n");
    cfg.push_str("header = \"Accept: application/vnd.github+json\"\n");
    cfg.push_str("header = \"X-GitHub-Api-Version: 2022-11-28\"\n");
    cfg.push_str("header = \"User-Agent: agentic-org-github/1\"\n");
    if let Some(body) = req.json_body.as_deref() {
        let escaped = escape_curl_value(body)?;
        cfg.push_str("header = \"Content-Type: application/json\"\n");
        cfg.push_str("data-raw = \"");
        cfg.push_str(&escaped);
        cfg.push_str("\"\n");
    }
    Ok(cfg)
}

fn kill_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn read_bounded_stdout(child: &mut Child) -> Result<Vec<u8>, GitHubError> {
    let Some(stdout) = child.stdout.take() else {
        kill_and_reap(child);
        return Err(GitHubError::Transport(
            "transport stdout unavailable".into(),
        ));
    };
    let cap = MAX_GITHUB_HTTP_RESPONSE_BYTES + 64;
    let mut buf = Vec::new();
    if stdout.take((cap + 1) as u64).read_to_end(&mut buf).is_err() {
        kill_and_reap(child);
        return Err(GitHubError::Transport(
            "failed reading GitHub API response".into(),
        ));
    }
    if buf.len() > cap {
        kill_and_reap(child);
        return Err(GitHubError::Transport(
            "GitHub API response exceeded maximum 2 MiB size limit".into(),
        ));
    }
    Ok(buf)
}

pub(crate) fn parse_curl_output(raw: &[u8]) -> Result<GitHubHttpResponse, GitHubError> {
    let split = raw
        .iter()
        .rposition(|&b| b == b'\n')
        .ok_or_else(|| GitHubError::Transport("malformed HTTPS transport response".into()))?;
    let status_str = std::str::from_utf8(&raw[split + 1..])
        .map_err(|_| GitHubError::Transport("invalid HTTP status from transport".into()))?;
    let status: u16 = status_str
        .trim()
        .parse()
        .map_err(|_| GitHubError::Transport("unparseable HTTP status from transport".into()))?;
    let body = raw[..split].to_vec();
    if body.len() > MAX_GITHUB_HTTP_RESPONSE_BYTES {
        return Err(GitHubError::Transport(
            "GitHub API response exceeded maximum 2 MiB size limit".into(),
        ));
    }
    Ok(GitHubHttpResponse { status, body })
}
