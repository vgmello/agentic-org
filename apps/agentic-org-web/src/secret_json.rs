//! Secret request fields decoded without an unzeroed copy.
//!
//! `serde_json` unescapes a string that holds an escape (`\"`, `\\`, `\uXXXX`, ...) into
//! its deserializer's private scratch buffer, which it frees unzeroed; only an
//! escape-free string reaches the visitor straight from the input. A password may need
//! escapes, so these fields decode as the borrowed raw JSON literal (a slice of the
//! zeroizing body buffer, never copied by `serde_json`) and are unescaped here into a
//! zeroizing buffer sized up front, which then becomes the secret newtype's own bytes.

use org_host_control::{LinkToken, Password};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;
use zeroize::Zeroizing;

pub fn password<'de, D: Deserializer<'de>>(d: D) -> Result<Password, D::Error> {
    secret(d, Password::new)
}

pub fn link<'de, D: Deserializer<'de>>(d: D) -> Result<LinkToken, D::Error> {
    secret(d, LinkToken::new)
}

fn secret<'de, D: Deserializer<'de>, T>(
    d: D,
    wrap: fn(String) -> Result<T, &'static str>,
) -> Result<T, D::Error> {
    let raw = <&'de RawValue>::deserialize(d)?;
    let text = unescape(raw.get()).ok_or_else(|| D::Error::custom("expected a secret string"))?;
    wrap(text).map_err(D::Error::custom)
}

/// The string a JSON string literal denotes, or `None` when `literal` isn't one. The
/// buffer never reallocates (unescaped text is never longer than its literal); every
/// path that doesn't hand it to the caller zeroes it on drop.
fn unescape(literal: &str) -> Option<String> {
    let inner = literal.strip_prefix('"')?.strip_suffix('"')?.as_bytes();
    let mut out = Zeroizing::new(Vec::with_capacity(inner.len()));
    let mut bytes = inner.iter().copied();
    while let Some(b) = bytes.next() {
        if b != b'\\' {
            out.push(b);
            continue;
        }
        let ch = match bytes.next()? {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                let high = hex4(&mut bytes)?;
                match high {
                    0xD800..=0xDBFF => {
                        if (bytes.next()?, bytes.next()?) != (b'\\', b'u') {
                            return None;
                        }
                        let low = hex4(&mut bytes)?;
                        if !(0xDC00..=0xDFFF).contains(&low) {
                            return None;
                        }
                        char::from_u32(0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00))?
                    }
                    _ => char::from_u32(high)?,
                }
            }
            _ => return None,
        };
        let mut utf8 = [0u8; 4];
        out.extend_from_slice(ch.encode_utf8(&mut utf8).as_bytes());
        zeroize::Zeroize::zeroize(&mut utf8);
    }
    debug_assert!(out.capacity() == inner.len());
    // `from_utf8` moves the vector in and, on failure, back out to be zeroed.
    match String::from_utf8(std::mem::take(&mut *out)) {
        Ok(text) => Some(text),
        Err(e) => {
            drop(Zeroizing::new(e.into_bytes()));
            None
        }
    }
}

fn hex4(bytes: &mut impl Iterator<Item = u8>) -> Option<u32> {
    (0..4).try_fold(0u32, |n, _| {
        Some(n * 16 + char::from(bytes.next()?).to_digit(16)?)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_unescape_to_the_text_they_denote() {
        for (literal, text) in [
            (r#""plain""#, "plain"),
            (r#""a\"b\\c\/d""#, "a\"b\\c/d"),
            (r#""\b\f\n\r\t""#, "\u{8}\u{c}\n\r\t"),
            (r#""caf\u00e9 \u20AC""#, "café €"),
            (r#""\ud83d\ude00 \u00fc""#, "\u{1F600} ü"),
        ] {
            assert_eq!(unescape(literal).as_deref(), Some(text), "{literal}");
            let expected: String = serde_json::from_str(literal).unwrap();
            assert_eq!(expected, text, "{literal}");
        }
    }

    #[test]
    fn non_strings_and_bad_escapes_are_refused() {
        for literal in [
            "1",
            "null",
            r#""\x""#,
            r#""\u12""#,
            r#""\ud83d""#,
            r#""\ud83d\u0041""#,
            r#""\ude00""#,
            r#""trailing\""#,
        ] {
            assert_eq!(unescape(literal), None, "{literal}");
        }
    }

    /// The escaped password decodes through the raw literal, not `serde_json`'s
    /// scratch; the result is the password the browser typed.
    #[test]
    fn an_escaped_password_decodes_to_the_typed_text() {
        #[derive(Deserialize)]
        struct Body {
            #[serde(deserialize_with = "password")]
            password: Password,
        }
        let body: Body =
            serde_json::from_slice(br#"{"password":"q\"uote\\ \u00e9 \ud83d\ude00"}"#).unwrap();
        assert_eq!(body.password.expose(), "q\"uote\\ é \u{1F600}");
        for bad in [
            r#"{"password":""}"#,
            r#"{"password":5}"#,
            r#"{"password":null}"#,
        ] {
            assert!(serde_json::from_str::<Body>(bad).is_err(), "{bad}");
        }
    }
}
