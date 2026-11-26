//! The listener setup records: choosing and checking its port and address (SRV-4) and
//! reporting its TLS pair.

use org_host_control::listen::ListenConfig;

use crate::instance::registry::Registry;
use crate::instance::setup::{Input, Journal, Paths, Record};

use super::output::fail;
use super::request::DEFAULT_PORT;

/// Setup's check of an `https` input's certificate, before anything is saved: a supplied pair
/// is read and verified for the origin now (`tls.set`'s checks), and its fingerprint is what
/// setup says; without one, what the host will make. `None` for plain HTTP.
pub(super) fn tls_preflight(
    input: &Input,
    tls: Option<&crate::instance::setup::SuppliedTls>,
) -> Result<Option<String>, String> {
    if input.listen.scheme() != org_host_control::listen::ListenMode::Https {
        return Ok(None);
    }
    let Some(tls) = tls else {
        return Ok(Some(made_notice(input.listen.origin())));
    };
    let now = crate::web_tls::now().ok_or("the clock is unavailable; nothing was saved")?;
    let checked = crate::web_tls::read_supplied(&tls.cert, &tls.key, input.listen.origin(), now)
        .map_err(|why| format!("{why}; nothing was saved"))?;
    Ok(Some(format!(
        "{}. The host imports it into its secrets at its first start; the files are not \
         read after that",
        checked.summary(crate::web_tls::Source::Supplied)
    )))
}

fn made_notice(origin: &org_host_control::listen::ListenOrigin) -> String {
    format!(
        "the instance makes its own certificate for {} at its first start (ECDSA P-256, \
         valid 397 days, renewed when 30 days remain) and prints its SHA-256 fingerprint in \
         its log at every start; check it against the fingerprint the browser shows before \
         you accept the certificate",
        origin.as_str()
    )
}

/// What a later run says of an `https` instance's certificate.
pub(super) fn tls_notice(record: &Record) -> String {
    match &record.tls {
        Some(tls) => format!(
            "supplied ({}, {}); the host prints its SHA-256 fingerprint in its log at every \
             start",
            tls.cert.display(),
            tls.key.display()
        ),
        None => made_notice(record.input.listen.origin()),
    }
}

pub(crate) fn loopback(port: u16) -> Result<ListenConfig, String> {
    serde_json::from_value(serde_json::json!({
        "address": "127.0.0.1", "port": port, "scheme": "http_loopback",
        "origin": format!("http://127.0.0.1:{port}"), "hsts": "off"
    }))
    .map_err(|e| fail("building the address", e))
}

pub(super) fn port_free(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// Whether the address of `listen` can be bound now: its own address and port, in its own
/// address family (X5 review, fourth round, P2-2).
pub(super) fn address_free(listen: &ListenConfig) -> bool {
    crate::BrowserListener::bind(listen).is_ok()
}

/// The ports other instances saved, by name: their frozen setup records and the browser
/// addresses in the instance locator. Unreadable entries are skipped; this only steers
/// the choice of a new port.
pub(super) fn saved_ports(paths: &Paths, name: Option<&str>) -> Vec<(String, u16)> {
    let shown = name.unwrap_or("default");
    let mut saved = Vec::new();
    if let Ok(entries) = std::fs::read_dir(paths.base.join("setup")) {
        for entry in entries.flatten() {
            let Some(other) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if other == shown {
                continue;
            }
            if let Ok(record) = Journal::snapshot(&paths.base, Some(&other)) {
                saved.push((other, record.input.listen.port()));
            }
        }
    }
    if let Ok(registry) = Registry::open(&paths.locator, &paths.user_temp) {
        if let Ok(ports) = registry.saved_browser_ports() {
            saved.extend(ports.into_iter().filter(|(other, _)| other != shown));
        }
    }
    saved.sort();
    saved.dedup();
    saved
}

/// Refuses a saved port that another program holds while this instance's service is not
/// running: the service could not serve its address, and the address does not change.
pub(super) fn port_held_elsewhere(listen: &ListenConfig, name: Option<&str>) -> String {
    format!(
        "port {} on {}, this instance's saved address, is in use by another program, so its \
         service cannot serve it; stop that program, then run {}",
        listen.port(),
        listen.address(),
        match name {
            None | Some("default") => "agentic-org setup".to_owned(),
            Some(name) => format!("agentic-org setup --name {name}"),
        }
    )
}

/// The first run's port: the requested one if free, otherwise the instance's default.
/// A named instance's default is the first free port from `named_base`.
/// `saved` holds the ports other instances saved, by name; none of them is chosen.
pub(crate) fn choose_port(
    name: Option<&str>,
    requested: Option<u16>,
    named_base: u16,
    free: impl Fn(u16) -> bool,
    saved: &[(String, u16)],
) -> Result<u16, String> {
    let owner = |port: u16| saved.iter().find(|(_, p)| *p == port).map(|(n, _)| n);
    if let Some(port) = requested {
        if port == 0 {
            return Err("port 0 is not a fixed address; choose a port from 1 to 65535".into());
        }
        if let Some(other) = owner(port) {
            return Err(format!(
                "port {port} is the saved address of instance \"{other}\"; choose another \
                 port with --port; nothing was saved"
            ));
        }
        if !free(port) {
            return Err(format!(
                "port {port} on 127.0.0.1 is in use by another program; choose another \
                 port with --port, or stop that program; nothing was saved"
            ));
        }
        return Ok(port);
    }
    match name {
        None | Some("default") => {
            if let Some(other) = owner(DEFAULT_PORT) {
                Err(format!(
                    "port {DEFAULT_PORT} is the saved address of instance \"{other}\"; choose \
                     another port with --port; nothing was saved"
                ))
            } else if free(DEFAULT_PORT) {
                Ok(DEFAULT_PORT)
            } else {
                Err(format!(
                    "port {DEFAULT_PORT} on 127.0.0.1 is in use by another program; choose \
                     another port with --port, or stop that program; nothing was saved"
                ))
            }
        }
        Some(_) => (named_base..=u16::MAX)
            .find(|p| owner(*p).is_none() && free(*p))
            .ok_or_else(|| {
                format!("no free port on 127.0.0.1 from {named_base}; nothing was saved")
            }),
    }
}
