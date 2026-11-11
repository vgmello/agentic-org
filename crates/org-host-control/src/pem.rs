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
//! It reads the whole text line by line ([`blocks`]); lines end with LF, CRLF or a lone CR
//! (RFC 7468 §3's `eol`), in any mix. Every block must be well formed, whatever its label, and a
//! marker that is misplaced, mistyped, unmatched or nested fails the whole text rather than being
//! skipped. It is stricter than RFC 7468's lax parsers in one way: the BEGIN marker's line must
//! end right after the marker, so trailing whitespace there (`preeb *WSP eol`) is refused, and
//! such a pair is `tls_invalid`.

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

/// A text with a block that isn't well formed, or with no block of the wanted labels. It never
/// quotes the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidPem;

impl fmt::Display for InvalidPem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("no PEM block of the expected label with a valid base64 body")
    }
}

impl std::error::Error for InvalidPem {}

/// The DER of the first block in `text` whose label is one of `labels`. The whole text is read
/// first ([`blocks`]): every block in it must be well formed, whatever its label, and a text
/// with no wanted block fails.
pub fn first_block(text: &str, labels: &[&str]) -> Result<Der, InvalidPem> {
    blocks(text)?
        .into_iter()
        .find(|(label, _)| labels.contains(label))
        .map(|(_, der)| der)
        .ok_or(InvalidPem)
}

/// The DER of every block in `text` whose label is one of `labels`, in order, read as
/// [`first_block`] reads them: for a certificate chain, `all_blocks(cert, CERTIFICATE)`, whose
/// first item is `first_block`'s. A text with a block that isn't well formed fails, and so does a
/// text with no wanted block.
pub fn all_blocks(text: &str, labels: &[&str]) -> Result<Vec<Der>, InvalidPem> {
    let wanted: Vec<Der> = blocks(text)?
        .into_iter()
        .filter(|(label, _)| labels.contains(label))
        .map(|(_, der)| der)
        .collect();
    if wanted.is_empty() {
        return Err(InvalidPem);
    }
    Ok(wanted)
}

/// Every block in `text`, in order, with its label: a line-oriented state machine over the lines
/// of the text (each ending with LF, CRLF or a lone CR).
///
/// **Outside a block**, a line starting with `-----BEGIN ` must be exactly
/// `-----BEGIN <label>-----`, with nothing after it, not even whitespace, and opens a block. A
/// line starting with `-----END `, or any other line with `-----BEGIN ` or `-----END ` anywhere
/// in it, fails the text. Every other line is explanatory text and is skipped.
///
/// **Inside a block**, a line starting with `-----END ` must be exactly `-----END <label>-----`
/// with the BEGIN line's label, optionally followed by spaces or tabs, and closes the block. Any
/// other line with `-----BEGIN ` (no nesting) or `-----END ` anywhere in it fails the text. Every
/// other line is body: strict base64, in which spaces and tabs are skipped. An empty body fails.
///
/// A text that ends inside a block fails. So nothing in the text is silently skipped: a mistyped
/// or misplaced marker can't cut a chain short.
fn blocks(text: &str) -> Result<Vec<(&str, Der)>, InvalidPem> {
    const BEGIN: &str = "-----BEGIN ";
    const END: &str = "-----END ";
    const DASHES: &str = "-----";
    let mut found = Vec::new();
    // Inside a block: its label and the offset where its body starts (just past the BEGIN line).
    let mut open: Option<(&str, usize)> = None;
    for (at, line) in lines(text) {
        match open {
            None => {
                if let Some(rest) = line.strip_prefix(BEGIN) {
                    let label = rest.strip_suffix(DASHES).ok_or(InvalidPem)?;
                    if !is_label(label) {
                        return Err(InvalidPem);
                    }
                    open = Some((label, at + line.len()));
                } else if line.contains(BEGIN) || line.contains(END) {
                    // A stray END, or a marker that doesn't start its line.
                    return Err(InvalidPem);
                }
            }
            Some((label, body_at)) => {
                if let Some(rest) = line.strip_prefix(END) {
                    let tail = rest
                        .strip_prefix(label)
                        .and_then(|rest| rest.strip_prefix(DASHES))
                        .ok_or(InvalidPem)?;
                    if !tail.bytes().all(|b| matches!(b, b' ' | b'\t')) {
                        return Err(InvalidPem);
                    }
                    // The body is every line between the two markers; their line ends are
                    // skipped by the decoder like spaces and tabs.
                    found.push((label, decode_base64(&text[body_at..at])?));
                    open = None;
                } else if line.contains(BEGIN) || line.contains(END) {
                    // A nested block, or an END that doesn't start its line.
                    return Err(InvalidPem);
                }
            }
        }
    }
    if open.is_some() {
        return Err(InvalidPem);
    }
    Ok(found)
}

/// RFC 7468's label, stricter in one way: printable ASCII other than `-`, with single hyphens or
/// spaces between those characters, and never empty (RFC 7468 allows an empty label; X-02 §6.2
/// refuses it, since no TLS block has one). So it never holds a marker's five dashes.
fn is_label(label: &str) -> bool {
    label.split([' ', '-']).all(|word| {
        !word.is_empty()
            && word
                .bytes()
                .all(|b| (0x21..=0x7e).contains(&b) && b != b'-')
    })
}

/// The lines of `text` with the offset each starts at, without their line ends: LF, CRLF or a
/// lone CR (RFC 7468 §3's `eol`), in any mix. A text ending with a line end has no empty last
/// line.
fn lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut at = 0;
    std::iter::from_fn(move || {
        if at >= text.len() {
            return None;
        }
        let start = at;
        let rest = &text[start..];
        let (len, eol) = match rest.find(['\r', '\n']) {
            Some(i) if rest[i..].starts_with("\r\n") => (i, 2),
            Some(i) => (i, 1),
            None => (rest.len(), 0),
        };
        at = start + len + eol;
        Some((start, &rest[..len]))
    })
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
            "-----BEGIN PRIVATE KEY-----\nTWFu\n-----END PRIVATE KEY-----garbage\n",
            "-----BEGIN PRIVATE KEY-----\nTWFu\n-----END PRIVATE KEY-----\n  -----BEGIN PRIVATE KEY-----\n!!!\n-----END PRIVATE KEY-----\n",
            "-----BEGIN PRIVATE KEY-----\nTWFu\n-----END PRIVATE KEY-----\nTQ==\n-----END PRIVATE KEY-----\n",
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

    /// Each rule of the reader's state machine ([`blocks`]), outside a block and inside one.
    #[test]
    fn every_block_must_be_well_formed_whatever_its_label() {
        let cert = |text: &str| all_blocks(text, CERTIFICATE).map(|b| b.len());
        // Explanatory text outside a block is skipped; a block of another label is read and
        // checked, then left out.
        let ok = "subject=CN=x\n-----BEGIN FOO BAR-----\nTQ==\n-----END FOO BAR-----\n\
                  issuer: y\n-----BEGIN CERTIFICATE-----\nTWFu\n-----END CERTIFICATE-----\n";
        assert_eq!(cert(ok), Ok(1));
        // END: spaces and tabs may follow the marker; spaces and tabs in the body are skipped.
        for ok in [
            "-----BEGIN CERTIFICATE-----\nTW Fu\n-----END CERTIFICATE----- \t \n",
            "-----BEGIN CERTIFICATE-----\n\tTWFu\n-----END CERTIFICATE-----\t",
        ] {
            assert_eq!(cert(ok), Ok(1), "{ok:?}");
        }
        for (why, bad) in [
            // Outside: a BEGIN line must be exactly the marker, with nothing after it.
            ("whitespace after BEGIN", "-----BEGIN CERTIFICATE----- \nTWFu\n-----END CERTIFICATE-----\n"),
            ("a tab after BEGIN", "-----BEGIN CERTIFICATE-----\t\nTWFu\n-----END CERTIFICATE-----\n"),
            ("BEGIN without its dashes", "-----BEGIN CERTIFICATE\nTWFu\n-----END CERTIFICATE-----\n"),
            ("an empty label", "-----BEGIN -----\nTWFu\n-----END -----\n"),
            ("a label with a double space", "-----BEGIN A  B-----\nTWFu\n-----END A  B-----\n"),
            ("a label with dashes", "-----BEGIN A-----B-----\nTWFu\n-----END A-----B-----\n"),
            // Outside: a stray END.
            ("a stray END", "-----END CERTIFICATE-----\n-----BEGIN CERTIFICATE-----\nTWFu\n-----END CERTIFICATE-----\n"),
            ("a stray END after a block", "-----BEGIN CERTIFICATE-----\nTWFu\n-----END CERTIFICATE-----\n-----END CERTIFICATE-----\n"),
            // Outside: a marker that doesn't start its line.
            ("an indented BEGIN", "-----BEGIN CERTIFICATE-----\nTWFu\n-----END CERTIFICATE-----\n  -----BEGIN CERTIFICATE-----\nTQ==\n-----END CERTIFICATE-----\n"),
            ("text before BEGIN", "x-----BEGIN CERTIFICATE-----\nTWFu\n-----END CERTIFICATE-----\n"),
            ("text before a stray END", "x -----END CERTIFICATE-----\n-----BEGIN CERTIFICATE-----\nTWFu\n-----END CERTIFICATE-----\n"),
            // Inside: END with another label, including a mistyped one on either marker.
            ("BEGIN X, END Y", "-----BEGIN X-----\nTWFu\n-----END Y-----\n-----BEGIN CERTIFICATE-----\nTWFu\n-----END CERTIFICATE-----\n"),
            ("a mistyped intermediate", "-----BEGIN CERTIFICATE-----\nTWFu\n-----END CERTIFICATE-----\n-----BEGIN CERTIFICATF-----\nTQ==\n-----END CERTIFICATE-----\n"),
            ("an unmatched BEGIN FOO, then a stray END", "-----BEGIN CERTIFICATE-----\nTWFu\n-----END CERTIFICATE-----\n-----BEGIN FOO-----\nTQ==\n-----END CERTIFICATE-----\n"),
            // Inside: END followed by anything but spaces and tabs.
            ("text after END", "-----BEGIN CERTIFICATE-----\nTWFu\n-----END CERTIFICATE-----x\n"),
            ("END without its dashes", "-----BEGIN CERTIFICATE-----\nTWFu\n-----END CERTIFICATE\n"),
            // Inside: no nesting.
            ("a block nested in another", "-----BEGIN FOO-----\nTWFu\n-----BEGIN CERTIFICATE-----\nTQ==\n-----END CERTIFICATE-----\n-----END FOO-----\n"),
            ("a BEGIN inside a body line", "-----BEGIN CERTIFICATE-----\nTWFu -----BEGIN X-----\n-----END CERTIFICATE-----\n"),
            // Inside: an END that doesn't start its line.
            ("an END after body text", "-----BEGIN CERTIFICATE-----\nTWFu-----END CERTIFICATE-----\n"),
            ("an indented END", "-----BEGIN CERTIFICATE-----\nTWFu\n -----END CERTIFICATE-----\n"),
            // Inside: the body is strict base64 and not empty, in a block of any label.
            ("an empty body", "-----BEGIN CERTIFICATE-----\n-----END CERTIFICATE-----\n"),
            ("a blank body", "-----BEGIN CERTIFICATE-----\n \t\n-----END CERTIFICATE-----\n"),
            ("a header line in the body", "-----BEGIN CERTIFICATE-----\nProc-Type: 4,ENCRYPTED\nTWFu\n-----END CERTIFICATE-----\n"),
            ("a bad body in an unwanted block", "-----BEGIN FOO-----\nTQ=\n-----END FOO-----\n-----BEGIN CERTIFICATE-----\nTWFu\n-----END CERTIFICATE-----\n"),
            // The text ends inside a block.
            ("no END", "-----BEGIN CERTIFICATE-----\nTWFu\n"),
            ("no END after a block", "-----BEGIN CERTIFICATE-----\nTWFu\n-----END CERTIFICATE-----\n-----BEGIN FOO-----\nTQ==\n"),
            // No wanted block.
            ("no block", "TWFu\n"),
            ("an empty text", ""),
            ("only another label", "-----BEGIN FOO-----\nTWFu\n-----END FOO-----\n"),
        ] {
            assert_eq!(cert(bad), Err(InvalidPem), "{why}");
            assert!(first_block(bad, CERTIFICATE).is_err(), "{why}");
        }
    }

    #[test]
    fn lines_end_with_lf_crlf_or_a_lone_cr() {
        let lines: Vec<(usize, &str)> = lines("a\nb\r\nc\rd\n\ne").collect();
        assert_eq!(
            lines,
            [(0, "a"), (2, "b"), (5, "c"), (7, "d"), (9, ""), (10, "e")]
        );
        assert_eq!(super::lines("").count(), 0);
        assert_eq!(super::lines("a\r\n").collect::<Vec<_>>(), [(0, "a")]);
    }
}
