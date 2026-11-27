use super::*;
use std::io::Cursor;

#[test]
fn bounded_read_stops_at_max_plus_one() {
    let bytes = vec![b'a'; 100];
    let got = read_bounded(Cursor::new(&bytes), 16).unwrap();
    assert_eq!(got.len(), 17);
}
