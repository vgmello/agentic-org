//! The DER inside a PEM text (RFC 7468): the body of the first block with one of the wanted
//! labels, base64-decoded strictly. It is the one PEM reader of X-02 (§6.2): `tls.set`'s and
//! `bootstrap`'s pair fingerprint ([`crate::ops::TlsPair::fingerprint`]) reads the pair with it,
//! and so does the host's own parse of the pair ([`all_blocks`] for the certificate chain,
//! [`first_block`] for the key), so both frame a pair the same way.
//!
//! This is not a certificate or key parser: it checks no ASN.1. A text it refuses has no
//! fingerprint, and the host answers that pair with `tls_invalid`, unstored, on every path,
//! the journal lookup included ([`crate::ops::TlsRefusal::TlsInvalid`]).
//!
//! Lines end with LF, CRLF or a lone CR (RFC 7468 §3's `eol`), in any mix. It is stricter than
//! RFC 7468's lax parsers in one way: the BEGIN marker's line must end right after the marker, so
//! trailing whitespace there (`preeb *WSP eol`) is refused, and such a pair is `tls_invalid`.

use std::fmt;

/// The labels of a pair's certificate block.
pub const CERTIFICATE: &[&str] = &["CERTIFICATE"];

/// The labels of a pair's key block: PKCS #8, PKCS #1 (RSA) and SEC 1 (EC). An encrypted key
/// (`ENCRYPTED PRIVATE KEY`) isn't one of them.
pub const PRIVATE_KEY: &[&str] = &["PRIVATE KEY", "RSA PRIVATE KEY", "EC PRIVATE KEY"];

/// Decoded DER bytes, zeroed on drop: for a key they are the secret. Not `Clone`, and its
/// `Debug` shows only the length.
pub struct Der(Vec<u8>);

impl Der {
    /// The DER. Copy it only where the copy is zeroed too.
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Der {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Der(<{} bytes>)", self.0.len())
    }
}

impl Drop for Der {
    fn drop(&mut self) {
        org_proto::zero_bytes(&mut self.0);
    }
}

/// A text with no block of the wanted labels, or one whose body isn't strict base64. It never
/// quotes the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidPem;

impl fmt::Display for InvalidPem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("no PEM block of the expected label with a valid base64 body")
    }
}

impl std::error::Error for InvalidPem {}

/// The DER of the first block in `text` whose label is one of `labels`: the bytes between a
/// `-----BEGIN <label>-----` line and the next `-----END <label>-----`, each at the start of a
/// line (RFC 7468), base64-decoded. A line ends with LF, CRLF or a lone CR. Text before the block
/// and other blocks around it are ignored; only that block's body is decoded.
pub fn first_block(text: &str, labels: &[&str]) -> Result<Der, InvalidPem> {
    block_from(text, labels, 0)?
        .map(|(der, _)| der)
        .ok_or(InvalidPem)
}

/// The DER of every block in `text` whose label is one of `labels`, in order, framed as
/// [`first_block`] frames one: for a certificate chain, `all_blocks(cert, CERTIFICATE)`, whose
/// first item is `first_block`'s. Text between the blocks is ignored. A wanted block that doesn't
/// frame or decode fails the whole text, and so does a text with none.
pub fn all_blocks(text: &str, labels: &[&str]) -> Result<Vec<Der>, InvalidPem> {
    let mut blocks = Vec::new();
    let mut from = 0;
    while let Some((der, next)) = block_from(text, labels, from)? {
        blocks.push(der);
        from = next;
    }
    if blocks.is_empty() {
        return Err(InvalidPem);
    }
    Ok(blocks)
}

/// The first block at or after `from` whose label is one of `labels`, with the offset just past
/// its END marker; `None` when no such block begins there.
fn block_from(
    text: &str,
    labels: &[&str],
    from: usize,
) -> Result<Option<(Der, usize)>, InvalidPem> {
    let Some((begin_at, label)) = labels
        .iter()
        .filter_map(|label| {
            line_start_find(text, &format!("-----BEGIN {label}-----"), from).map(|at| (at, *label))
        })
        .min_by_key(|&(at, _)| at)
    else {
        return Ok(None);
    };
    let body_at = begin_at + format!("-----BEGIN {label}-----").len();
    // RFC 7468: the body starts on the line after BEGIN. CRLF before a lone CR, so a CRLF is one
    // line end.
    let rest = &text[body_at..];
    let body_at = if rest.starts_with("\r\n") {
        body_at + 2
    } else if rest.starts_with(['\n', '\r']) {
        body_at + 1
    } else {
        return Err(InvalidPem);
    };
    let end_marker = format!("-----END {label}-----");
    let end_at = line_start_find(text, &end_marker, body_at).ok_or(InvalidPem)?;
    let der = decode_base64(&text[body_at..end_at])?;
    Ok(Some((der, end_at + end_marker.len())))
}

/// The first `marker` in `text` at or after `from` that starts a line: the text's start, or
/// right after an LF or a CR (the end of an LF, CRLF or lone CR line).
fn line_start_find(text: &str, marker: &str, from: usize) -> Option<usize> {
    text[from..]
        .match_indices(marker)
        .map(|(at, _)| from + at)
        .find(|&at| at == 0 || matches!(text.as_bytes()[at - 1], b'\n' | b'\r'))
}

/// Strict standard base64 (RFC 4648 §4): the alphabet `A-Z a-z 0-9 + /`, whole 4-character
/// quantums, `=` padding only at the end of the last one, and zero unused bits. Spaces, tabs, CR
/// and LF may appear anywhere and are skipped; any other byte fails. An empty body fails.
fn decode_base64(body: &str) -> Result<Der, InvalidPem> {
    // Room for the whole output up front, so the buffer never reallocates and leaves a copy of a
    // key behind.
    let mut out = Der(Vec::with_capacity(body.len() / 4 * 3 + 3));
    let mut quantum = [0u8; 4];
    let decoded = decode_into(body, &mut out.0, &mut quantum);
    // On every path: the last quantum holds key bits.
    org_proto::zero_bytes(&mut quantum);
    decoded?;
    if out.0.is_empty() {
        return Err(InvalidPem);
    }
    Ok(out)
}

/// [`decode_base64`]'s loop, writing into `out` through the caller's `quantum`, so the caller
/// zeroes both whatever happens.
fn decode_into(body: &str, out: &mut Vec<u8>, quantum: &mut [u8; 4]) -> Result<(), InvalidPem> {
    let mut filled = 0;
    let mut padding = 0;
    for &c in body.as_bytes() {
        if matches!(c, b' ' | b'\t' | b'\r' | b'\n') {
            continue;
        }
        if padding > 0 && c != b'=' {
            // Data after padding.
            return Err(InvalidPem);
        }
        quantum[filled] = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            // Padding fills only the last one or two places of a quantum.
            b'=' if filled >= 2 => {
                padding += 1;
                0
            }
            _ => return Err(InvalidPem),
        };
        filled += 1;
        if filled == 4 {
            // The bits the padding drops must be zero, so each text has one encoding.
            let unused = match padding {
                0 => 0,
                1 => quantum[2] & 0x03,
                _ => quantum[1] & 0x0f,
            };
            if unused != 0 {
                return Err(InvalidPem);
            }
            out.push((quantum[0] << 2) | (quantum[1] >> 4));
            if padding < 2 {
                out.push((quantum[1] << 4) | (quantum[2] >> 2));
            }
            if padding < 1 {
                out.push((quantum[2] << 6) | quantum[3]);
            }
            filled = 0;
        }
    }
    if filled == 0 {
        Ok(())
    } else {
        Err(InvalidPem)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(body: &str) -> Result<Vec<u8>, InvalidPem> {
        decode_base64(body).map(|der| der.bytes().to_vec())
    }

    #[test]
    fn base64_is_strict() {
        assert_eq!(decode("TWFu").unwrap(), b"Man");
        assert_eq!(decode("TWE=").unwrap(), b"Ma");
        assert_eq!(decode("TQ==").unwrap(), b"M");
        // Whitespace anywhere is skipped.
        assert_eq!(decode(" T W\r\nF\tu\n").unwrap(), b"Man");
        assert_eq!(decode("TWFu\nTQ==\n").unwrap(), b"ManM");
        for bad in [
            "",         // empty
            " \n",      // whitespace only
            "TWF",      // not a whole quantum
            "TWFuT",    // a stray character
            "TQ=",      // short padding
            "T===",     // padding in the second place
            "TQ==TWFu", // data after padding
            "TQ=a",     // data inside padding
            "TR==",     // unused bits set
            "TWF=",     // unused bits set
            "TWFu=",    // padding after a whole quantum
            "TW-u",     // URL-safe alphabet
            "TW_u",     // URL-safe alphabet
            "TWFu\x0b", // a vertical tab isn't skipped
            "TWFu\0",   // NUL
            "TW:u",     // a header's colon
        ] {
            assert_eq!(decode(bad), Err(InvalidPem), "{bad:?}");
        }
    }

    #[test]
    fn the_first_block_of_a_wanted_label() {
        let text = "Bag Attributes\n-----BEGIN EC PARAMETERS-----\nBggqhkjOPQMBBw==\n-----END EC PARAMETERS-----\n\
                    -----BEGIN RSA PRIVATE KEY-----\nTWFu\n-----END RSA PRIVATE KEY-----\n\
                    -----BEGIN PRIVATE KEY-----\nTQ==\n-----END PRIVATE KEY-----\n";
        assert_eq!(first_block(text, PRIVATE_KEY).unwrap().bytes(), b"Man");
        assert_eq!(first_block(text, &["PRIVATE KEY"]).unwrap().bytes(), b"M");
        for bad in [
            "TWFu",
            "-----BEGIN ENCRYPTED PRIVATE KEY-----\nTWFu\n-----END ENCRYPTED PRIVATE KEY-----\n",
            // No end line, or the end line of another label.
            "-----BEGIN PRIVATE KEY-----\nTWFu\n",
            "-----BEGIN PRIVATE KEY-----\nTWFu\n-----END RSA PRIVATE KEY-----\n",
            // An empty body.
            "-----BEGIN PRIVATE KEY-----\n-----END PRIVATE KEY-----\n",
            // A begin or an end line that doesn't start a line.
            "x-----BEGIN PRIVATE KEY-----\nTWFu\n-----END PRIVATE KEY-----\n",
            "-----BEGIN PRIVATE KEY-----\nTWFu-----END PRIVATE KEY-----\n",
            "-----BEGIN PRIVATE KEY-----TWFu\n-----END PRIVATE KEY-----\n",
        ] {
            assert!(first_block(bad, PRIVATE_KEY).is_err(), "{bad:?}");
        }
        assert!(first_block(text, CERTIFICATE).is_err());
    }

    /// A chain: every block of the wanted labels, in order, the first one `first_block`'s.
    #[test]
    fn all_blocks_reads_a_chain_with_the_same_framing() {
        let chain = "leaf\r-----BEGIN CERTIFICATE-----\rTWFu\r-----END CERTIFICATE-----\r\n\
                     -----BEGIN EC PARAMETERS-----\nBggqhkjOPQMBBw==\n-----END EC PARAMETERS-----\n\
                     issuer\n-----BEGIN CERTIFICATE-----\nTQ==\n-----END CERTIFICATE-----\n";
        let blocks = all_blocks(chain, CERTIFICATE).unwrap();
        let bytes: Vec<&[u8]> = blocks.iter().map(Der::bytes).collect();
        assert_eq!(bytes, [b"Man".as_slice(), b"M".as_slice()]);
        assert_eq!(first_block(chain, CERTIFICATE).unwrap().bytes(), b"Man");
        for bad in [
            // None, or a later wanted block that doesn't frame or decode.
            "-----BEGIN EC PARAMETERS-----\nBggqhkjOPQMBBw==\n-----END EC PARAMETERS-----\n",
            "-----BEGIN CERTIFICATE-----\nTWFu\n-----END CERTIFICATE-----\n-----BEGIN CERTIFICATE-----\nTQ=\n-----END CERTIFICATE-----\n",
            "-----BEGIN CERTIFICATE-----\nTWFu\n-----END CERTIFICATE-----\n-----BEGIN CERTIFICATE-----\nTQ==\n",
        ] {
            assert!(all_blocks(bad, CERTIFICATE).is_err(), "{bad:?}");
        }
    }

    /// RFC 7468 §3: `eol = CRLF / CR / LF`. Each one, and a mix, frames the same block.
    #[test]
    fn every_line_end_frames_the_same_block() {
        for eol in ["\n", "\r\n", "\r"] {
            let text = format!(
                "Bag Attributes{eol}-----BEGIN PRIVATE KEY-----{eol}TWFu{eol}TQ=={eol}-----END PRIVATE KEY-----{eol}"
            );
            assert_eq!(
                first_block(&text, PRIVATE_KEY).unwrap().bytes(),
                b"ManM",
                "{eol:?}"
            );
        }
        let mixed = "-----BEGIN PRIVATE KEY-----\rTWFu\r\nTQ==\n-----END PRIVATE KEY-----";
        assert_eq!(first_block(mixed, PRIVATE_KEY).unwrap().bytes(), b"ManM");
        // A marker still has to start a line: one after other text on its line doesn't count.
        for bad in [
            "-----BEGIN PRIVATE KEY-----\rTWFu-----END PRIVATE KEY-----\r",
            "x\r x-----BEGIN PRIVATE KEY-----\rTWFu\r-----END PRIVATE KEY-----\r",
        ] {
            assert!(first_block(bad, PRIVATE_KEY).is_err(), "{bad:?}");
        }
        // Debug shows no key bytes.
        let der = first_block(mixed, PRIVATE_KEY).unwrap();
        assert_eq!(format!("{der:?}"), "Der(<4 bytes>)");
    }
}
