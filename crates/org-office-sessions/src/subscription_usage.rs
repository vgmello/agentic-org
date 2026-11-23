//! Authenticated provider subscription model usage collection (`TASK-79`).
//!
//! Keeps all provider/user usage-API credentials private to office storage via [`CredentialVault`]
//! (outside shared organization Git), binds credentials to explicit HTTPS destination origins,
//! isolates `curl` (`--disable`, `--noproxy '*'`, `--globoff`, cleared env, pinned `--resolve`),
//! enforces a single shared deadline inside the 5s office bridge limit, fences concurrent writes
//! by monotonic attempt generation, and delegates external collection behind consumer-owned ports
//! ([`SubscriptionUsageCollector`] and [`SubscriptionUsageTransport`]).

use std::collections::BTreeMap;
use std::fmt;
use std::io::{Read, Write};
use std::net::{IpAddr, ToSocketAddrs};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use org_office::accounting::{
    build_snapshot_view, canonical_provider_namespace, StoredSubscriptionUsageSnapshot,
    SubscriptionCredentialPresence, SUBSCRIPTION_USAGE_FRESHNESS_MS,
};
use org_office::organization_family::records::{ProviderAuthMode, ProviderConnection};
use org_office::Office;
use org_proto::costs::{
    CostSubscriptionUsageCollectParams, CostSubscriptionUsageCollectResult,
    CostSubscriptionUsageCredentialRevokeParams, CostSubscriptionUsageCredentialRevokeResult,
    CostSubscriptionUsageCredentialSetParams, CostSubscriptionUsageCredentialSetResult,
    CostSubscriptionUsageListParams, CostSubscriptionUsageListResult, SubscriptionModelUsageView,
    SubscriptionUsageAuthSource, SubscriptionUsageSnapshotView, SubscriptionUsageStatus,
};
use org_proto::{ApiError, ErrorCode};
use zeroize::Zeroizing;

use crate::credentials::{
    redact_secrets, CredentialVault, EnrollmentVaultError, UnwrappedCredential,
};
use crate::subscription_usage_parse::parse_provider_payload;

const MAX_USAGE_SECRET_BYTES: usize = 4096;
const MAX_USAGE_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_COLLECT_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_ACCOUNT_HINT_BYTES: usize = 256;
const MAX_CONCURRENT_COLLECTORS: usize = 8;
const MAX_CONCURRENT_DNS_LOOKUPS: usize = 16;
const TOTAL_COLLECTION_BUDGET: Duration = Duration::from_millis(3_000);
const MAX_DNS_TIMEOUT: Duration = Duration::from_millis(1_000);

static ACTIVE_DNS_LOOKUPS: AtomicUsize = AtomicUsize::new(0);
static ATTEMPT_SCHEDULE_SEQ: AtomicU64 = AtomicU64::new(1);

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

fn attempt_schedule_map() -> &'static Mutex<BTreeMap<String, u64>> {
    static MAP: OnceLock<Mutex<BTreeMap<String, u64>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn record_scheduling_attempt(connection_id: &str) {
    let seq = ATTEMPT_SCHEDULE_SEQ.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut map) = attempt_schedule_map().lock() {
        if map.len() > 2048 {
            map.clear();
        }
        map.insert(connection_id.to_owned(), seq);
    }
}
const DEFAULT_ANTHROPIC_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const DEFAULT_OPENAI_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

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

/// Raw HTTP JSON response returned by a [`SubscriptionUsageTransport`].
#[derive(Clone, PartialEq, Eq)]
pub struct HttpUsageResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl fmt::Debug for HttpUsageResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpUsageResponse")
            .field("status", &self.status)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// Low-level transport failure before receiving an HTTP response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageTransportError {
    InvalidEndpoint(String),
    InvalidHeader(String),
    DeadlineExceeded,
    Unavailable(String),
}

/// Consumer-owned HTTPS GET transport port for provider subscription usage APIs (`AGENTS.md` Rule 1).
pub trait SubscriptionUsageTransport: Send + Sync {
    fn get_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        deadline: Instant,
    ) -> Result<HttpUsageResponse, UsageTransportError>;
}

/// Production HTTPS transport invoking `/usr/bin/curl` with `--disable` first, cleared child env,
/// proxies disabled, globbing disabled, credentials passed ONLY over stdin (`--config -`),
/// bounded DNS resolution + pinned `--resolve`, single shared deadline, and bounded stdout reading.
#[derive(Debug, Default)]
pub struct SystemHttpsUsageTransport;

impl SubscriptionUsageTransport for SystemHttpsUsageTransport {
    fn get_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        deadline: Instant,
    ) -> Result<HttpUsageResponse, UsageTransportError> {
        let ep = validate_usage_endpoint_url(url)?;
        let pinned_ip = resolve_and_validate_ip(&ep, deadline)?;
        let rem = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or(UsageTransportError::DeadlineExceeded)?;
        let max_secs = format!("{:.3}", rem.as_secs_f64().clamp(0.05, 3.0));
        let connect_secs = format!("{:.3}", rem.as_secs_f64().clamp(0.05, 2.0));
        let config_body = build_curl_stdin_config(url, &ep, pinned_ip, headers)?;
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
                &MAX_USAGE_RESPONSE_BYTES.to_string(),
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
                UsageTransportError::Unavailable("HTTPS transport executable is unavailable".into())
            })?;
        if let Some(mut stdin) = child.stdin.take() {
            if Instant::now() >= deadline {
                drop(stdin);
                kill_and_reap(&mut child);
                return Err(UsageTransportError::DeadlineExceeded);
            }
            if stdin.write_all(config_body.as_bytes()).is_err() {
                drop(stdin);
                kill_and_reap(&mut child);
                return Err(UsageTransportError::Unavailable(
                    "failed writing transport request".into(),
                ));
            }
        }
        let raw_stdout = read_bounded_stdout(&mut child)?;
        let status = child.wait().map_err(|_| {
            UsageTransportError::Unavailable("failed waiting for HTTPS usage request".into())
        })?;
        if Instant::now() >= deadline {
            return Err(UsageTransportError::DeadlineExceeded);
        }
        if !status.success() {
            return Err(UsageTransportError::Unavailable(
                "provider usage endpoint request failed or timed out".into(),
            ));
        }
        parse_curl_output(&raw_stdout)
    }
}

fn kill_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn read_bounded_stdout(child: &mut Child) -> Result<Vec<u8>, UsageTransportError> {
    let Some(stdout) = child.stdout.take() else {
        kill_and_reap(child);
        return Err(UsageTransportError::Unavailable(
            "transport stdout unavailable".into(),
        ));
    };
    let cap = MAX_USAGE_RESPONSE_BYTES + 64;
    let mut buf = Vec::new();
    if stdout.take((cap + 1) as u64).read_to_end(&mut buf).is_err() {
        kill_and_reap(child);
        return Err(UsageTransportError::Unavailable(
            "failed reading provider usage response".into(),
        ));
    }
    if buf.len() > cap {
        kill_and_reap(child);
        return Err(UsageTransportError::Unavailable(
            "provider usage response exceeded maximum size".into(),
        ));
    }
    Ok(buf)
}

fn resolve_and_validate_ip(
    ep: &ValidatedEndpoint,
    deadline: Instant,
) -> Result<IpAddr, UsageTransportError> {
    if let Ok(ip) = ep.host.parse::<IpAddr>() {
        if is_disallowed_ip(ip) {
            return Err(UsageTransportError::InvalidEndpoint(
                "usage endpoint URL cannot target loopback, private, or link-local addresses"
                    .into(),
            ));
        }
        return Ok(ip);
    }
    let rem = deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or(UsageTransportError::DeadlineExceeded)?;
    let permit = DnsPermit::try_acquire().ok_or_else(|| {
        UsageTransportError::Unavailable("DNS resolver concurrency limit reached".into())
    })?;
    let host = ep.host.clone();
    let port = ep.port;
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
                UsageTransportError::DeadlineExceeded
            } else {
                UsageTransportError::Unavailable("DNS lookup timed out".into())
            }
        })?
        .map_err(|_| {
            UsageTransportError::Unavailable(
                "could not resolve provider usage endpoint host".into(),
            )
        })?;
    if addrs.is_empty() {
        return Err(UsageTransportError::Unavailable(
            "provider usage endpoint host resolved to no addresses".into(),
        ));
    }
    if addrs.iter().any(|ip| is_disallowed_ip(*ip)) {
        return Err(UsageTransportError::InvalidEndpoint(
            "usage endpoint host resolves to a disallowed private or loopback IP".into(),
        ));
    }
    Ok(addrs[0])
}

fn build_curl_stdin_config(
    url: &str,
    ep: &ValidatedEndpoint,
    pinned_ip: IpAddr,
    headers: &[(String, String)],
) -> Result<Zeroizing<String>, UsageTransportError> {
    let mut cfg = Zeroizing::new(String::with_capacity(320));
    if !is_safe_curl_url_chars(url) {
        return Err(UsageTransportError::InvalidEndpoint(
            "usage endpoint URL contains disallowed characters".into(),
        ));
    }
    cfg.push_str("url = \"");
    cfg.push_str(url);
    cfg.push_str("\"\n");
    // Omit --resolve for validated IP literals (curl connects directly; unbracketed IPv6 breaks --resolve).
    if ep.host.parse::<IpAddr>().is_err() {
        let ip_str = match pinned_ip {
            IpAddr::V4(v4) => v4.to_string(),
            IpAddr::V6(v6) => format!("[{v6}]"),
        };
        cfg.push_str(&format!(
            "resolve = \"{}:{}:{}\"\n",
            ep.host, ep.port, ip_str
        ));
    }
    for (name, value) in headers {
        if !is_safe_header_name(name) || !is_safe_curl_header_value(value) {
            return Err(UsageTransportError::InvalidHeader(
                "request header contains disallowed characters".into(),
            ));
        }
        cfg.push_str("header = \"");
        cfg.push_str(name);
        cfg.push_str(": ");
        cfg.push_str(value);
        cfg.push_str("\"\n");
    }
    Ok(cfg)
}

fn parse_curl_output(raw: &[u8]) -> Result<HttpUsageResponse, UsageTransportError> {
    let split = raw.iter().rposition(|&b| b == b'\n').ok_or_else(|| {
        UsageTransportError::Unavailable("malformed HTTPS transport response".into())
    })?;
    let status_str = std::str::from_utf8(&raw[split + 1..]).map_err(|_| {
        UsageTransportError::Unavailable("invalid HTTP status from transport".into())
    })?;
    let status: u16 = status_str.trim().parse().map_err(|_| {
        UsageTransportError::Unavailable("unparseable HTTP status from transport".into())
    })?;
    let body = raw[..split].to_vec();
    if body.len() > MAX_USAGE_RESPONSE_BYTES {
        return Err(UsageTransportError::Unavailable(
            "provider usage response exceeded maximum size".into(),
        ));
    }
    Ok(HttpUsageResponse { status, body })
}

/// Authenticated collection input passed to [`SubscriptionUsageCollector`].
pub struct SubscriptionUsageCollectRequest {
    pub connection_id: String,
    pub provider_type: String,
    pub subscription_identity: Option<String>,
    pub endpoint_url: Option<String>,
    pub auth_source: SubscriptionUsageAuthSource,
    pub credential: Zeroizing<Vec<u8>>,
    pub credential_deadline: Option<Instant>,
    pub time_zone: String,
    pub collected_at: String,
    pub now_ms: u64,
    pub deadline: Instant,
}

/// Structured outcome of collecting subscription model usage for one provider connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscriptionUsageCollectOutcome {
    Collected {
        models: Vec<SubscriptionModelUsageView>,
        message: Option<String>,
    },
    AuthRequired {
        issue_code: String,
        message: String,
    },
    Unavailable {
        issue_code: String,
        message: String,
    },
    Unsupported {
        issue_code: String,
        message: String,
    },
    /// Batch deadline expired before provider response completed; do not overwrite stored snapshot.
    Deferred,
}

/// Consumer-owned collector port for provider subscription model usage (`TASK-79`).
pub trait SubscriptionUsageCollector: Send + Sync {
    fn collect(&self, req: &SubscriptionUsageCollectRequest) -> SubscriptionUsageCollectOutcome;
}

/// Production provider subscription usage collector backed by a pluggable [`SubscriptionUsageTransport`].
pub struct ProviderSubscriptionUsageCollector {
    transport: Arc<dyn SubscriptionUsageTransport>,
}

impl ProviderSubscriptionUsageCollector {
    pub fn production() -> Self {
        Self::with_transport(Arc::new(SystemHttpsUsageTransport))
    }

    pub fn with_transport(transport: Arc<dyn SubscriptionUsageTransport>) -> Self {
        Self { transport }
    }
}

impl SubscriptionUsageCollector for ProviderSubscriptionUsageCollector {
    fn collect(&self, req: &SubscriptionUsageCollectRequest) -> SubscriptionUsageCollectOutcome {
        let provider = canonical_provider_namespace(&req.provider_type);
        let token = match std::str::from_utf8(&req.credential) {
            Ok(s) if is_valid_bearer_token(s.trim()) => s.trim(),
            _ => {
                return SubscriptionUsageCollectOutcome::AuthRequired {
                    issue_code: "INVALID_CREDENTIAL_FORMAT".into(),
                    message: "Stored credential cannot be encoded as an HTTPS bearer token; re-authenticate.".into(),
                };
            }
        };
        let (url, extra_headers) =
            match resolve_provider_endpoint(&provider, req.endpoint_url.as_deref()) {
                Ok(pair) => pair,
                Err(outcome) => return outcome,
            };
        let ep = match validate_usage_endpoint_url(&url) {
            Ok(ep) => ep,
            Err(err) => {
                return SubscriptionUsageCollectOutcome::Unavailable {
                    issue_code: "INVALID_ENDPOINT".into(),
                    message: redact_secrets(&transport_error_msg(err), &[token]),
                };
            }
        };
        if req.auth_source == SubscriptionUsageAuthSource::EnrolledSubscriptionCredential
            && default_provider_origin(&provider) != Some(ep.origin().as_str())
        {
            return SubscriptionUsageCollectOutcome::AuthRequired {
                issue_code: "ENDPOINT_CREDENTIAL_MISMATCH".into(),
                message: "Enrolled subscription credentials are only sent to the official provider origin; configure a dedicated usage credential for custom endpoints.".into(),
            };
        }
        let cred_deadline = req.credential_deadline;
        if cred_deadline.is_some_and(|cd| Instant::now() >= cd) {
            return SubscriptionUsageCollectOutcome::AuthRequired {
                issue_code: "CREDENTIAL_EXPIRED".into(),
                message: "Subscription usage credential expired before dispatch; re-authenticate."
                    .into(),
            };
        }
        let effective_deadline = cred_deadline.map_or(req.deadline, |cd| req.deadline.min(cd));
        let mut headers = vec![
            ("Authorization".to_owned(), format!("Bearer {token}")),
            ("Accept".to_owned(), "application/json".to_owned()),
        ];
        headers.extend(extra_headers);
        let resp = match self.transport.get_json(&url, &headers, effective_deadline) {
            Ok(r) => r,
            Err(UsageTransportError::InvalidHeader(_)) => {
                return SubscriptionUsageCollectOutcome::AuthRequired {
                    issue_code: "INVALID_CREDENTIAL_FORMAT".into(),
                    message: "Stored credential contains disallowed header characters.".into(),
                };
            }
            Err(UsageTransportError::InvalidEndpoint(msg)) => {
                return SubscriptionUsageCollectOutcome::Unavailable {
                    issue_code: "INVALID_ENDPOINT".into(),
                    message: redact_secrets(&msg, &[token]),
                };
            }
            Err(UsageTransportError::DeadlineExceeded) => {
                if cred_deadline.is_some_and(|cd| Instant::now() >= cd) {
                    return SubscriptionUsageCollectOutcome::AuthRequired {
                        issue_code: "CREDENTIAL_EXPIRED".into(),
                        message: "Subscription usage credential expired before dispatch; re-authenticate.".into(),
                    };
                }
                return SubscriptionUsageCollectOutcome::Deferred;
            }
            Err(UsageTransportError::Unavailable(msg)) => {
                return SubscriptionUsageCollectOutcome::Unavailable {
                    issue_code: "PROVIDER_UNAVAILABLE".into(),
                    message: redact_secrets(&msg, &[token]),
                };
            }
        };
        classify_http_and_parse(&provider, resp, token, req.subscription_identity.as_deref())
    }
}

fn default_provider_origin(provider: &str) -> Option<&'static str> {
    match canonical_provider_namespace(provider).as_str() {
        "anthropic" => Some("https://api.anthropic.com"),
        "openai" => Some("https://chatgpt.com"),
        _ => None,
    }
}

fn transport_error_msg(err: UsageTransportError) -> String {
    match err {
        UsageTransportError::InvalidEndpoint(m)
        | UsageTransportError::InvalidHeader(m)
        | UsageTransportError::Unavailable(m) => m,
        UsageTransportError::DeadlineExceeded => "usage collection deadline exceeded".into(),
    }
}

fn resolve_provider_endpoint(
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

fn resolve_expected_origin(conn: &ProviderConnection) -> Result<String, ApiError> {
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

fn join_usage_path(base: &str, suffix: &str) -> Result<String, SubscriptionUsageCollectOutcome> {
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

fn classify_http_and_parse(
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

fn validate_host_ip(host_lower: &str) -> Result<(), UsageTransportError> {
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

fn split_strict_authority(authority: &str) -> Result<(&str, u16), UsageTransportError> {
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

fn parse_optional_port(after: &str) -> Result<u16, UsageTransportError> {
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

fn parse_port_digits(digits: &str) -> Result<u16, UsageTransportError> {
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

fn is_disallowed_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_disallowed_v4(v4),
        IpAddr::V6(v6) => is_disallowed_v6(v6),
    }
}

fn is_disallowed_v4(v4: std::net::Ipv4Addr) -> bool {
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_multicast()
        || v4.is_broadcast()
        || (v4.octets()[0] == 100 && (v4.octets()[1] & 0b1100_0000) == 64)
}

fn is_disallowed_v6(v6: std::net::Ipv6Addr) -> bool {
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

fn is_valid_bearer_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= MAX_USAGE_SECRET_BYTES
        && token
            .bytes()
            .all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\')
}

fn is_safe_header_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn is_safe_curl_url_chars(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| (0x21..=0x7e).contains(&b) && b != b'"' && b != b'\\')
}

fn is_safe_curl_header_value(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| (0x20..=0x7e).contains(&b) && b != b'"' && b != b'\\')
}

pub(crate) fn format_rfc3339_ms(ms: u64) -> String {
    let secs = (ms / 1_000) as i64;
    let millis = ms % 1_000;
    let days = secs.div_euclid(86_400);
    let sec_of_day = secs.rem_euclid(86_400) as u64;
    let (year, month, day) = civil_from_days(days);
    let hour = sec_of_day / 3_600;
    let minute = (sec_of_day % 3_600) / 60;
    let second = sec_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u32, d as u32)
}

// ---------------------------------------------------------------------------
// Office + CredentialVault orchestration (called by SessionManager)
// ---------------------------------------------------------------------------

pub(crate) fn credential_presence_for(
    vault: &CredentialVault,
    connection_id: &str,
) -> SubscriptionCredentialPresence {
    let usage = vault.usage_status(connection_id).ok();
    let enrolled = vault.status(connection_id).ok();
    let has_usage_credential = usage.as_ref().is_some_and(|u| u.configured);
    let has_enrolled_credential = enrolled.as_ref().is_some_and(|e| e.enrolled);
    let account_hint = usage
        .and_then(|u| u.account_hint)
        .or_else(|| enrolled.and_then(|e| e.account_hint));
    SubscriptionCredentialPresence {
        has_usage_credential,
        has_enrolled_credential,
        account_hint,
    }
}

pub(crate) fn list_subscription_usage(
    office: &Office,
    vault: &CredentialVault,
    actor: &str,
    params: CostSubscriptionUsageListParams,
) -> Result<CostSubscriptionUsageListResult, ApiError> {
    office
        .subscription_usage_list_with_credentials(actor, params, |cid| {
            credential_presence_for(vault, cid)
        })
        .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))?
}

pub(crate) fn set_subscription_usage_credential(
    office: &Office,
    vault: &CredentialVault,
    actor: &str,
    params: CostSubscriptionUsageCredentialSetParams,
) -> Result<CostSubscriptionUsageCredentialSetResult, ApiError> {
    require_owner(office, actor)?;
    let conn = find_subscription_connection(office, &params.connection_id, false)?;
    let bound_origin = resolve_expected_origin(&conn)?;
    verify_expected_origin(&conn, &bound_origin, params.expected_origin.as_deref())?;
    let trimmed_secret = params.secret.trim();
    if !is_valid_bearer_token(trimmed_secret) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "usage credential secret must be 1..=4096 printable ASCII bytes without quotes or backslashes",
        ));
    }
    let expires_at = match params.expires_at {
        Some(ref raw) if !raw.trim().is_empty() => {
            let canon = org_proto::canonicalize_rfc3339_utc(raw).ok_or_else(|| {
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "expires_at must be a valid RFC 3339 timestamp",
                )
            })?;
            Some(canon)
        }
        _ => None,
    };
    let account_hint = match params.account_hint {
        Some(ref h) if !h.trim().is_empty() => {
            let clean = h.trim();
            if clean.len() > MAX_ACCOUNT_HINT_BYTES || clean.contains('\n') || clean.contains('\r')
            {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "account_hint must be a single line of at most 256 bytes",
                ));
            }
            Some(clean.to_owned())
        }
        _ => None,
    };
    let credential = vault
        .store_wrapped_usage(
            conn.id.as_str(),
            &conn.provider_type,
            &bound_origin,
            trimmed_secret.as_bytes(),
            expires_at,
            account_hint,
        )
        .map_err(map_vault_error)?;
    Ok(CostSubscriptionUsageCredentialSetResult { credential })
}

pub(crate) fn revoke_subscription_usage_credential(
    office: &Office,
    vault: &CredentialVault,
    actor: &str,
    params: CostSubscriptionUsageCredentialRevokeParams,
) -> Result<CostSubscriptionUsageCredentialRevokeResult, ApiError> {
    require_owner(office, actor)?;
    let conn = find_subscription_connection(office, &params.connection_id, true)?;
    let credential = vault
        .revoke_usage(conn.id.as_str())
        .map_err(map_vault_error)?;
    Ok(CostSubscriptionUsageCredentialRevokeResult { credential })
}

#[derive(Clone, Copy)]
struct CollectCtx<'a> {
    office: &'a Office,
    vault: &'a CredentialVault,
    collector: &'a dyn SubscriptionUsageCollector,
    time_zone: &'a str,
    collected_at: &'a str,
    now_ms: u64,
    deadline: Instant,
}

pub(crate) fn collect_subscription_usage(
    office: &Office,
    vault: &CredentialVault,
    collector: &dyn SubscriptionUsageCollector,
    actor: &str,
    params: CostSubscriptionUsageCollectParams,
) -> Result<CostSubscriptionUsageCollectResult, ApiError> {
    require_owner(office, actor)?;
    let time_zone = params.time_zone.unwrap_or_else(|| "UTC".to_owned());
    if !org_proto::tz::validate_iana_timezone(&time_zone) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "time_zone must be a valid IANA time zone",
        ));
    }
    let mut targets = resolve_collection_targets(office, params.connection_id.as_deref())?;
    prioritize_oldest_targets(office, &mut targets);
    let now_ms = office.current_time_ms();
    let collected_at = format_rfc3339_ms(now_ms);
    let ctx = CollectCtx {
        office,
        vault,
        collector,
        time_zone: &time_zone,
        collected_at: &collected_at,
        now_ms,
        deadline: Instant::now() + TOTAL_COLLECTION_BUDGET,
    };
    let (mut views, mut deferred_connection_ids) = collect_targets_bounded(ctx, &targets)?;
    views.sort_by(|a, b| a.connection_key.cmp(&b.connection_key));
    let (snapshots, overflow_ids) = bound_collect_views(views);
    deferred_connection_ids.extend(overflow_ids);
    deferred_connection_ids.sort();
    deferred_connection_ids.dedup();
    Ok(CostSubscriptionUsageCollectResult {
        snapshots,
        collected_at,
        deferred_connection_ids,
    })
}

fn verify_expected_origin(
    conn: &ProviderConnection,
    bound_origin: &str,
    supplied: Option<&str>,
) -> Result<(), ApiError> {
    let has_custom_endpoint = conn
        .endpoint_url
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    match supplied.map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => {
            let normalized = if raw.starts_with("provider:") {
                raw.to_ascii_lowercase()
            } else {
                let ep = validate_usage_endpoint_url(raw).map_err(|err| {
                    ApiError::new(ErrorCode::InvalidRequest, transport_error_msg(err))
                })?;
                ep.origin()
            };
            if normalized != bound_origin {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "connection destination origin changed; refresh and confirm expected_origin before saving credential",
                ));
            }
            Ok(())
        }
        None if has_custom_endpoint => Err(ApiError::new(
            ErrorCode::Conflict,
            "expected_origin is required when configuring a credential for a custom endpoint URL",
        )),
        None => Ok(()),
    }
}

fn prioritize_oldest_targets(office: &Office, targets: &mut [ProviderConnection]) {
    let ages = office
        .subscription_usage_collected_at_map()
        .unwrap_or_default();
    let sched = attempt_schedule_map()
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default();
    targets.sort_by(|a, b| {
        let a_sched = sched.get(a.id.as_str()).copied().unwrap_or(0);
        let b_sched = sched.get(b.id.as_str()).copied().unwrap_or(0);
        let a_ts = ages.get(a.id.as_str()).map(String::as_str);
        let b_ts = ages.get(b.id.as_str()).map(String::as_str);
        a_sched
            .cmp(&b_sched)
            .then_with(|| a_ts.cmp(&b_ts))
            .then_with(|| a.key.cmp(&b.key))
    });
}

fn bound_collect_views(
    views: Vec<SubscriptionUsageSnapshotView>,
) -> (Vec<SubscriptionUsageSnapshotView>, Vec<String>) {
    let mut kept = Vec::new();
    let mut overflow = Vec::new();
    let mut used = 4096usize;
    for view in views {
        let sz = serde_json::to_vec(&view).map_or(0, |v| v.len()) + 16;
        if !kept.is_empty() && used.saturating_add(sz) > MAX_COLLECT_RESPONSE_BYTES {
            overflow.push(view.connection_id);
            continue;
        }
        used = used.saturating_add(sz);
        kept.push(view);
    }
    (kept, overflow)
}

fn collect_targets_bounded(
    ctx: CollectCtx<'_>,
    targets: &[ProviderConnection],
) -> Result<(Vec<SubscriptionUsageSnapshotView>, Vec<String>), ApiError> {
    let mut out = Vec::with_capacity(targets.len());
    let mut deferred = Vec::new();
    for chunk in targets.chunks(MAX_CONCURRENT_COLLECTORS) {
        if Instant::now() >= ctx.deadline {
            deferred.extend(chunk.iter().map(|c| c.id.as_str().to_owned()));
            continue;
        }
        if chunk.len() == 1 {
            if let Some(view) = collect_one_connection(ctx, &chunk[0])? {
                out.push(view);
            } else {
                deferred.push(chunk[0].id.as_str().to_owned());
            }
            continue;
        }
        std::thread::scope(|scope| -> Result<(), ApiError> {
            let handles: Vec<_> = chunk
                .iter()
                .map(|conn| {
                    let cid = conn.id.as_str().to_owned();
                    (cid, scope.spawn(move || collect_one_connection(ctx, conn)))
                })
                .collect();
            for (cid, handle) in handles {
                let maybe_view = handle.join().map_err(|_| {
                    ApiError::new(
                        ErrorCode::StorageError,
                        "subscription usage worker thread panicked",
                    )
                })??;
                match maybe_view {
                    Some(view) => out.push(view),
                    None => deferred.push(cid),
                }
            }
            Ok(())
        })?;
    }
    Ok((out, deferred))
}

fn collect_one_connection(
    ctx: CollectCtx<'_>,
    conn: &ProviderConnection,
) -> Result<Option<SubscriptionUsageSnapshotView>, ApiError> {
    if Instant::now() >= ctx.deadline {
        return Ok(None);
    }
    record_scheduling_attempt(conn.id.as_str());
    let attempt_seq = ctx.office.next_subscription_attempt_seq();
    let cred_presence = credential_presence_for(ctx.vault, conn.id.as_str());
    let (auth_source, outcome) = run_collection_attempt(ctx, conn);
    if matches!(outcome, SubscriptionUsageCollectOutcome::Deferred)
        || Instant::now() >= ctx.deadline
    {
        return Ok(None);
    }
    let stored =
        persist_outcome_with_fallback(ctx, conn.id.as_str(), attempt_seq, auth_source, outcome)?;
    Ok(Some(build_snapshot_view(
        conn,
        &ctx.office.info().office_id,
        Some(&stored),
        &cred_presence,
        ctx.office.current_time_ms(),
    )))
}

fn run_collection_attempt(
    ctx: CollectCtx<'_>,
    conn: &ProviderConnection,
) -> (SubscriptionUsageAuthSource, SubscriptionUsageCollectOutcome) {
    let expected_origin = match resolve_expected_origin(conn) {
        Ok(origin) => origin,
        Err(err) => {
            return (
                SubscriptionUsageAuthSource::None,
                SubscriptionUsageCollectOutcome::Unavailable {
                    issue_code: "INVALID_ENDPOINT".into(),
                    message: err.message,
                },
            );
        }
    };
    let sample_instant = Instant::now();
    let attempt_now_ms = ctx.office.current_time_ms();
    match load_active_credential(
        ctx.vault,
        conn.id.as_str(),
        &expected_origin,
        attempt_now_ms,
    ) {
        Ok((source, unwrapped)) => {
            let credential_deadline = unwrapped
                .expires_at
                .as_deref()
                .and_then(org_proto::parse_rfc3339_ms)
                .map(|exp_ms| {
                    sample_instant + Duration::from_millis(exp_ms.saturating_sub(attempt_now_ms))
                });
            let req = SubscriptionUsageCollectRequest {
                connection_id: conn.id.as_str().to_owned(),
                provider_type: conn.provider_type.clone(),
                subscription_identity: conn.subscription_identity.clone(),
                endpoint_url: conn.endpoint_url.clone(),
                auth_source: source,
                credential: unwrapped.secret,
                credential_deadline,
                time_zone: ctx.time_zone.to_owned(),
                collected_at: ctx.collected_at.to_owned(),
                now_ms: attempt_now_ms,
                deadline: ctx.deadline,
            };
            (source, ctx.collector.collect(&req))
        }
        Err((source, early_outcome)) => (source, early_outcome),
    }
}

fn persist_outcome_with_fallback(
    ctx: CollectCtx<'_>,
    connection_id: &str,
    attempt_seq: u64,
    auth_source: SubscriptionUsageAuthSource,
    outcome: SubscriptionUsageCollectOutcome,
) -> Result<StoredSubscriptionUsageSnapshot, ApiError> {
    let candidate =
        outcome_to_stored_snapshot(ctx, connection_id, attempt_seq, auth_source, outcome);
    let saved = match ctx
        .office
        .save_subscription_usage_snapshot(candidate.clone())
        .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))?
    {
        Ok(()) => candidate,
        Err(_) => {
            let fallback = outcome_to_stored_snapshot(
                ctx,
                connection_id,
                attempt_seq,
                auth_source,
                SubscriptionUsageCollectOutcome::Unavailable {
                    issue_code: "INVALID_PROVIDER_RESPONSE".into(),
                    message: "Provider usage response failed validation and was rejected.".into(),
                },
            );
            ctx.office
                .save_subscription_usage_snapshot(fallback.clone())
                .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))??;
            fallback
        }
    };
    Ok(ctx
        .office
        .get_subscription_usage_snapshot(connection_id)
        .ok()
        .flatten()
        .unwrap_or(saved))
}

fn load_active_credential(
    vault: &CredentialVault,
    connection_id: &str,
    expected_origin: &str,
    now_ms: u64,
) -> Result<
    (SubscriptionUsageAuthSource, UnwrappedCredential),
    (SubscriptionUsageAuthSource, SubscriptionUsageCollectOutcome),
> {
    match vault.load_unwrapped_usage_with_meta(connection_id, expected_origin) {
        Ok(Some(cred)) => {
            if is_expired(cred.expires_at.as_deref(), now_ms) {
                return Err((
                    SubscriptionUsageAuthSource::DedicatedUsageCredential,
                    SubscriptionUsageCollectOutcome::AuthRequired {
                        issue_code: "CREDENTIAL_EXPIRED".into(),
                        message: "Dedicated usage credential has expired; rotate or revoke it."
                            .into(),
                    },
                ));
            }
            return Ok((SubscriptionUsageAuthSource::DedicatedUsageCredential, cred));
        }
        Ok(None) => {}
        Err(EnrollmentVaultError::Conflict(_)) => {
            return Err((
                SubscriptionUsageAuthSource::DedicatedUsageCredential,
                SubscriptionUsageCollectOutcome::AuthRequired {
                    issue_code: "ENDPOINT_ORIGIN_CHANGED".into(),
                    message: "Connection endpoint origin changed since this usage credential was stored; re-configure the usage credential.".into(),
                },
            ));
        }
        Err(err) => {
            return Err((
                SubscriptionUsageAuthSource::DedicatedUsageCredential,
                vault_err_to_outcome(err),
            ));
        }
    }
    match vault.load_unwrapped_with_meta(connection_id) {
        Ok(Some(cred)) => {
            if is_expired(cred.expires_at.as_deref(), now_ms) {
                return Err((
                    SubscriptionUsageAuthSource::EnrolledSubscriptionCredential,
                    SubscriptionUsageCollectOutcome::AuthRequired {
                        issue_code: "CREDENTIAL_EXPIRED".into(),
                        message: "Enrolled subscription credential has expired; re-authenticate."
                            .into(),
                    },
                ));
            }
            Ok((
                SubscriptionUsageAuthSource::EnrolledSubscriptionCredential,
                cred,
            ))
        }
        Ok(None) => Err((
            SubscriptionUsageAuthSource::None,
            SubscriptionUsageCollectOutcome::AuthRequired {
                issue_code: "AUTH_MISSING".into(),
                message: "No subscription or usage-API credential is configured on this office."
                    .into(),
            },
        )),
        Err(err) => Err((
            SubscriptionUsageAuthSource::EnrolledSubscriptionCredential,
            vault_err_to_outcome(err),
        )),
    }
}

fn is_expired(expires_at: Option<&str>, now_ms: u64) -> bool {
    let Some(raw) = expires_at else {
        return false;
    };
    org_proto::parse_rfc3339_ms(raw).is_none_or(|exp| now_ms >= exp)
}

fn vault_err_to_outcome(err: EnrollmentVaultError) -> SubscriptionUsageCollectOutcome {
    match err {
        EnrollmentVaultError::MissingSecretsKey => SubscriptionUsageCollectOutcome::AuthRequired {
            issue_code: "SECRETS_KEY_UNAVAILABLE".into(),
            message: "Office secrets key is unavailable; cannot unwrap provider credentials."
                .into(),
        },
        _ => SubscriptionUsageCollectOutcome::AuthRequired {
            issue_code: "CREDENTIAL_UNREADABLE".into(),
            message: "Stored provider credential could not be decrypted; re-authenticate.".into(),
        },
    }
}

fn outcome_to_stored_snapshot(
    ctx: CollectCtx<'_>,
    connection_id: &str,
    attempt_seq: u64,
    auth_source: SubscriptionUsageAuthSource,
    outcome: SubscriptionUsageCollectOutcome,
) -> StoredSubscriptionUsageSnapshot {
    let info = ctx.office.info();
    match outcome {
        SubscriptionUsageCollectOutcome::Collected { models, message } => {
            let fresh_until = Some(format_rfc3339_ms(
                ctx.now_ms.saturating_add(SUBSCRIPTION_USAGE_FRESHNESS_MS),
            ));
            StoredSubscriptionUsageSnapshot {
                organization_id: info.org_id.clone(),
                office_id: info.office_id.clone(),
                connection_id: connection_id.to_owned(),
                attempt_seq,
                status: SubscriptionUsageStatus::Ok,
                auth_source,
                collected_at: ctx.collected_at.to_owned(),
                last_ok_collected_at: Some(ctx.collected_at.to_owned()),
                fresh_until,
                issue_code: None,
                message,
                models,
            }
        }
        SubscriptionUsageCollectOutcome::AuthRequired {
            issue_code,
            message,
        } => StoredSubscriptionUsageSnapshot {
            organization_id: info.org_id.clone(),
            office_id: info.office_id.clone(),
            connection_id: connection_id.to_owned(),
            attempt_seq,
            status: SubscriptionUsageStatus::AuthRequired,
            auth_source,
            collected_at: ctx.collected_at.to_owned(),
            last_ok_collected_at: None,
            fresh_until: None,
            issue_code: Some(issue_code),
            message: Some(message),
            models: Vec::new(),
        },
        SubscriptionUsageCollectOutcome::Unavailable {
            issue_code,
            message,
        } => StoredSubscriptionUsageSnapshot {
            organization_id: info.org_id.clone(),
            office_id: info.office_id.clone(),
            connection_id: connection_id.to_owned(),
            attempt_seq,
            status: SubscriptionUsageStatus::Unavailable,
            auth_source,
            collected_at: ctx.collected_at.to_owned(),
            last_ok_collected_at: None,
            fresh_until: None,
            issue_code: Some(issue_code),
            message: Some(message),
            models: Vec::new(),
        },
        SubscriptionUsageCollectOutcome::Unsupported {
            issue_code,
            message,
        } => StoredSubscriptionUsageSnapshot {
            organization_id: info.org_id.clone(),
            office_id: info.office_id.clone(),
            connection_id: connection_id.to_owned(),
            attempt_seq,
            status: SubscriptionUsageStatus::Unsupported,
            auth_source,
            collected_at: ctx.collected_at.to_owned(),
            last_ok_collected_at: None,
            fresh_until: None,
            issue_code: Some(issue_code),
            message: Some(message),
            models: Vec::new(),
        },
        SubscriptionUsageCollectOutcome::Deferred => StoredSubscriptionUsageSnapshot {
            organization_id: info.org_id.clone(),
            office_id: info.office_id.clone(),
            connection_id: connection_id.to_owned(),
            attempt_seq,
            status: SubscriptionUsageStatus::Unavailable,
            auth_source,
            collected_at: ctx.collected_at.to_owned(),
            last_ok_collected_at: None,
            fresh_until: None,
            issue_code: Some("COLLECTION_DEADLINE_EXCEEDED".into()),
            message: Some("Usage collection deferred before completion.".into()),
            models: Vec::new(),
        },
    }
}

fn require_owner(office: &Office, actor: &str) -> Result<(), ApiError> {
    office
        .require_owner_member(actor)
        .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))?
        .map(|_| ())
}

fn find_subscription_connection(
    office: &Office,
    connection_ref: &str,
    allow_archived: bool,
) -> Result<ProviderConnection, ApiError> {
    let conns = office
        .load_provider_connections()
        .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))?;
    let mut matches: Vec<ProviderConnection> = if org_proto::is_uuid_v7(connection_ref) {
        conns
            .into_iter()
            .filter(|c| c.id.as_str() == connection_ref)
            .collect()
    } else {
        conns
            .into_iter()
            .filter(|c| c.key == connection_ref)
            .collect()
    };
    if matches.len() > 1 {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "ambiguous provider connection key; specify canonical connection_id",
        ));
    }
    let Some(conn) = matches.pop() else {
        return Err(ApiError::new(
            ErrorCode::NotFound,
            "provider connection not found",
        ));
    };
    if conn.auth_mode != ProviderAuthMode::SubscriptionCli {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "subscription usage collection is only valid for subscription_cli connections",
        ));
    }
    if !allow_archived && conn.archived {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "archived provider connections cannot collect subscription usage",
        ));
    }
    Ok(conn)
}

fn resolve_collection_targets(
    office: &Office,
    connection_id: Option<&str>,
) -> Result<Vec<ProviderConnection>, ApiError> {
    if let Some(cid) = connection_id {
        return Ok(vec![find_subscription_connection(office, cid, false)?]);
    }
    let conns = office
        .load_provider_connections()
        .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))?;
    Ok(conns
        .into_iter()
        .filter(|c| !c.archived && c.auth_mode == ProviderAuthMode::SubscriptionCli)
        .collect())
}

fn map_vault_error(err: EnrollmentVaultError) -> ApiError {
    match err {
        EnrollmentVaultError::MissingSecretsKey => ApiError::new(
            ErrorCode::StorageError,
            "office secrets key unavailable; cannot wrap usage credentials",
        ),
        EnrollmentVaultError::InvalidInput(msg) => ApiError::new(ErrorCode::InvalidRequest, msg),
        EnrollmentVaultError::NotAuthorized(msg) => ApiError::new(ErrorCode::NotAuthorized, msg),
        EnrollmentVaultError::Conflict(msg) => ApiError::new(ErrorCode::Conflict, msg),
        EnrollmentVaultError::NotFound(msg) => ApiError::new(ErrorCode::NotFound, msg),
        EnrollmentVaultError::Expired(msg) => ApiError::new(ErrorCode::Expired, msg),
        EnrollmentVaultError::Crypto(msg) | EnrollmentVaultError::Io(msg) => {
            ApiError::new(ErrorCode::StorageError, msg)
        }
    }
}
