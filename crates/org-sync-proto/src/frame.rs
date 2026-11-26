//! Bounded versioned wire frames for office-to-office federation over Organization-CA mTLS
//! (`doc-67`, `X-05`, `TASK-52`).
use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use crate::MAX_ENCODED_FRAME_BYTES;

pub const SYNC_WIRE_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnrollTerminalStatus {
    Expired,
    Cancelled,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireOfficeRole {
    Hq,
    Satellite,
}

fn default_one_u32() -> u32 {
    1
}

/// A single length-delimited JSON federation frame bounded by [`MAX_ENCODED_FRAME_BYTES`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SyncFrame {
    // --- Satellite enrollment over HQ-pinned TLS + TLS-exporter proof ---
    EnrollRequest {
        invitation_id: String,
        request_id: String,
        office_id: String,
        /// Uncompressed 65-byte P-256 public key (base64) for check-code & identity binding.
        joining_pub_key_b64: String,
        /// Proof-of-possession ECDSA signature by the satellite's local TLS private key
        /// (private key NEVER leaves the satellite).
        joining_key_pop_b64: String,
        exporter_proof_b64: String,
    },
    EnrollPending {
        request_id: String,
        check_code: String,
    },
    EnrollApproved {
        cert_der_b64: String,
        ca_cert_der_b64: String,
        hq_office_id: String,
        epoch: u64,
    },
    EnrollTerminal {
        status: EnrollTerminalStatus,
        reason: String,
    },

    // --- Mutual TLS session handshake & protocol level negotiation ---
    Hello {
        org_id: String,
        office_id: String,
        role: WireOfficeRole,
        epoch: u64,
        protocol_level: u32,
        supported_max: u32,
        codec_max: u32,
        #[serde(default)]
        hq_addresses: Vec<String>,
        /// HQ-authoritative active owner employee UUIDv7 (`TASK-141`), populated by HQ on Hello
        /// so satellites track ownership across handovers without guessing from audit metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner_member_id: Option<String>,
    },

    // --- Organization replication (HQ -> Satellite) ---
    OrgSnapshotRequest {
        after_epoch: u64,
        after_seq: u64,
        #[serde(default)]
        snapshot_id: Option<String>,
        #[serde(default)]
        page_index: u32,
        #[serde(default)]
        supports_paging: bool,
    },
    OrgSnapshotChunk {
        snapshot_id: String,
        chunk_index: u32,
        total_chunks: u32,
        sha256_hex: String,
        data_b64: String,
        final_epoch: u64,
        final_seq: u64,
        final_digest_hex: String,
        protocol_level: u32,
        #[serde(default)]
        page_index: u32,
        #[serde(default = "default_one_u32")]
        total_pages: u32,
    },
    OrgEvent {
        org_id: String,
        epoch: u64,
        seq: u64,
        previous_hex: String,
        digest_hex: String,
        codec_name: String,
        codec_version: u32,
        payload_b64: String,
    },
    OrgAck {
        epoch: u64,
        seq: u64,
        digest_hex: String,
        duplicate: bool,
    },
    OrgGap {
        after_epoch: u64,
        after_seq: u64,
    },

    // --- Work replication (Origin -> HQ -> Replicas) ---
    WorkChange {
        org_id: String,
        origin_office: String,
        incarnation: String,
        seq: u64,
        record_id: String,
        generation: u64,
        version: u64,
        digest_hex: String,
        record_digest_hex: String,
        codec_name: String,
        codec_version: u32,
        payload_b64: String,
        required_org_epoch: u64,
        required_org_seq: u64,
        created_ms: u64,
    },
    WorkAck {
        origin_office: String,
        incarnation: String,
        seq: u64,
        proof_hex: String,
    },

    // --- Cross-office messaging & wake relay (Origin -> HQ -> Destination) ---
    MessageDeliver {
        org_id: String,
        origin_office: String,
        destination_office: String,
        message_id: String,
        fingerprint_hex: String,
        payload_b64: String,
        deadline_ms: u64,
    },
    MessageReceipt {
        org_id: String,
        origin_office: String,
        destination_office: String,
        message_id: String,
        fingerprint_hex: String,
        delivered: bool,
        #[serde(default)]
        relayed_by_hq: bool,
        deadline_ms: u64,
    },
    WakeNotify {
        org_id: String,
        origin_office: String,
        destination_office: String,
        employee_id: String,
        kind: String,
        source_id: String,
        sender_id: String,
        sender_name: String,
        sender_is_person: bool,
        conversation_id: Option<String>,
        summary: String,
    },
    /// Explicit receiver acknowledgement after durably consuming a pushed `"wake"` or
    /// forwarded `"receipt"` relay item from HQ (`TASK-52` M-19).
    RelayAck {
        relay_id: String,
    },

    // --- Multi-office L4 provider permit & cooldown sync ---
    PermitSyncRequest {
        request_id: String,
        connection_id: String,
        employee_id: String,
        task_id: Option<String>,
        model_id: String,
        is_failover_switch: bool,
    },
    PermitSyncResponse {
        request_id: String,
        granted: bool,
        permit_id: Option<String>,
        coordinator_generation: Option<u64>,
        reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cooldown_until: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        occupied: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_capacity: Option<u64>,
    },
    /// Heartbeat renewal of an HQ-issued `HqPool` permit from a satellite (`TASK-52` #12).
    PermitRenewRequest {
        request_id: String,
        permit_id: String,
        coordinator_generation: u64,
    },
    PermitRenewResponse {
        request_id: String,
        renewed: bool,
    },
    PermitReleaseNotify {
        permit_id: String,
        release_pending: bool,
    },
    CooldownNotify {
        connection_id: String,
        resets_at: Option<String>,
        reason: Option<String>,
    },

    // --- HQ -> Satellite proxied session-control RPCs (`TASK-143` AC #3) ---
    /// Authenticated session-control RPC forwarded by HQ to the satellite office where the target
    /// agent is placed (`member.session_inspect|approval|stop|start|transcript|clear_stall|grants`).
    SessionRpcRequest {
        request_id: String,
        caller_member_id: String,
        #[serde(default)]
        caller_is_owner: bool,
        method: String,
        params_json: String,
    },
    /// Reply from the placed satellite office for a [`SyncFrame::SessionRpcRequest`], encoding
    /// the serialized `org_proto::Outcome` JSON.
    SessionRpcResponse {
        request_id: String,
        outcome_json: String,
    },

    // --- Keepalive / explicit refusal ---
    Ping {
        nonce: u64,
    },
    Pong {
        nonce: u64,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Serialize, Deserialize)]
struct WireEnvelope {
    v: u32,
    #[serde(flatten)]
    frame: SyncFrame,
}

#[derive(Debug)]
pub enum FrameCodecError {
    TooLarge(usize),
    UnsupportedVersion(u32),
    FrameTimeout,
    Json(serde_json::Error),
    Io(io::Error),
}

impl std::fmt::Display for FrameCodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge(n) => write!(
                f,
                "federation frame size {n} exceeds limit {MAX_ENCODED_FRAME_BYTES}"
            ),
            Self::UnsupportedVersion(v) => write!(f, "unsupported federation wire version {v}"),
            Self::FrameTimeout => write!(f, "federation partial frame read timed out"),
            Self::Json(e) => write!(f, "federation frame JSON error: {e}"),
            Self::Io(e) => write!(f, "federation frame I/O error: {e}"),
        }
    }
}

impl std::error::Error for FrameCodecError {}

/// Encodes `frame` as a bounded JSON byte payload without length prefix.
pub fn encode_sync_frame(frame: &SyncFrame) -> Result<Vec<u8>, FrameCodecError> {
    let bytes = serde_json::to_vec(&WireEnvelope {
        v: SYNC_WIRE_VERSION,
        frame: frame.clone(),
    })
    .map_err(FrameCodecError::Json)?;
    if bytes.len() > MAX_ENCODED_FRAME_BYTES {
        return Err(FrameCodecError::TooLarge(bytes.len()));
    }
    Ok(bytes)
}

/// Decodes a bounded JSON byte payload, checking size before deserialization.
pub fn decode_sync_frame(bytes: &[u8]) -> Result<SyncFrame, FrameCodecError> {
    if bytes.len() > MAX_ENCODED_FRAME_BYTES {
        return Err(FrameCodecError::TooLarge(bytes.len()));
    }
    let env: WireEnvelope = serde_json::from_slice(bytes).map_err(FrameCodecError::Json)?;
    if env.v != SYNC_WIRE_VERSION {
        return Err(FrameCodecError::UnsupportedVersion(env.v));
    }
    Ok(env.frame)
}

/// Writes a 4-byte big-endian length-prefixed bounded federation frame.
pub fn write_sync_frame(w: &mut impl Write, frame: &SyncFrame) -> Result<(), FrameCodecError> {
    let bytes = encode_sync_frame(frame)?;
    let len = u32::try_from(bytes.len()).map_err(|_| FrameCodecError::TooLarge(bytes.len()))?;
    w.write_all(&len.to_be_bytes())
        .map_err(FrameCodecError::Io)?;
    w.write_all(&bytes).map_err(FrameCodecError::Io)?;
    w.flush().map_err(FrameCodecError::Io)
}

/// Stateful frame reader that preserves partial header/body bytes across `WouldBlock`/`TimedOut`
/// reads so slow or fragmented TCP/TLS streams never desynchronize (`TASK-52` / `X-05`).
#[derive(Debug, Default)]
pub struct FrameReader {
    header: [u8; 4],
    header_read: usize,
    body: Vec<u8>,
    body_read: usize,
    frame_started_at: Option<std::time::Instant>,
}

impl FrameReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_partial_frame(&self) -> bool {
        self.header_read > 0 || !self.body.is_empty()
    }

    /// Reads the next complete frame from `r`. On `WouldBlock`/`TimedOut`, preserves any already
    /// read prefix unless `frame_timeout` has elapsed since the first byte of the current frame,
    /// in which case [`FrameCodecError::FrameTimeout`] is returned so the caller closes the stream.
    pub fn read_frame(
        &mut self,
        r: &mut impl Read,
        frame_timeout: std::time::Duration,
    ) -> Result<Option<SyncFrame>, FrameCodecError> {
        if self.header_read < 4
            && !read_into_buf(
                r,
                &mut self.header,
                &mut self.header_read,
                &mut self.frame_started_at,
                frame_timeout,
                true,
            )?
        {
            return Ok(None);
        }
        if self.body.is_empty() {
            let len = u32::from_be_bytes(self.header) as usize;
            if len == 0 || len > MAX_ENCODED_FRAME_BYTES {
                return Err(FrameCodecError::TooLarge(len));
            }
            self.body = vec![0u8; len];
            self.body_read = 0;
        }
        read_into_buf(
            r,
            &mut self.body,
            &mut self.body_read,
            &mut self.frame_started_at,
            frame_timeout,
            false,
        )?;
        let decoded = decode_sync_frame(&self.body)?;
        self.header_read = 0;
        self.body.clear();
        self.body_read = 0;
        self.frame_started_at = None;
        Ok(Some(decoded))
    }
}

fn read_into_buf(
    r: &mut impl Read,
    buf: &mut [u8],
    offset: &mut usize,
    started_at: &mut Option<std::time::Instant>,
    frame_timeout: std::time::Duration,
    allow_clean_eof: bool,
) -> Result<bool, FrameCodecError> {
    while *offset < buf.len() {
        match r.read(&mut buf[*offset..]) {
            Ok(0) if allow_clean_eof && *offset == 0 => return Ok(false),
            Ok(0) => {
                return Err(FrameCodecError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "eof mid-frame",
                )));
            }
            Ok(n) => {
                if started_at.is_none() {
                    *started_at = Some(std::time::Instant::now());
                }
                *offset += n;
                if *offset < buf.len() && started_at.is_some_and(|s| s.elapsed() >= frame_timeout) {
                    return Err(FrameCodecError::FrameTimeout);
                }
            }
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                if started_at.is_some_and(|s| s.elapsed() >= frame_timeout) {
                    return Err(FrameCodecError::FrameTimeout);
                }
                return Err(FrameCodecError::Io(e));
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(FrameCodecError::Io(e)),
        }
    }
    Ok(true)
}

/// Reads a 4-byte big-endian length-prefixed federation frame, enforcing
/// [`MAX_ENCODED_FRAME_BYTES`] BEFORE allocating the body buffer (`X-05`).
pub fn read_sync_frame(r: &mut impl Read) -> Result<Option<SyncFrame>, FrameCodecError> {
    let mut reader = FrameReader::new();
    reader.read_frame(r, std::time::Duration::from_secs(30))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct ChunkyReader {
        data: Vec<u8>,
        pos: usize,
        yield_every: usize,
        reads: usize,
    }

    impl Read for ChunkyReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.reads += 1;
            if self.reads.is_multiple_of(2) {
                return Err(io::Error::new(io::ErrorKind::WouldBlock, "split"));
            }
            if self.pos >= self.data.len() {
                return Ok(0);
            }
            let take = self
                .yield_every
                .min(buf.len())
                .min(self.data.len() - self.pos);
            buf[..take].copy_from_slice(&self.data[self.pos..self.pos + take]);
            self.pos += take;
            Ok(take)
        }
    }

    #[test]
    fn round_trips_sync_frame_and_enforces_size_bound_before_allocating() {
        let frame = SyncFrame::EnrollPending {
            request_id: "req-1".into(),
            check_code: "ABCD-EFGH".into(),
        };
        let mut buf = Vec::new();
        write_sync_frame(&mut buf, &frame).unwrap();
        let decoded = read_sync_frame(&mut Cursor::new(&buf)).unwrap().unwrap();
        assert_eq!(decoded, frame);

        // Oversize length header must be refused immediately without allocating.
        let huge = ((MAX_ENCODED_FRAME_BYTES + 1) as u32).to_be_bytes();
        let err = read_sync_frame(&mut Cursor::new(&huge)).unwrap_err();
        assert!(matches!(err, FrameCodecError::TooLarge(_)));
    }

    #[test]
    fn frame_reader_preserves_partial_bytes_across_would_block() {
        let f1 = SyncFrame::Ping { nonce: 11 };
        let f2 = SyncFrame::Pong { nonce: 22 };
        let mut wire = Vec::new();
        write_sync_frame(&mut wire, &f1).unwrap();
        write_sync_frame(&mut wire, &f2).unwrap();

        let mut chunky = ChunkyReader {
            data: wire,
            pos: 0,
            yield_every: 2,
            reads: 0,
        };
        let mut reader = FrameReader::new();
        let mut decoded = Vec::new();
        loop {
            match reader.read_frame(&mut chunky, std::time::Duration::from_secs(5)) {
                Ok(Some(frame)) => decoded.push(frame),
                Ok(None) => break,
                Err(FrameCodecError::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(other) => panic!("unexpected error: {other}"),
            }
        }
        assert_eq!(decoded, vec![f1, f2]);
    }

    #[test]
    fn round_trips_session_rpc_frames() {
        let req = SyncFrame::SessionRpcRequest {
            request_id: "srpc-42".into(),
            caller_member_id: "0190a5d2-0000-7000-8000-000000000001".into(),
            caller_is_owner: true,
            method: "member.session_clear_stall".into(),
            params_json: r#"{"employee_id":"0190a5d2-0000-7000-8000-000000000002"}"#.into(),
        };
        let resp = SyncFrame::SessionRpcResponse {
            request_id: "srpc-42".into(),
            outcome_json: r#"{"result":{"state":"idle"}}"#.into(),
        };
        let mut buf = Vec::new();
        write_sync_frame(&mut buf, &req).unwrap();
        write_sync_frame(&mut buf, &resp).unwrap();
        let mut cursor = Cursor::new(&buf);
        assert_eq!(read_sync_frame(&mut cursor).unwrap().unwrap(), req);
        assert_eq!(read_sync_frame(&mut cursor).unwrap().unwrap(), resp);
    }
}
