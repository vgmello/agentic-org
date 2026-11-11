//! The DER inside a PEM text (RFC 7468), for `tls.set`'s journal fingerprint (X-02 §6.2): the
//! body of the first block with one of the wanted labels, base64-decoded strictly.
//!
//! This is not a certificate or key parser: it checks no ASN.1. The host's own parse of the pair
//! (`tls_invalid`) comes first, so a pair this module refuses is never fingerprinted.

use std::fmt;

/// The labels of `tls.set`'s certificate block.
pub(crate) const CERTIFICATE: &[&str] = &["CERTIFICATE"];

/// The labels of `tls.set`'s key block: PKCS #8, PKCS #1 (RSA) and SEC 1 (EC). An encrypted key
/// (`ENCRYPTED PRIVATE KEY`) isn't one of them.
pub(crate) const PRIVATE_KEY: &[&str] = &["PRIVATE KEY", "RSA PRIVATE KEY", "EC PRIVATE KEY"];

/// Decoded DER bytes, zeroed on drop: for a key they are the secret.
pub(crate) struct Der(Vec<u8>);

impl Der {
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for Der {
    fn drop(&mut self) {
        zero(&mut self.0);
    }
}

/// A text with no block of the wanted labels, or one whose body isn't strict base64. It never
/// quotes the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InvalidPem;

impl fmt::Display for InvalidPem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("no PEM block of the expected label with a valid base64 body")
    }
}

/// The DER of the first block in `text` whose label is one of `labels`: the bytes between a
/// `-----BEGIN <label>-----` and the next `-----END <label>-----`, each at the start of a line
/// (RFC 7468), base64-decoded. Text before the block and other blocks around it are ignored; only
/// that block's body is decoded.
pub(crate) fn first_block(text: &str, labels: &[&str]) -> Result<Der, InvalidPem> {
    let (begin_at, label) = labels
        .iter()
        .filter_map(|label| {
            line_start_find(text, &format!("-----BEGIN {label}-----"), 0).map(|at| (at, *label))
        })
        .min_by_key(|&(at, _)| at)
        .ok_or(InvalidPem)?;
    let body_at = begin_at + format!("-----BEGIN {label}-----").len();
    let end_at =
        line_start_find(text, &format!("-----END {label}-----"), body_at).ok_or(InvalidPem)?;
    decode_base64(&text[body_at..end_at])
}

/// The first `marker` in `text` at or after `from` that starts a line.
fn line_start_find(text: &str, marker: &str, from: usize) -> Option<usize> {
    text[from..]
        .match_indices(marker)
        .map(|(at, _)| from + at)
        .find(|&at| at == 0 || text.as_bytes()[at - 1] == b'\n')
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
    zero(&mut quantum);
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

/// Overwrite with zeros in a way the optimizer can't remove (best effort, as `org-proto` does for
/// its secrets).
// The crate denies unsafe code. This is the one exception: a plain write of zeros to bytes that
// are about to be dropped is a dead store the optimizer may remove, and safe Rust has no
// volatile write. `write_volatile` through an exclusive reference is sound.
#[allow(unsafe_code)]
fn zero(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        // SAFETY: `b` is a valid, aligned, exclusive reference to a u8.
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
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
        ] {
            assert!(first_block(bad, PRIVATE_KEY).is_err(), "{bad:?}");
        }
        assert!(first_block(text, CERTIFICATE).is_err());
    }
}
