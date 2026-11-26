//! Tests of the contract that run on every target.

use super::{refused, wipe, SecretBytes};
use crate::Refused;
use std::time::Duration;

#[test]
fn the_refusing_adapter_names_the_capability() {
    let Err(Refused::Unsupported { capability, os }) =
        refused::find_generic_password("any service", 4096, Duration::from_secs(1))
    else {
        panic!("the refusing adapter must refuse");
    };
    assert_eq!(capability, "keychain");
    assert_eq!(os, std::env::consts::OS);
}

#[test]
fn secret_bytes_never_print() {
    let mut secret = SecretBytes::with_capacity(8).expect("eight bytes");
    secret.extend_within_capacity(b"hunter2");
    assert_eq!(format!("{secret:?}"), "<redacted>");
    assert_eq!(format!("{:?}", Some(&secret)), "Some(<redacted>)");
    assert_eq!(secret.as_slice(), b"hunter2");
}

#[test]
fn secret_bytes_fill_only_their_reserved_capacity() {
    let mut secret = SecretBytes::with_capacity(4).expect("four bytes");
    let capacity = secret.bytes.capacity();
    let address = secret.as_slice().as_ptr();
    assert_eq!(secret.extend_within_capacity(b"ab"), 2);
    let taken = secret.extend_within_capacity(&[b'c'; 64]);
    assert_eq!(taken, capacity - 2);
    assert_eq!(secret.as_slice().len(), capacity);
    assert_eq!(secret.extend_within_capacity(b"d"), 0);
    // Never reallocated, so no copy was left behind.
    assert_eq!(secret.bytes.capacity(), capacity);
    assert_eq!(secret.as_slice().as_ptr(), address);
}

#[test]
fn wiping_zeroes_every_byte_spare_capacity_included() {
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(b"secret");
    let capacity = bytes.capacity();
    wipe(&mut bytes);
    assert_eq!(bytes.len(), capacity);
    assert!(bytes.iter().all(|&byte| byte == 0));

    let mut scratch = SecretBytes::zeroed(16);
    scratch.as_mut_slice().copy_from_slice(&[0xa5; 16]);
    wipe(&mut scratch.bytes);
    assert!(scratch.as_slice().iter().all(|&byte| byte == 0));
}
