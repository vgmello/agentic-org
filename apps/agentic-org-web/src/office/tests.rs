use super::*;
use std::io::{BufRead, Write};

#[test]
fn multiplexed_office_channel_correlates_out_of_order_concurrent_replies() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let (client, server) = StdUnixStream::pair().unwrap();
        let peer = std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(server.try_clone().unwrap());
            let mut writer = server;
            let mut lines = Vec::new();
            for _ in 0..2 {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let req: Value = serde_json::from_str(&line).unwrap();
                lines.push(req["id"].as_u64().unwrap());
            }
            // Reply out of order (second request first) to prove async correlation.
            for id in lines.into_iter().rev() {
                let reply = serde_json::json!({ "id": id, "result": { "echo": id } });
                writeln!(writer, "{reply}").unwrap();
            }
            writer.flush().unwrap();
        });
        let channels = Arc::new(Mutex::new(BTreeMap::new()));
        let channel = MultiplexedChannel::from_std(
            "office-1".into(),
            "ch-1".into(),
            client,
            channels,
            EventHub::default(),
        )
        .unwrap();
        let ch_a = Arc::clone(&channel);
        let ch_b = Arc::clone(&channel);
        let a = tokio::spawn(async move {
            let req = Request::new::<OrgSnapshotCall>(10, &Empty {}).unwrap();
            ch_a.roundtrip_raw(&req, 10).await.unwrap()
        });
        let b = tokio::spawn(async move {
            let req = Request::new::<OrgSnapshotCall>(11, &Empty {}).unwrap();
            ch_b.roundtrip_raw(&req, 11).await.unwrap()
        });
        assert_eq!(a.await.unwrap()["result"]["echo"], 10);
        assert_eq!(b.await.unwrap()["result"]["echo"], 11);
        peer.join().unwrap();
    });
}
