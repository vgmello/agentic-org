//! Transfer-socket records (X-02 §5.2): channel kinds and record headers.

use org_proto::frame::FrameError;
use serde::{Deserialize, Serialize};

use super::TRANSFER_HEADER_MAX_BYTES;
use crate::LaunchError;

/// Which client a channel's descriptor serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    /// For the web process, opened with `office.open_channel` on its control connection.
    Web,
    /// For `agentic-org` on `host.sock`.
    Local,
}

/// One record on a transfer socket (X-02 §5.2): a JSON header of at most
/// [`TRANSFER_HEADER_MAX_BYTES`], with exactly [`TransferRecord::fd_count`] descriptors. A record
/// of another kind, or with another descriptor count, is fatal: the receiver closes the transfer
/// socket (a worker exits and the host restarts it with a new key).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TransferRecord {
    /// One-shot public committed HQ escrow read, on a separate connected FD.
    RecoveryPublicRead {
        version: u32,
        office_id: String,
        channel_id: String,
    },
    /// Private writer admission/checkpoint channel, never a human or agent task
    /// channel. Sent only for a previously provisioned selected office.
    WriterControl {
        office_id: String,
        channel_id: String,
    },
    /// A new host-made channel to this office: one `SOCK_STREAM` descriptor.
    Channel {
        kind: ChannelKind,
        office_id: String,
        channel_id: String,
    },
    /// Sessions that ended: end their subscriptions, release their leases and refuse their grants
    /// until they expire. No descriptor.
    Revoked { session_ids: Vec<String> },
}

impl TransferRecord {
    /// How many descriptors arrive with this record.
    pub fn fd_count(&self) -> usize {
        match self {
            TransferRecord::Channel { .. }
            | TransferRecord::WriterControl { .. }
            | TransferRecord::RecoveryPublicRead { .. } => 1,
            TransferRecord::Revoked { .. } => 0,
        }
    }
}

/// Encode a record's header. Over [`TRANSFER_HEADER_MAX_BYTES`] is refused
/// ([`FrameError::TooLargeToSend`]): split a long revocation into several records.
pub fn encode_transfer_record(record: &TransferRecord) -> Result<Vec<u8>, LaunchError> {
    if matches!(record, TransferRecord::RecoveryPublicRead { version, .. } if *version != 1) {
        return Err(LaunchError::Invalid(
            "unsupported recovery transfer version",
        ));
    }
    let bytes = serde_json::to_vec(record).map_err(LaunchError::Json)?;
    if bytes.len() > TRANSFER_HEADER_MAX_BYTES {
        return Err(LaunchError::Frame(FrameError::TooLargeToSend));
    }
    Ok(bytes)
}

/// Decode a record's header, as received in one datagram. Over [`TRANSFER_HEADER_MAX_BYTES`] is
/// refused ([`FrameError::TooLarge`]); so is anything that isn't a known record.
pub fn decode_transfer_record(bytes: &[u8]) -> Result<TransferRecord, LaunchError> {
    if bytes.len() > TRANSFER_HEADER_MAX_BYTES {
        return Err(LaunchError::Frame(FrameError::TooLarge));
    }
    let record = serde_json::from_slice(bytes).map_err(LaunchError::Json)?;
    if matches!(record, TransferRecord::RecoveryPublicRead { version, .. } if version != 1) {
        return Err(LaunchError::Invalid(
            "unsupported recovery transfer version",
        ));
    }
    Ok(record)
}
