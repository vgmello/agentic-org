//! Receipts: the store's own record family. A receipt is the file
//! `recovery/commands/<operation-id>.json` with the original reply inline. It is written
//! by the same entry as the operation's files, and projected like any other file.

use serde::{Deserialize, Serialize};

use crate::{
    change::{Ack, Actor, Bytes, Id, Outcome, Receipt, Sha256, Timestamp},
    engine::{CorePort, Engine, Projector},
    error::ProjectionError,
    path::DurablePath,
};

/// The directory that holds every receipt.
pub const RECEIPTS: &str = "recovery/commands";
const FORMAT: &str = "command-receipt/1";

/// Default receipt retention window: 30 days (`TASK-33` / B1c).
/// Receipts within this window are retained for idempotent retry replay unless the hard count cap
/// is exceeded.
pub const DEFAULT_RECEIPT_RETENTION_DAYS: u64 = 30;
/// Default hard upper bound on retained command receipts per office (`TASK-33` / B1c).
pub const DEFAULT_MAX_RETAINED_RECEIPTS: usize = 50_000;

/// Retention and pruning bounds for command receipts (`recovery/commands/<operation-id>.json`).
///
/// Pruning rules:
/// 1. Any receipt older than `max_age_ms` relative to `now` is eligible for pruning.
/// 2. If more than `max_entries` receipts remain, the oldest receipts beyond `max_entries`
///    (ordered by `(at, id)`) are also selected for pruning.
/// 3. Receipts within both bounds are always preserved so client retries receive the original reply.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReceiptRetentionPolicy {
    pub max_age_ms: u64,
    pub max_entries: usize,
}

impl Default for ReceiptRetentionPolicy {
    fn default() -> Self {
        Self {
            max_age_ms: DEFAULT_RECEIPT_RETENTION_DAYS * 24 * 60 * 60 * 1_000,
            max_entries: DEFAULT_MAX_RETAINED_RECEIPTS,
        }
    }
}

impl ReceiptRetentionPolicy {
    /// Returns the operation IDs eligible for pruning at `now`, ordered oldest-first.
    pub fn prunable_ids(&self, receipts: &[(&Id, Timestamp)], now: Timestamp) -> Vec<Id> {
        let cutoff = now.0.saturating_sub(self.max_age_ms);
        let mut expired = Vec::new();
        let mut active = Vec::new();
        for &(id, at) in receipts {
            if at.0 < cutoff {
                expired.push((at.0, id.clone()));
            } else {
                active.push((at.0, id.clone()));
            }
        }
        expired.sort();
        active.sort();
        let mut prunable: Vec<Id> = expired.into_iter().map(|(_, id)| id).collect();
        if active.len() > self.max_entries {
            let excess = active.len() - self.max_entries;
            prunable.extend(active.into_iter().take(excess).map(|(_, id)| id));
        }
        prunable
    }
}

pub fn receipt_path(id: &Id) -> DurablePath {
    DurablePath::parse(&format!("{RECEIPTS}/{id}.json")).expect("an ID is one safe component")
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptFile {
    format: String,
    operation_id: Id,
    kind: String,
    actor: Actor,
    fingerprint: Sha256,
    outcome: Outcome,
    ack: Ack,
    ordinal: u64,
    at: Timestamp,
    result: ResultFile,
}

/// The reply, as text when it is UTF-8 and as lowercase hex otherwise.
#[derive(Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "snake_case",
    tag = "encoding",
    content = "value"
)]
enum ResultFile {
    Utf8(String),
    Hex(String),
}

/// The exact bytes of a receipt file: one line of JSON in this field order.
pub fn encode_receipt(receipt: &Receipt) -> Bytes {
    let result = match std::str::from_utf8(&receipt.reply) {
        Ok(text) => ResultFile::Utf8(text.to_owned()),
        Err(_) => ResultFile::Hex(receipt.reply.iter().map(|b| format!("{b:02x}")).collect()),
    };
    let file = ReceiptFile {
        format: FORMAT.to_owned(),
        operation_id: receipt.id.clone(),
        kind: receipt.kind.clone(),
        actor: receipt.actor.clone(),
        fingerprint: receipt.fingerprint,
        outcome: receipt.outcome,
        ack: receipt.ack,
        ordinal: receipt.ordinal,
        at: receipt.at,
        result,
    };
    let mut bytes = serde_json::to_vec(&file).expect("a receipt is plain data");
    bytes.push(b'\n');
    bytes.into()
}

pub fn decode_receipt(path: &DurablePath, bytes: &[u8]) -> Result<Receipt, ProjectionError> {
    let refuse = |reason: String| ProjectionError::new(path, reason);
    let file: ReceiptFile = serde_json::from_slice(bytes).map_err(|e| refuse(e.to_string()))?;
    if file.format != FORMAT {
        return Err(refuse(format!("unknown receipt format {:?}", file.format)));
    }
    if receipt_path(&file.operation_id) != *path {
        return Err(refuse("the receipt is not at its operation's path".into()));
    }
    let reply: Vec<u8> = match file.result {
        ResultFile::Utf8(text) => text.into_bytes(),
        ResultFile::Hex(text) => {
            let digit = |b: u8| (b as char).to_digit(16).filter(|_| !b.is_ascii_uppercase());
            let (pairs, rest) = text.as_bytes().as_chunks::<2>();
            let decoded: Option<Vec<u8>> = pairs
                .iter()
                .map(|[high, low]| Some((digit(*high)? << 4 | digit(*low)?) as u8))
                .collect();
            decoded
                .filter(|_| rest.is_empty())
                .ok_or_else(|| refuse("the result is not lowercase hex".into()))?
        }
    };
    Ok(Receipt {
        id: file.operation_id,
        kind: file.kind,
        actor: file.actor,
        fingerprint: file.fingerprint,
        outcome: file.outcome,
        ack: file.ack,
        ordinal: file.ordinal,
        at: file.at,
        reply: reply.into(),
    })
}

/// Projects receipt files through [`CorePort::put_receipt`], for every engine.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReceiptProjector;

impl ReceiptProjector {
    pub fn owns_path(path: &DurablePath) -> bool {
        path.as_str()
            .strip_prefix(RECEIPTS)
            .is_some_and(|rest| rest.starts_with('/'))
    }
}

impl<E: Engine> Projector<E> for ReceiptProjector {
    fn owns(&self, path: &DurablePath) -> bool {
        Self::owns_path(path)
    }

    fn apply(
        &self,
        tx: &mut E::Tx,
        path: &DurablePath,
        bytes: Option<&[u8]>,
    ) -> Result<(), ProjectionError> {
        let Some(bytes) = bytes else {
            return Err(ProjectionError::new(
                path,
                "the store never removes a receipt",
            ));
        };
        let receipt = decode_receipt(path, bytes)?;
        tx.put_receipt(&receipt).map_err(|e| ProjectionError {
            path: Some(path.clone()),
            ..ProjectionError::from(e)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::id;

    fn receipt(reply: &[u8]) -> Receipt {
        Receipt {
            id: id(7),
            kind: "probe.put".into(),
            actor: Actor::Employee(id(9)),
            fingerprint: Sha256::of(b"request"),
            outcome: Outcome::Rejected,
            ack: Ack::Durable,
            ordinal: 12,
            at: Timestamp(1_700_000_000_000),
            reply: reply.to_vec().into(),
        }
    }

    #[test]
    fn a_receipt_file_has_exact_bytes_and_round_trips_any_reply() {
        let text = receipt(b"{\"ok\":true}");
        let bytes = encode_receipt(&text);
        let fingerprint = Sha256::of(b"request");
        assert_eq!(
            String::from_utf8(bytes.to_vec()).unwrap(),
            format!(
                "{{\"format\":\"command-receipt/1\",\"operation_id\":\"{}\",\"kind\":\"probe.put\",\
                 \"actor\":{{\"employee\":\"{}\"}},\"fingerprint\":\"{fingerprint}\",\
                 \"outcome\":\"rejected\",\"ack\":\"durable\",\"ordinal\":12,\
                 \"at\":1700000000000,\"result\":{{\"encoding\":\"utf8\",\
                 \"value\":\"{{\\\"ok\\\":true}}\"}}}}\n",
                id(7),
                id(9)
            )
        );
        let path = receipt_path(&id(7));
        assert_eq!(path.as_str(), format!("recovery/commands/{}.json", id(7)));
        assert_eq!(decode_receipt(&path, &bytes).unwrap(), text);

        for reply in [&b""[..], &[0xff, 0x00, 0x80, 0x7f]] {
            let binary = receipt(reply);
            assert_eq!(
                decode_receipt(&path, &encode_receipt(&binary)).unwrap(),
                binary
            );
        }
    }

    #[test]
    fn a_receipt_that_is_misplaced_unknown_or_malformed_is_refused_by_path() {
        let bytes = encode_receipt(&receipt(b"ok"));
        let elsewhere = receipt_path(&id(8));
        let error = decode_receipt(&elsewhere, &bytes).unwrap_err();
        assert_eq!(error.path, Some(elsewhere));
        assert!(error.refuses_rebuild);

        let path = receipt_path(&id(7));
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        for bad in [
            text.replace("command-receipt/1", "command-receipt/2"),
            text.replace("\"ordinal\":12,", ""),
            text.replace("\"ordinal\":12,", "\"ordinal\":12,\"extra\":1,"),
            text.replace("\"utf8\",\"value\":\"ok\"", "\"hex\",\"value\":\"0G\""),
            text.replace("\"utf8\",\"value\":\"ok\"", "\"hex\",\"value\":\"abc\""),
            text.replace("\"utf8\",\"value\":\"ok\"", "\"hex\",\"value\":\"AB\""),
        ] {
            assert_ne!(bad, text);
            assert!(decode_receipt(&path, bad.as_bytes()).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_receipt_projector_owns_exactly_the_receipt_directory() {
        for (path, owned) in [
            ("recovery/commands/a.json", true),
            ("recovery/commands", false),
            ("recovery/commands-old/a.json", false),
            ("recovery/offices/a", false),
            ("state/recovery/commands/a.json", false),
        ] {
            let path = DurablePath::parse(path).unwrap();
            assert_eq!(ReceiptProjector::owns_path(&path), owned, "{path}");
        }
    }

    #[test]
    fn receipt_retention_policy_prunes_expired_and_excess_entries_oldest_first() {
        let policy = ReceiptRetentionPolicy {
            max_age_ms: 1_000,
            max_entries: 2,
        };
        let now = Timestamp(10_000);
        let r_old = id(1);
        let r_keep_1 = id(2);
        let r_keep_2 = id(3);
        let r_keep_3 = id(4);
        let entries = [
            (&r_old, Timestamp(8_500)),    // expired (< 9_000)
            (&r_keep_1, Timestamp(9_100)), // active, oldest beyond max_entries=2
            (&r_keep_2, Timestamp(9_500)), // retained
            (&r_keep_3, Timestamp(9_900)), // retained
        ];
        assert_eq!(policy.prunable_ids(&entries, now), vec![r_old, r_keep_1]);

        let default_policy = ReceiptRetentionPolicy::default();
        assert_eq!(default_policy.max_entries, DEFAULT_MAX_RETAINED_RECEIPTS);
        assert!(default_policy.max_age_ms >= 30 * 24 * 3_600 * 1_000);

        // Boundary cases: exact cutoff kept unless over cap, underflow safe, unsorted + ties, cap=0.
        let r_at_cutoff = id(10);
        let r_tie_b = id(12);
        let r_tie_a = id(11);
        let unsorted = [
            (&r_tie_b, Timestamp(9_500)),
            (&r_at_cutoff, Timestamp(9_000)), // exact cutoff (10_000 - 1_000): active, not expired
            (&r_tie_a, Timestamp(9_500)),
        ];
        // max_entries=2: keeps r_tie_a & r_tie_b, prunes oldest active r_at_cutoff.
        assert_eq!(
            policy.prunable_ids(&unsorted, now),
            vec![r_at_cutoff.clone()]
        );
        // max_entries=1: keeps r_tie_b (newest by (at, id)), prunes r_at_cutoff then r_tie_a.
        let cap1 = ReceiptRetentionPolicy {
            max_age_ms: 1_000,
            max_entries: 1,
        };
        assert_eq!(
            cap1.prunable_ids(&unsorted, now),
            vec![r_at_cutoff.clone(), r_tie_a.clone()]
        );
        // max_entries=0: prunes all in (at, id) order.
        let cap0 = ReceiptRetentionPolicy {
            max_age_ms: 1_000,
            max_entries: 0,
        };
        assert_eq!(
            cap0.prunable_ids(&unsorted, now),
            vec![r_at_cutoff, r_tie_a, r_tie_b.clone()]
        );
        // Underflow: now < max_age_ms saturates cutoff to 0 without panicking.
        let underflow = ReceiptRetentionPolicy {
            max_age_ms: 50_000,
            max_entries: 10,
        };
        assert!(underflow
            .prunable_ids(&[(&r_tie_b, Timestamp(1))], Timestamp(5))
            .is_empty());
    }
}
