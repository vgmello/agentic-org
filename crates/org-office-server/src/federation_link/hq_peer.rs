//! The HQ's view of one connected satellite: revocation checks, peer-gated frame writes, and
//! organization-event pushes.

use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::Office;
use org_sync_proto::{write_sync_frame, FrameCodecError, FrameReader, OfficeId, OrgId, SyncFrame};
use rustls::{ServerConnection, StreamOwned};

use super::PARTIAL_FRAME_DEADLINE;

pub(crate) struct HqSessionPeer<'a> {
    pub(crate) office: &'a Office,
    pub(crate) org_id: &'a OrgId,
    pub(crate) sat_id: &'a OfficeId,
    pub(crate) cert_serial_hex: &'a str,
    pub(crate) session_stop: &'a AtomicBool,
    pub(crate) server_stop: &'a AtomicBool,
}

pub(crate) fn check_not_revoked_fail_closed(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    office: &Office,
    satellite_office_id: &str,
    cert_serial_hex: &str,
) -> bool {
    match org_office::is_peer_revoked(office, satellite_office_id, Some(cert_serial_hex)) {
        Ok(false) => true,
        Ok(true) => {
            let _ = write_sync_frame(
                tls,
                &SyncFrame::Error {
                    code: "revoked".into(),
                    message: "office certificate revoked".into(),
                },
            );
            false
        }
        Err(_) => {
            let _ = write_sync_frame(
                tls,
                &SyncFrame::Error {
                    code: "unavailable".into(),
                    message: "revocation store unavailable".into(),
                },
            );
            false
        }
    }
}

pub(crate) fn write_sync_frame_if_peer_active(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    frame: &SyncFrame,
) -> bool {
    if peer.session_stop.load(Ordering::Acquire) || peer.server_stop.load(Ordering::Acquire) {
        return false;
    }
    matches!(
        org_office::run_if_peer_active(
            peer.office,
            peer.sat_id.as_str(),
            Some(peer.cert_serial_hex),
            || write_sync_frame(tls, frame),
        ),
        Ok(Some(Ok(())))
    )
}

pub(crate) fn read_hq_frame_nonblocking(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    reader: &mut FrameReader,
) -> Result<Option<SyncFrame>, FrameCodecError> {
    let _ = tls.sock.set_nonblocking(true);
    let res = reader.read_frame(tls, PARTIAL_FRAME_DEADLINE);
    let _ = tls.sock.set_nonblocking(false);
    res
}

pub(crate) fn push_hq_org_events_if_behind(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    peer_org_cursor: &mut Option<(u64, u64)>,
) -> bool {
    let Some((after_epoch, after_seq)) = *peer_org_cursor else {
        return true;
    };
    if peer.office.sync_org_journal_to_replication().is_err() {
        return false;
    }
    let Ok(events) = peer
        .office
        .replication()
        .org_events_after(after_epoch, after_seq)
    else {
        return false;
    };
    for ev in events {
        let ep = ev.position.epoch.get();
        let sq = ev.position.seq;
        let frame = SyncFrame::OrgEvent {
            org_id: ev.org_id.as_str().to_owned(),
            epoch: ep,
            seq: sq,
            previous_hex: ev.previous.to_hex(),
            digest_hex: ev.digest.to_hex(),
            codec_name: ev.codec.id().to_owned(),
            codec_version: ev.codec.version(),
            payload_b64: STANDARD.encode(&ev.payload),
        };
        if !write_sync_frame_if_peer_active(tls, peer, &frame) {
            return false;
        }
        *peer_org_cursor = Some((ep, sq));
    }
    true
}

pub(crate) fn send_hq_org_events_after(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    epoch: u64,
    seq: u64,
) -> Option<(u64, u64)> {
    // Publish any accepted HQ `org-store` entries via the `ARCH-03` consumer before streaming (`TASK-52` F2).
    if peer.office.sync_org_journal_to_replication().is_err() {
        return None;
    }
    let events = peer
        .office
        .replication()
        .org_events_after(epoch, seq)
        .ok()?;
    let mut last_sent = (epoch, seq);
    for ev in events {
        let ep = ev.position.epoch.get();
        let sq = ev.position.seq;
        let frame = SyncFrame::OrgEvent {
            org_id: ev.org_id.as_str().to_owned(),
            epoch: ep,
            seq: sq,
            previous_hex: ev.previous.to_hex(),
            digest_hex: ev.digest.to_hex(),
            codec_name: ev.codec.id().to_owned(),
            codec_version: ev.codec.version(),
            payload_b64: STANDARD.encode(&ev.payload),
        };
        // Fence every outbound org event against concurrent removal (`TASK-52` P20-#1).
        if !write_sync_frame_if_peer_active(tls, peer, &frame) {
            return None;
        }
        last_sent = (ep, sq);
    }
    write_sync_frame_if_peer_active(tls, peer, &SyncFrame::Pong { nonce: seq }).then_some(last_sent)
}
