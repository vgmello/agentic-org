use super::*;
use std::io::Write;

#[test]
fn the_shared_wipe_overwrites_every_byte() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let mut secret = [0xb3; 4096];
    wipe_bytes(&mut secret);
    assert!(secret.iter().all(|byte| *byte == 0));
}

#[test]
fn secret_storage_is_wiped_on_success_error_and_unwind() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    for outcome in 0..3 {
        let before = WIPES.with(std::cell::Cell::get);
        let result = std::panic::catch_unwind(|| {
            let mut buffer = SecretBuffer::new(4096);
            buffer.as_mut().fill(0xb3);
            match outcome {
                0 => Ok(()),
                1 => Err(()),
                _ => panic!("synthetic failure"),
            }
        });
        assert_eq!(WIPES.with(std::cell::Cell::get), before + 1);
        assert_eq!(
            take_secret_wipes(),
            vec![SecretWipe {
                len: 4096,
                had_data: true,
                zeroed: true
            }]
        );
        assert_eq!(result.is_err(), outcome == 2);
    }
}

fn reader_after_one_line() -> ZeroedReader {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(b"first line with a secret\nsecond\n")
        .unwrap();
    std::io::Seek::rewind(&mut file).unwrap();
    let mut reader = ZeroedReader::new(file);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert_eq!(line, "first line with a secret\n");
    assert!(!reader.buffer_is_zero(), "the secret is in the buffer");
    reader
}

#[test]
fn it_reads_lines_and_a_line_longer_than_its_buffer() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let mut file = tempfile::tempfile().unwrap();
    let long = "x".repeat(3000);
    file.write_all(format!("{long}\nshort\n").as_bytes())
        .unwrap();
    std::io::Seek::rewind(&mut file).unwrap();
    let mut reader = ZeroedReader::new(file);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert_eq!(line, format!("{long}\n"));
    line.clear();
    reader.read_line(&mut line).unwrap();
    assert_eq!(line, "short\n");
    line.clear();
    assert_eq!(reader.read_line(&mut line).unwrap(), 0, "end of file");
}

#[test]
fn taking_the_file_wipes_the_buffer_in_place() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let mut reader = reader_after_one_line();
    let _file = reader.take_inner();
    assert!(reader.buffer_is_zero());
}

#[test]
fn dropping_the_reader_wipes_its_buffer() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let reader = reader_after_one_line();
    let before = WIPES.with(std::cell::Cell::get);
    drop(reader);
    assert_eq!(
        WIPES.with(std::cell::Cell::get),
        before + 1,
        "Drop did not wipe"
    );
}
