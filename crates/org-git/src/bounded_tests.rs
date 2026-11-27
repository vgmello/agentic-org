//! Bounded capture under a stdout cap, a stderr flood and a timeout, against small POSIX
//! tools so each failure mode is exercised on purpose.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::{
    capture, decode_bounded, read_capped, CaptureError, OutputLimits, MAX_BOUNDED_STDERR_BYTES,
};

fn spawn_sh(script: &str) -> std::process::Child {
    Command::new("/bin/sh")
        .args(["-c", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn /bin/sh")
}

fn limits(max_stdout: usize, timeout_ms: u64) -> OutputLimits {
    OutputLimits {
        max_stdout,
        timeout: Duration::from_millis(timeout_ms),
    }
}

#[test]
fn stdout_over_the_cap_is_cut_and_the_writer_killed() {
    let started = Instant::now();
    let out = capture(
        spawn_sh("while :; do echo yyyyyyyy; done"),
        limits(1000, 10_000),
    )
    .unwrap();
    assert!(out.stdout_truncated);
    assert_eq!(out.stdout.len(), 1000);
    assert_eq!(out.status, -1);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn output_exactly_at_the_cap_is_complete() {
    let out = capture(spawn_sh("printf 0123456789"), limits(10, 10_000)).unwrap();
    assert!(!out.stdout_truncated);
    assert_eq!(out.stdout, b"0123456789");
    assert_eq!(out.status, 0);
}

#[test]
fn a_child_past_its_timeout_is_killed() {
    let started = Instant::now();
    let err = capture(spawn_sh("sleep 30"), limits(1000, 200)).unwrap_err();
    assert!(matches!(err, CaptureError::TimedOut), "{err:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_child_that_closes_stdout_but_keeps_running_times_out() {
    let started = Instant::now();
    let err = capture(spawn_sh("exec 1>&-; sleep 30"), limits(1000, 200)).unwrap_err();
    assert!(matches!(err, CaptureError::TimedOut), "{err:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_stderr_flood_never_blocks_and_is_capped() {
    let out = capture(
        spawn_sh("i=0; while [ $i -lt 4000 ]; do echo eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee >&2; i=$((i+1)); done; printf done"),
        limits(1000, 20_000),
    )
    .unwrap();
    assert_eq!(out.stdout, b"done");
    assert_eq!(out.status, 0);
    assert_eq!(out.stderr.len(), MAX_BOUNDED_STDERR_BYTES);
}

#[test]
fn a_child_without_piped_output_is_refused_and_reaped() {
    let child = Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    assert!(matches!(
        capture(child, limits(10, 1000)),
        Err(CaptureError::Io(_))
    ));
}

#[test]
fn read_capped_reads_one_byte_past_the_cap() {
    assert_eq!(read_capped(&b"abc"[..], 3).unwrap(), b"abc");
    assert_eq!(read_capped(&b"abcd"[..], 3).unwrap(), b"abcd");
    assert_eq!(read_capped(&b"abcdef"[..], 3).unwrap(), b"abcd");
}

#[test]
fn decoding_never_splits_a_multibyte_character_at_the_cap() {
    // "é" is two bytes, "€" three and "😀" four: cut each at every byte inside it.
    for ch in ['é', '€', '😀'] {
        let mut text = "a".repeat(10);
        text.push(ch);
        text.push('z');
        let bytes = text.as_bytes();
        for max in 10..10 + ch.len_utf8() {
            let (decoded, cut) = decode_bounded(bytes, max, false);
            assert!(cut);
            assert_eq!(decoded, "a".repeat(10), "{ch} cut at {max}");
        }
        let (decoded, cut) = decode_bounded(bytes, 10 + ch.len_utf8(), false);
        assert!(cut);
        assert_eq!(decoded, format!("{}{ch}", "a".repeat(10)));
        let (whole, cut) = decode_bounded(bytes, bytes.len(), false);
        assert!(!cut);
        assert_eq!(whole, text);
    }
}

#[test]
fn output_the_capture_already_cut_drops_its_split_character() {
    // The capture keeps exactly `max` bytes and reports the cut: the split "😀" goes whole.
    let mut bytes = b"abc".to_vec();
    bytes.extend_from_slice(&"😀".as_bytes()[..3]);
    let (decoded, cut) = decode_bounded(&bytes, bytes.len(), true);
    assert!(cut);
    assert_eq!(decoded, "abc");
    // Without the hint the same bytes are a complete (invalid) output.
    let (decoded, cut) = decode_bounded(&bytes, bytes.len(), false);
    assert!(!cut);
    assert_eq!(decoded, "abc\u{FFFD}");
}

#[test]
fn decoding_replaces_invalid_bytes_and_stays_within_the_cap() {
    // Every invalid byte becomes a 3-byte U+FFFD, so the text grows past the byte budget.
    let bytes = [0xFFu8; 8];
    let (decoded, cut) = decode_bounded(&bytes, 8, false);
    assert!(cut);
    assert!(decoded.len() <= 8);
    assert!(decoded.chars().all(|c| c == '\u{FFFD}'));
    // A complete input that is invalid only at its end keeps its replacement character.
    let (decoded, cut) = decode_bounded(b"ab\xC3", 10, false);
    assert!(!cut);
    assert_eq!(decoded, "ab\u{FFFD}");
}
