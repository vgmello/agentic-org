use super::*;

/// The HQ the worker created registers the inbox (`inbox.list`) and live `inbox.changed` hints
/// from its projection's HQ standing (`B6`). Satellites arrive with replication (S1).
#[test]
fn inbox_registration_follows_the_hq_standing_and_advertises_hints() {
    let _processes = process_isolation();
    let world = World::new();
    let mut run = started(&world);
    // A raw synchronous hello owns no detached client helpers across the next spawn.
    let mut reader = BufReader::new(UnixStream::connect(socket(&world)).unwrap());
    frame::write_message(
        reader.get_mut(),
        &serde_json::json!({"id":1,"method":"hello","params":{}}),
    )
    .unwrap();
    let reply: org_proto::Response = frame::read_message(&mut reader).unwrap().unwrap();
    let hello = reply.into_result::<org_proto::Hello>().unwrap();
    assert!(hello.supports("inbox.list"));
    assert!(hello.supports("inbox.changed"));
    drop(reader);
    assert!(run.host_goes_away().success());
}
