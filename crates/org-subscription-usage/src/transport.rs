//! The system HTTPS usage transport: isolated `curl` with pinned DNS resolution, bounded
//! output, and shared DNS-lookup and scheduling budgets.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{IpAddr, ToSocketAddrs};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::Instant;

use zeroize::Zeroizing;

use super::endpoint::{
    is_disallowed_ip, is_safe_curl_header_value, is_safe_curl_url_chars, is_safe_header_name,
    validate_usage_endpoint_url, ValidatedEndpoint,
};
use super::{
    HttpUsageResponse, SubscriptionUsageTransport, UsageTransportError, MAX_CONCURRENT_DNS_LOOKUPS,
    MAX_DNS_TIMEOUT, MAX_USAGE_RESPONSE_BYTES,
};

pub(super) static ACTIVE_DNS_LOOKUPS: AtomicUsize = AtomicUsize::new(0);

pub(super) static ATTEMPT_SCHEDULE_SEQ: AtomicU64 = AtomicU64::new(1);

pub(super) struct DnsPermit;

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

pub(super) fn attempt_schedule_map() -> &'static Mutex<BTreeMap<String, u64>> {
    static MAP: OnceLock<Mutex<BTreeMap<String, u64>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub(super) fn record_scheduling_attempt(connection_id: &str) {
    let seq = ATTEMPT_SCHEDULE_SEQ.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut map) = attempt_schedule_map().lock() {
        if map.len() > 2048 {
            map.clear();
        }
        map.insert(connection_id.to_owned(), seq);
    }
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

pub(super) fn kill_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

pub(super) fn read_bounded_stdout(child: &mut Child) -> Result<Vec<u8>, UsageTransportError> {
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

pub(super) fn resolve_and_validate_ip(
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

pub(super) fn build_curl_stdin_config(
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

pub(super) fn parse_curl_output(raw: &[u8]) -> Result<HttpUsageResponse, UsageTransportError> {
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
