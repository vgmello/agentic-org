use super::*;

#[test]
fn a_pax_record_counts_its_own_length_and_parses_back() {
    for value in ["a", &"x".repeat(5), &"y".repeat(93), &"z".repeat(994)] {
        let record = pax_record("path", value);
        let text = std::str::from_utf8(&record).unwrap();
        let (length, _) = text.split_once(' ').unwrap();
        assert_eq!(length.parse::<usize>().unwrap(), record.len());
        assert_eq!(parse_pax_path(&record).unwrap(), value);
    }
}

#[test]
fn only_one_path_record_is_a_valid_extended_header() {
    assert!(parse_pax_path(b"13 size=1234\n").is_err());
    assert!(parse_pax_path(b"9 path=a\n9 path=b\n").is_err());
    assert!(parse_pax_path(b"99 path=a\n").is_err());
    assert!(parse_pax_path(b"9 path=a").is_err());
}

#[test]
fn a_long_path_gets_an_extended_header_and_a_short_one_does_not() {
    let (pax, short) = member_headers("durable/a/b", EntryType::Regular, 3).unwrap();
    assert!(pax.is_none());
    assert_eq!(short.mtime().unwrap(), 0);
    assert_eq!(short.mode().unwrap(), 0o600);
    let long = format!("durable/{}", "n/".repeat(200));
    let (pax, member) = member_headers(long.trim_end_matches('/'), EntryType::Regular, 3).unwrap();
    let (pax, record) = pax.unwrap();
    assert_eq!(pax.entry_type(), EntryType::XHeader);
    assert_eq!(parse_pax_path(&record).unwrap(), long.trim_end_matches('/'));
    assert!(is_ustar(&member));
}
