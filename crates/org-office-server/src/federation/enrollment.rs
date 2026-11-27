//! Satellite enrollment: the HQ side (proof checks, invitation decision) and the satellite side
//! (joining HQ and validating the enrolled certificate bundle).

use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use std::{io, thread};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::{InvitationPollStatus, JoinRequestAccepted, JoinRequestParams, Office};
use org_pki::cert::{
    build_satellite_tls_client_config, export_enrollment_binding, sign_satellite_key_pop,
};
use org_pki::invitation::{sign_exporter_proof, InvitationToken};
use org_pki::SigningKey;
use org_sync_proto::{
    write_sync_frame, EnrollTerminalStatus, FrameCodecError, FrameReader, SyncFrame,
};
use rustls::{ServerConnection, StreamOwned};

use super::client::{connect_verified_enrollment_hq, read_client_frame};
use super::{ENROLL_WAIT_MAX, FIRST_FRAME_DEADLINE, PARTIAL_FRAME_DEADLINE, POLL_SLEEP};

pub(super) fn handle_hq_enrollment(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    office: &Office,
    stop: &AtomicBool,
) {
    let Ok(exporter) = export_enrollment_binding(&tls.conn) else {
        return;
    };
    let mut reader = FrameReader::new();
    let Some(frame) = read_frame_with_deadline(tls, &mut reader, stop, FIRST_FRAME_DEADLINE) else {
        return;
    };
    let SyncFrame::EnrollRequest {
        invitation_id,
        request_id,
        office_id,
        joining_pub_key_b64,
        joining_key_pop_b64,
        exporter_proof_b64,
    } = frame
    else {
        let _ = write_sync_frame(
            tls,
            &SyncFrame::Error {
                code: "not_authorized".into(),
                message: "unauthenticated connection may only send enroll_request".into(),
            },
        );
        return;
    };
    let proofs = match decode_enroll_proofs(
        &joining_pub_key_b64,
        &joining_key_pop_b64,
        &exporter_proof_b64,
    ) {
        Ok(v) => v,
        Err(msg) => {
            send_enroll_terminal(tls, EnrollTerminalStatus::Rejected, msg);
            return;
        }
    };
    match office.register_satellite_join_request(JoinRequestParams {
        invitation_id: invitation_id.clone(),
        request_id: request_id.clone(),
        office_id,
        joining_pub_key: proofs.pub_key,
        pop_signature_der: proofs.pop_der,
        exporter_binding: exporter,
        exporter_proof_der: proofs.proof_der,
    }) {
        Ok(Ok(JoinRequestAccepted::Pending { check_code, .. })) => {
            if write_sync_frame(
                tls,
                &SyncFrame::EnrollPending {
                    request_id,
                    check_code,
                },
            )
            .is_err()
            {
                return;
            }
            wait_for_invitation_decision(tls, office, &invitation_id, stop);
        }
        Ok(Ok(JoinRequestAccepted::AlreadyApproved {
            cert_der,
            ca_cert_der,
            hq_office_id,
            epoch,
        })) => {
            let _ = write_sync_frame(
                tls,
                &SyncFrame::EnrollApproved {
                    cert_der_b64: STANDARD.encode(cert_der),
                    ca_cert_der_b64: STANDARD.encode(ca_cert_der),
                    hq_office_id,
                    epoch,
                },
            );
        }
        Ok(Err(api)) => {
            let status = if api.code == org_proto::ErrorCode::Expired {
                EnrollTerminalStatus::Expired
            } else {
                EnrollTerminalStatus::Rejected
            };
            send_enroll_terminal(tls, status, api.message);
        }
        Err(e) => send_enroll_terminal(tls, EnrollTerminalStatus::Rejected, e.to_string()),
    }
}

fn send_enroll_terminal(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    status: EnrollTerminalStatus,
    reason: String,
) {
    let _ = write_sync_frame(tls, &SyncFrame::EnrollTerminal { status, reason });
}

struct DecodedEnrollProofs {
    pub_key: [u8; 65],
    pop_der: Vec<u8>,
    proof_der: Vec<u8>,
}

fn decode_enroll_proofs(
    pub_b64: &str,
    pop_b64: &str,
    proof_b64: &str,
) -> Result<DecodedEnrollProofs, String> {
    let pub_vec = STANDARD
        .decode(pub_b64)
        .map_err(|_| "invalid joining_pub_key base64".to_owned())?;
    let pub_key: [u8; 65] = pub_vec
        .try_into()
        .map_err(|_| "joining_pub_key must be 65 bytes".to_owned())?;
    let pop_der = STANDARD
        .decode(pop_b64)
        .map_err(|_| "invalid joining_key_pop base64".to_owned())?;
    let proof_der = STANDARD
        .decode(proof_b64)
        .map_err(|_| "invalid exporter_proof base64".to_owned())?;
    Ok(DecodedEnrollProofs {
        pub_key,
        pop_der,
        proof_der,
    })
}

fn wait_for_invitation_decision(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    office: &Office,
    invitation_id: &str,
    stop: &AtomicBool,
) {
    let deadline = Instant::now() + ENROLL_WAIT_MAX;
    while !stop.load(Ordering::Acquire) && Instant::now() < deadline {
        match office.poll_satellite_invitation(invitation_id) {
            Ok(InvitationPollStatus::Pending) => thread::sleep(POLL_SLEEP),
            Ok(InvitationPollStatus::Approved {
                cert_der,
                ca_cert_der,
                hq_office_id,
                epoch,
            }) => {
                let _ = write_sync_frame(
                    tls,
                    &SyncFrame::EnrollApproved {
                        cert_der_b64: STANDARD.encode(cert_der),
                        ca_cert_der_b64: STANDARD.encode(ca_cert_der),
                        hq_office_id,
                        epoch,
                    },
                );
                return;
            }
            Ok(InvitationPollStatus::Terminal { code, message }) => {
                let status = match code.as_str() {
                    "expired" => EnrollTerminalStatus::Expired,
                    "cancelled" => EnrollTerminalStatus::Cancelled,
                    _ => EnrollTerminalStatus::Rejected,
                };
                send_enroll_terminal(tls, status, message);
                return;
            }
            Err(_) => return,
        }
    }
}

fn read_frame_with_deadline(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    reader: &mut FrameReader,
    stop: &AtomicBool,
    max_wait: Duration,
) -> Option<SyncFrame> {
    let deadline = Instant::now() + max_wait;
    while !stop.load(Ordering::Acquire) && Instant::now() < deadline {
        match reader.read_frame(tls, PARTIAL_FRAME_DEADLINE) {
            Ok(Some(f)) => return Some(f),
            Ok(None) => return None,
            Err(FrameCodecError::Io(e))
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(_) => return None,
        }
    }
    None
}

/// Output of a successful satellite enrollment exchange with HQ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnrolledCertificateBundle {
    pub cert_der: Vec<u8>,
    pub ca_cert_der: Vec<u8>,
    pub hq_office_id: String,
    pub epoch: u64,
    pub check_code: String,
}

/// Performs satellite enrollment against HQ over Org-CA-pinned TLS using RFC 5705 exporter
/// binding + one-time invitation proof without the satellite's private key ever leaving it.
pub fn enroll_satellite_with_hq(
    token: &InvitationToken,
    local_tls_key: &SigningKey,
    on_check_code: impl FnOnce(&str),
) -> Result<EnrolledCertificateBundle, String> {
    let client_cfg = build_satellite_tls_client_config(&token.ca_cert_der, None)
        .map_err(|e| format!("failed building enrollment TLS config: {e}"))?;
    let (mut tls, verified_hq_epoch) =
        connect_verified_enrollment_hq(&token.hq_addresses, client_cfg, token)?;
    let exporter = export_enrollment_binding(&tls.conn)
        .map_err(|e| format!("TLS exporter binding failed: {e}"))?;
    let inv_key = token
        .signing_key()
        .map_err(|e| format!("invalid invitation signing key: {e}"))?;
    let pop = sign_satellite_key_pop(
        local_tls_key,
        &token.org_id,
        &token.office_id,
        &token.invitation_id,
    )
    .map_err(|e| format!("failed signing satellite key PoP: {e}"))?;
    let proof = sign_exporter_proof(
        &inv_key,
        &exporter,
        &token.invitation_id,
        &token.office_id,
        &local_tls_key.public_key(),
    )
    .map_err(|e| format!("failed signing exporter proof: {e}"))?;

    write_sync_frame(
        &mut tls,
        &SyncFrame::EnrollRequest {
            invitation_id: token.invitation_id.clone(),
            request_id: format!("req-{}", token.office_id),
            office_id: token.office_id.clone(),
            joining_pub_key_b64: STANDARD.encode(local_tls_key.public_key()),
            joining_key_pop_b64: STANDARD.encode(pop),
            exporter_proof_b64: STANDARD.encode(proof),
        },
    )
    .map_err(|e| format!("failed sending EnrollRequest: {e}"))?;

    let mut reader = FrameReader::new();
    let check_code = match read_client_frame(&mut tls, &mut reader, FIRST_FRAME_DEADLINE, None)? {
        Some(SyncFrame::EnrollPending { check_code, .. }) => {
            on_check_code(&check_code);
            check_code
        }
        Some(SyncFrame::EnrollApproved {
            cert_der_b64,
            ca_cert_der_b64,
            hq_office_id,
            epoch,
        }) => {
            return validate_enrolled_bundle(
                token,
                verified_hq_epoch,
                cert_der_b64,
                ca_cert_der_b64,
                hq_office_id,
                epoch,
                String::new(),
            );
        }
        Some(SyncFrame::EnrollTerminal { status, reason }) => {
            return Err(format!("enrollment refused ({status:?}): {reason}"));
        }
        other => return Err(format!("unexpected enrollment frame: {other:?}")),
    };

    match read_client_frame(&mut tls, &mut reader, ENROLL_WAIT_MAX, None)? {
        Some(SyncFrame::EnrollApproved {
            cert_der_b64,
            ca_cert_der_b64,
            hq_office_id,
            epoch,
        }) => validate_enrolled_bundle(
            token,
            verified_hq_epoch,
            cert_der_b64,
            ca_cert_der_b64,
            hq_office_id,
            epoch,
            check_code,
        ),
        Some(SyncFrame::EnrollTerminal { status, reason }) => {
            Err(format!("enrollment terminal ({status:?}): {reason}"))
        }
        other => Err(format!("unexpected final enrollment frame: {other:?}")),
    }
}

fn validate_enrolled_bundle(
    token: &InvitationToken,
    verified_hq_epoch: u64,
    cert_der_b64: String,
    ca_cert_der_b64: String,
    hq_office_id: String,
    epoch: u64,
    check_code: String,
) -> Result<EnrolledCertificateBundle, String> {
    let cert_der = STANDARD
        .decode(&cert_der_b64)
        .map_err(|_| "invalid cert_der base64".to_owned())?;
    let ca_cert_der = STANDARD
        .decode(&ca_cert_der_b64)
        .map_err(|_| "invalid ca_cert_der base64".to_owned())?;
    if ca_cert_der != token.ca_cert_der || hq_office_id != token.hq_office_id {
        return Err("EnrollApproved CA or HQ office ID does not match pinned invitation".into());
    }
    if epoch != verified_hq_epoch {
        return Err("EnrollApproved epoch does not match authenticated HQ leaf cert".into());
    }
    Ok(EnrolledCertificateBundle {
        cert_der,
        ca_cert_der,
        hq_office_id,
        epoch,
        check_code,
    })
}
