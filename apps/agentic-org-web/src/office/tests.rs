use super::*;
use std::io::{BufRead, Write};
use std::os::unix::net::UnixStream as StdUnixStream;

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

/// `TASK-174` AC #3: a round trip cancelled while it waits for a slot in the full 64-frame
/// write queue (as `race_until_closed` does when the client goes away) leaves no stale
/// pending entry behind; nor does one whose writer closed or whose reply timed out.
#[test]
fn a_cancelled_or_failed_round_trip_removes_its_pending_entry() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let (writer, mut write_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(CHANNEL_WRITE_QUEUE);
        let pending: PendingMap = Arc::new(Mutex::new(BTreeMap::new()));
        let channel = MultiplexedChannel {
            channel_id: "ch-1".into(),
            writer: writer.clone(),
            pending: Arc::clone(&pending),
        };
        // Nobody drains the queue: fill every slot.
        for _ in 0..CHANNEL_WRITE_QUEUE {
            writer.try_send(b"{}\n".to_vec()).unwrap();
        }
        let request = Request::new::<OrgSnapshotCall>(7, &Empty {}).unwrap();

        // Cancelled mid-send: the future is dropped while waiting for queue capacity.
        let cancelled = tokio::time::timeout(
            Duration::from_millis(20),
            channel.roundtrip_raw_with_timeout(&request, 7, Duration::from_secs(60)),
        )
        .await;
        assert!(cancelled.is_err(), "the full queue keeps the send waiting");
        assert!(
            pending.lock().unwrap().is_empty(),
            "the dropped round trip removed its entry"
        );

        // Sent, then the reply never comes: the timeout removes it.
        write_rx.recv().await.unwrap();
        let timed_out = channel
            .roundtrip_raw_with_timeout(&request, 8, Duration::from_millis(10))
            .await
            .unwrap_err();
        assert_eq!(timed_out.kind(), std::io::ErrorKind::TimedOut);
        assert!(pending.lock().unwrap().is_empty());

        // The writer closed: the failed send removes it.
        drop(write_rx);
        let closed = channel
            .roundtrip_raw_with_timeout(&request, 9, Duration::from_secs(60))
            .await
            .unwrap_err();
        assert_eq!(closed.kind(), std::io::ErrorKind::BrokenPipe);
        assert!(pending.lock().unwrap().is_empty());
    });
}

/// The reply path still works with the guard: a delivered reply is returned and the entry is
/// gone (the read loop took it).
#[test]
fn a_completed_round_trip_returns_its_reply_and_leaves_no_entry() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let (writer, mut write_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(CHANNEL_WRITE_QUEUE);
        let pending: PendingMap = Arc::new(Mutex::new(BTreeMap::new()));
        let channel = Arc::new(MultiplexedChannel {
            channel_id: "ch-1".into(),
            writer,
            pending: Arc::clone(&pending),
        });
        let answering = Arc::clone(&pending);
        let responder = tokio::spawn(async move {
            write_rx.recv().await.unwrap();
            let tx = answering.lock().unwrap().remove(&5).unwrap();
            tx.send(serde_json::json!({ "id": 5, "result": { "ok": true } }))
                .unwrap();
        });
        let request = Request::new::<OrgSnapshotCall>(5, &Empty {}).unwrap();
        let reply = channel
            .roundtrip_raw_with_timeout(&request, 5, Duration::from_secs(60))
            .await
            .unwrap();
        assert_eq!(reply["result"]["ok"], true);
        responder.await.unwrap();
        assert!(pending.lock().unwrap().is_empty());
    });
}
