//! Receipts: the store's own record family. A receipt is the file
//! `recovery/commands/<operation-id>.json` with the original reply inline. It is written
//! by the same entry as the operation's files, and projected like any other file.

use serde::{Deserialize, Serialize};

use crate::{
    change::{Ack, Actor, Bytes, DurablePath, Id, Outcome, Receipt, Sha256, Timestamp},
    engine::{CorePort, Engine, Projector},
    error::ProjectionError,
};

/// The directory that holds every receipt.
pub const RECEIPTS: &str = "recovery/commands";
const FORMAT: &str = "command-receipt/1";

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
}
