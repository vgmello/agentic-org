//! The archive's tar dialect, written and read by this crate alone: ustar headers with
//! normalized metadata (owner and group 0, modification time 0, mode `0600` for files and
//! `0700` for folders), and, for a path ustar cannot hold, one PAX extended header carrying
//! only `path`. Nothing else (links, devices, GNU extensions, other PAX keys) is ever valid.

use std::io;

use tar::{EntryType, Header};

/// The most a PAX extended header may hold.
pub(crate) const MAX_PAX_BYTES: u64 = 4096;
/// The ustar name of a PAX extended header.
const PAX_NAME: &str = "././@PaxHeader";
/// The ustar name of a member whose path the preceding PAX header carries.
const LONG_NAME: &str = "durable/././@LongPath";

fn header(kind: EntryType, size: u64, mode: u32) -> Header {
    let mut header = Header::new_ustar();
    header.set_entry_type(kind);
    header.set_size(size);
    header.set_mode(mode);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header
}

/// An extended header and its one `path` record.
pub(crate) type Extended = (Header, Vec<u8>);

/// The headers of one member named `path`: a PAX header with its record first when ustar
/// cannot hold the name.
pub(crate) fn member_headers(
    path: &str,
    kind: EntryType,
    size: u64,
) -> io::Result<(Option<Extended>, Header)> {
    let mode = if kind == EntryType::Directory {
        0o700
    } else {
        0o600
    };
    let mut member = header(kind, size, mode);
    if member.set_path(path).is_ok() {
        member.set_cksum();
        return Ok((None, member));
    }
    let record = pax_record("path", path);
    let mut pax = header(EntryType::XHeader, record.len() as u64, 0o600);
    pax.set_path(PAX_NAME)?;
    pax.set_cksum();
    member.set_path(LONG_NAME)?;
    member.set_cksum();
    Ok((Some((pax, record)), member))
}

/// One PAX record: `<length> <key>=<value>\n`, where the length counts the whole record.
fn pax_record(key: &str, value: &str) -> Vec<u8> {
    let rest = 1 + key.len() + 1 + value.len() + 1;
    let mut length = rest + 1;
    while length != rest + length.to_string().len() {
        length = rest + length.to_string().len();
    }
    format!("{length} {key}={value}\n").into_bytes()
}

/// The path a PAX extended header carries: exactly one well-formed `path` record and
/// nothing else.
pub(crate) fn parse_pax_path(data: &[u8]) -> Result<String, &'static str> {
    let text = std::str::from_utf8(data).map_err(|_| "an extended header that is not UTF-8")?;
    let (length, rest) = text
        .split_once(' ')
        .ok_or("an extended header without a record length")?;
    let length: usize = length
        .parse()
        .map_err(|_| "an extended header with a bad record length")?;
    if length != text.len() || !rest.ends_with('\n') {
        return Err("an extended header that is not exactly one record");
    }
    let value = rest[..rest.len() - 1]
        .strip_prefix("path=")
        .ok_or("an extended header with a key other than `path`")?;
    Ok(value.to_owned())
}

/// Whether `header` is a ustar header as this dialect writes them.
pub(crate) fn is_ustar(header: &Header) -> bool {
    let raw = header.as_bytes();
    header.as_ustar().is_some() && &raw[257..263] == b"ustar\0" && &raw[263..265] == b"00"
}

#[cfg(test)]
mod tests;
