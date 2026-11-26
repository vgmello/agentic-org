//! The satellite's mTLS client: server names, HQ leaf verification, the client handshake, and
//! connecting to an HQ endpoint for enrollment or sync.

use std::collections::VecDeque;
use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use org_office::Office;
use org_pki::cert::{verify_office_cert, OfficeCertRole};
use org_pki::invitation::InvitationToken;
use org_sync::organization::{PeerIdentity, PeerRole};
use org_sync_proto::{
    write_sync_frame, Digest, Epoch, FrameCodecError, FrameReader, OfficeId, OrgId, SyncFrame,
    WireOfficeRole,
};
use rustls::pki_types::ServerName;
use rustls::{ClientConnection, StreamOwned};
use sha2::{Digest as _, Sha256};

use super::{
    now_unix, SatelliteSyncSession, CONNECT_TIMEOUT, FIRST_FRAME_DEADLINE, HANDSHAKE_DEADLINE,
    PARTIAL_FRAME_DEADLINE, SOCKET_READ_TIMEOUT, SOCKET_WRITE_TIMEOUT,
};

fn server_name_from_addr(addr: &str) -> Result<ServerName<'static>, String> {
    let stripped = addr
        .strip_prefix("https://")
        .or_else(|| addr.strip_prefix("http://"))
        .or_else(|| addr.strip_prefix("tls://"))
        .unwrap_or(addr);
    let host = if let Some(rest) = stripped.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        stripped.split(':').next().unwrap_or("")
    };
    ServerName::try_from(host.to_owned()).map_err(|_| format!("invalid HQ host `{host}`"))
}

/// Verifies HQ's TLS leaf certificate against the pinned Organization CA, org ID, HQ office ID,
/// and HQ role/epoch (`TASK-52` F17).
fn verify_hq_peer_leaf(
    tls: &StreamOwned<ClientConnection, TcpStream>,
    ca_cert_der: &[u8],
    expected_org_id: &str,
    expected_hq_office_id: &str,
    min_epoch: u64,
) -> Result<u64, String> {
    let leaf = tls
        .conn
        .peer_certificates()
        .and_then(|c| c.first())
        .ok_or_else(|| "HQ presented no TLS leaf certificate".to_owned())?;
    let verified = verify_office_cert(
        ca_cert_der,
        leaf.as_ref(),
        expected_org_id,
        None,
        &Default::default(),
        now_unix(),
    )
    .map_err(|e| format!("HQ leaf certificate verification failed: {e}"))?;
    if verified.office_id != expected_hq_office_id {
        return Err(format!(
            "HQ leaf office_id `{}` does not match pinned `{expected_hq_office_id}`",
            verified.office_id
        ));
    }
    match verified.role {
        OfficeCertRole::Hq { epoch } if epoch >= min_epoch => Ok(epoch),
        OfficeCertRole::Hq { epoch } => Err(format!(
            "HQ leaf epoch {epoch} is below pinned epoch {min_epoch}"
        )),
        OfficeCertRole::Satellite => Err("peer presented a satellite cert instead of HQ".into()),
    }
}

fn complete_client_handshake(
    tls: &mut StreamOwned<ClientConnection, TcpStream>,
) -> Result<(), String> {
    let deadline = Instant::now() + HANDSHAKE_DEADLINE;
    while tls.conn.is_handshaking() {
        if Instant::now() >= deadline {
            return Err("TLS handshake with HQ timed out".into());
        }
        match tls.conn.complete_io(&mut tls.sock) {
            Ok(_) => {}
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => return Err(format!("TLS handshake with HQ failed: {e}")),
        }
    }
    Ok(())
}

pub(super) fn read_client_frame(
    tls: &mut StreamOwned<ClientConnection, TcpStream>,
    reader: &mut FrameReader,
    wait: Duration,
) -> Result<Option<SyncFrame>, String> {
    let deadline = Instant::now() + wait;
    loop {
        match reader.read_frame(tls, PARTIAL_FRAME_DEADLINE) {
            Ok(frame) => return Ok(frame),
            Err(FrameCodecError::Io(e))
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                if Instant::now() >= deadline {
                    return Err("timed out waiting for HQ frame".into());
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Performs verified enrollment TCP + TLS + pinned HQ leaf verification on a single resolved IP
/// while preserving the hostname for SNI/certificate verification (`TASK-52` P4-#12).
fn try_enroll_sock_addr(
    sock_addr: std::net::SocketAddr,
    server_name: rustls::pki_types::ServerName<'static>,
    client_cfg: Arc<rustls::ClientConfig>,
    token: &InvitationToken,
) -> Result<(StreamOwned<ClientConnection, TcpStream>, u64), String> {
    let tcp = TcpStream::connect_timeout(&sock_addr, CONNECT_TIMEOUT)
        .map_err(|e| format!("TCP connect {sock_addr}: {e}"))?;
    let _ = tcp.set_nodelay(true);
    let _ = tcp.set_read_timeout(Some(SOCKET_READ_TIMEOUT));
    let _ = tcp.set_write_timeout(Some(SOCKET_WRITE_TIMEOUT));
    let conn = ClientConnection::new(client_cfg, server_name)
        .map_err(|e| format!("failed creating TLS client connection: {e}"))?;
    let mut tls = StreamOwned::new(conn, tcp);
    complete_client_handshake(&mut tls)?;
    let verified_hq_epoch = verify_hq_peer_leaf(
        &tls,
        &token.ca_cert_der,
        &token.org_id,
        &token.hq_office_id,
        1,
    )?;
    Ok((tls, verified_hq_epoch))
}

/// Iterates every invitation `hq_addresses` entry AND every resolved `SocketAddr` for multi-IP
/// hostnames through full TCP + TLS handshake + pinned HQ peer-leaf verification (`TASK-52` P3-#13 / P4-#12).
pub(super) fn connect_verified_enrollment_hq(
    hq_addresses: &[String],
    client_cfg: Arc<rustls::ClientConfig>,
    token: &InvitationToken,
) -> Result<(StreamOwned<ClientConnection, TcpStream>, u64), String> {
    let mut last_err = String::from("no HQ addresses configured in invitation");
    for addr in hq_addresses {
        let Ok(server_name) = server_name_from_addr(addr) else {
            last_err = format!("{addr}: invalid server name");
            continue;
        };
        let Ok(resolved) = addr.to_socket_addrs() else {
            last_err = format!("{addr}: DNS resolution failed");
            continue;
        };
        for sock_addr in resolved {
            match try_enroll_sock_addr(
                sock_addr,
                server_name.clone(),
                Arc::clone(&client_cfg),
                token,
            ) {
                Ok(pair) => return Ok(pair),
                Err(err) => last_err = format!("{addr} ({sock_addr}): {err}"),
            }
        }
    }
    Err(format!(
        "failed verified enrollment TLS handshake to all HQ addresses: {last_err}"
    ))
}

/// Attempts full TCP + Org-CA mTLS + leaf identity + `Hello` against every resolved IP of a
/// single HQ endpoint while preserving the hostname for certificate verification (`TASK-52` #17 / P4-#12).
pub(super) fn try_connect_hq_endpoint(
    office: Arc<Office>,
    ident: &org_office::SatelliteTlsIdentity,
    cfg: Arc<rustls::ClientConfig>,
    cp: &org_sync::organization::OrgCheckpoint,
    addr: &str,
    stop: Option<&AtomicBool>,
) -> Result<SatelliteSyncSession, String> {
    let server_name = server_name_from_addr(addr)?;
    let resolved: Vec<std::net::SocketAddr> = addr
        .to_socket_addrs()
        .map_err(|e| format!("DNS resolution for {addr}: {e}"))?
        .collect();
    let mut last_err = format!("no resolved addresses for {addr}");
    for sock_addr in resolved {
        if stop.is_some_and(|s| s.load(Ordering::Acquire)) {
            return Err("federation connect cancelled by shutdown".into());
        }
        match try_connect_hq_sock_addr(
            Arc::clone(&office),
            ident,
            Arc::clone(&cfg),
            cp,
            sock_addr,
            server_name.clone(),
        ) {
            Ok(session) => return Ok(session),
            Err(err) => last_err = format!("{addr} ({sock_addr}): {err}"),
        }
    }
    Err(last_err)
}

fn try_connect_hq_sock_addr(
    office: Arc<Office>,
    ident: &org_office::SatelliteTlsIdentity,
    cfg: Arc<rustls::ClientConfig>,
    cp: &org_sync::organization::OrgCheckpoint,
    sock_addr: std::net::SocketAddr,
    server_name: rustls::pki_types::ServerName<'static>,
) -> Result<SatelliteSyncSession, String> {
    let tcp = TcpStream::connect_timeout(&sock_addr, CONNECT_TIMEOUT)
        .map_err(|e| format!("TCP connect: {e}"))?;
    let _ = tcp.set_nodelay(true);
    let _ = tcp.set_read_timeout(Some(SOCKET_READ_TIMEOUT));
    let _ = tcp.set_write_timeout(Some(SOCKET_WRITE_TIMEOUT));
    let conn =
        ClientConnection::new(cfg, server_name).map_err(|e| format!("mTLS client init: {e}"))?;
    let mut tls = StreamOwned::new(conn, tcp);
    complete_client_handshake(&mut tls)?;
    let verified_hq_epoch = verify_hq_peer_leaf(
        &tls,
        &ident.ca_cert_der,
        &office.info().org_id,
        &ident.federation.hq_office_id,
        ident.federation.epoch,
    )?;
    // Verify TLS peer HQ ID and CA digest match the durable replication checkpoint BEFORE sending
    // Hello or installing any snapshot so a crash mid-re-enrollment can never restore trust in an
    // old HQ (`TASK-52` P9-#2).
    let ident_ca_digest = Digest::from_bytes(Sha256::digest(&ident.ca_cert_der).into());
    if ident.federation.hq_office_id != cp.hq_office.as_str() || ident_ca_digest != cp.ca {
        return Err(
            "satellite TLS identity does not match durable checkpoint HQ/CA; finish re-enrollment"
                .into(),
        );
    }
    write_sync_frame(
        &mut tls,
        &SyncFrame::Hello {
            org_id: office.info().org_id.clone(),
            office_id: office.info().office_id.clone(),
            role: WireOfficeRole::Satellite,
            epoch: cp.position.epoch.get(),
            protocol_level: cp.protocol_level,
            supported_max: 1,
            codec_max: 1,
            hq_addresses: Vec::new(),
            owner_member_id: None,
        },
    )
    .map_err(|e| format!("failed sending Hello: {e}"))?;
    let mut reader = FrameReader::new();
    let hello_reply = read_client_frame(&mut tls, &mut reader, FIRST_FRAME_DEADLINE)?;
    let (hq_office_id, hq_epoch) = match hello_reply {
        Some(SyncFrame::Hello {
            org_id,
            office_id,
            role,
            epoch,
            owner_member_id,
            ..
        }) => {
            if org_id != office.info().org_id
                || office_id != ident.federation.hq_office_id
                || role != WireOfficeRole::Hq
                || epoch != verified_hq_epoch
            {
                return Err("HQ Hello identity does not match verified TLS leaf".into());
            }
            let Some(owner_id) = owner_member_id else {
                return Err("HQ Hello missing owner_member_id".into());
            };
            office
                .record_replicated_hq_owner(&office_id, &owner_id)
                .map_err(|e| format!("invalid HQ Hello owner_member_id: {e}"))?;
            (office_id, epoch)
        }
        Some(SyncFrame::Error { code, message }) => {
            return Err(format!("HQ rejected mTLS session ({code}): {message}"));
        }
        other => return Err(format!("unexpected Hello reply: {other:?}")),
    };
    let hq_peer = PeerIdentity {
        org_id: OrgId::new(&office.info().org_id).map_err(|_| "invalid org_id".to_owned())?,
        office_id: OfficeId::new(&hq_office_id).map_err(|_| "invalid hq_office_id".to_owned())?,
        role: PeerRole::Hq,
        authority: Epoch::new(hq_epoch.max(1)).map_err(|_| "invalid hq_epoch".to_owned())?,
    };
    Ok(SatelliteSyncSession {
        office,
        tls,
        reader,
        hq_peer,
        pending_inbound: VecDeque::new(),
    })
}
