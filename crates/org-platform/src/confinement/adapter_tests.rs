//! Tests that need a real adapter (macOS or GNU/Linux).

#[test]
fn entropy_fills_every_chunk_of_a_buffer_longer_than_one_call() {
    // 256 bytes is one getentropy call; this buffer needs three, the last one short.
    let mut bytes = [0u8; 600];
    super::fill_entropy(&mut bytes).unwrap();
    for chunk in bytes.chunks(200) {
        assert_ne!(chunk, [0u8; 200]);
    }
    super::fill_entropy(&mut []).unwrap();
}
