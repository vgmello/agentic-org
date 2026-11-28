use super::*;
use crate::event_hub::InMemoryEventBus;
use crate::transport::local_unix::LocalUnixOfficeStream;
use org_proto::{Empty, OrgSnapshotCall};
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
        let channel = MultiplexedChannel::from_local_unix(
            "office-1".into(),
            "ch-1".into(),
            LocalUnixOfficeStream::from_std(client),
            channels,
            Arc::new(InMemoryEventBus::default()),
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
            office_id: "office-1".into(),
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
            office_id: "office-1".into(),
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

#[test]
fn multiplexed_channel_works_over_in_memory_duplex_streams_and_forwards_events_to_event_bus() {
    use crate::event_hub::HubEvent;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let (client, server) = tokio::io::duplex(4096);
        let (client_read, client_write) = tokio::io::split(client);
        let (server_read, mut server_write) = tokio::io::split(server);
        let bus = Arc::new(InMemoryEventBus::default());
        let mut sub = bus.subscribe();
        let channels = Arc::new(Mutex::new(BTreeMap::new()));
        let channel = MultiplexedChannel::from_async_halves(
            "office-duplex".into(),
            "office-duplex".into(),
            "ch-duplex".into(),
            client_read,
            client_write,
            channels,
            Arc::clone(&bus) as Arc<dyn EventBus>,
            None,
        );
        let peer = tokio::spawn(async move {
            let mut reader = BufReader::new(server_read);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let req: Value = serde_json::from_str(&line).unwrap();
            let event = serde_json::json!({
                "event": "team.created",
                "seq": 42,
                "data": { "organization_id": "org-duplex" }
            });
            server_write
                .write_all(format!("{event}\n").as_bytes())
                .await
                .unwrap();
            let reply = serde_json::json!({ "id": req["id"], "result": { "duplex": true } });
            server_write
                .write_all(format!("{reply}\n").as_bytes())
                .await
                .unwrap();
            server_write.flush().await.unwrap();
        });
        let req = Request::new::<OrgSnapshotCall>(21, &Empty {}).unwrap();
        let reply = channel.roundtrip_raw(&req, 21).await.unwrap();
        assert_eq!(reply["result"]["duplex"], true);
        let received = sub.recv().await.unwrap();
        match received {
            HubEvent::OrganizationChanged {
                org_id,
                cursor,
                event,
                ..
            } => {
                assert_eq!(org_id, "org-duplex");
                assert_eq!(cursor, "org:org-duplex:1:42");
                assert_eq!(event, "team.created");
            }
            other => panic!("unexpected event: {other:?}"),
        }
        peer.await.unwrap();
    });
}

fn synthetic_webhook_principal(org_id: &str, office_id: &str, channel_id: &str) -> Principal {
    Principal {
        grant: org_proto::Grant {
            claims: org_proto::GrantClaims {
                account: "github-webhook".into(),
                member: org_proto::github::WEBHOOK_PRINCIPAL_MEMBER.into(),
                org_id: org_id.into(),
                office_id: office_id.into(),
                channel_id: channel_id.into(),
                session_id: "01000000000000000000000001".into(),
                exp: 1_794_312_060_000,
            },
            mac: org_proto::GrantMac::from_bytes([0x11; org_proto::GRANT_MAC_BYTES]),
        },
    }
}

#[test]
fn webhook_channel_pool_reuses_single_channel_bounds_concurrency_and_reconnects_on_drop() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let pool = Arc::new(WebhookChannelPool::new(4));
        let open_count = Arc::new(AtomicUsize::new(0));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak_in_flight = Arc::new(AtomicUsize::new(0));
        let release_gate = Arc::new(tokio::sync::Notify::new());
        let four_arrived = Arc::new(tokio::sync::Notify::new());
        let server_tasks: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>> =
            Arc::new(Mutex::new(Vec::new()));
        let bus: Arc<dyn EventBus> = Arc::new(InMemoryEventBus::default());
        let next_rpc = Arc::new(AtomicU64::new(10));

        let make_opener = |open_count: Arc<AtomicUsize>,
                           in_flight: Arc<AtomicUsize>,
                           peak_in_flight: Arc<AtomicUsize>,
                           release_gate: Arc<tokio::sync::Notify>,
                           four_arrived: Arc<tokio::sync::Notify>,
                           server_tasks: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
                           bus: Arc<dyn EventBus>,
                           hold_first_wave: bool| {
            move |cache: ChannelMap| {
                let open_count = Arc::clone(&open_count);
                let in_flight = Arc::clone(&in_flight);
                let peak_in_flight = Arc::clone(&peak_in_flight);
                let release_gate = Arc::clone(&release_gate);
                let four_arrived = Arc::clone(&four_arrived);
                let server_tasks = Arc::clone(&server_tasks);
                let bus = Arc::clone(&bus);
                async move {
                    let seq = open_count.fetch_add(1, Ordering::SeqCst) + 1;
                    let ch_id = format!("wh-ch-{seq}");
                    let (client, server) = tokio::io::duplex(16 * 1024);
                    let (client_read, client_write) = tokio::io::split(client);
                    let (server_read, server_write) = tokio::io::split(server);
                    let server_write = Arc::new(tokio::sync::Mutex::new(server_write));
                    let channel = MultiplexedChannel::from_async_halves(
                        "org-1".into(),
                        "office-hq".into(),
                        ch_id,
                        client_read,
                        client_write,
                        cache,
                        bus,
                        None,
                    );
                    let handle = tokio::spawn(async move {
                        let mut reader = BufReader::new(server_read);
                        let mut line = String::new();
                        loop {
                            line.clear();
                            let Ok(n) = reader.read_line(&mut line).await else {
                                break;
                            };
                            if n == 0 {
                                break;
                            }
                            let req: Value = serde_json::from_str(&line).unwrap();
                            let id = req["id"].as_u64().unwrap();
                            let w = Arc::clone(&server_write);
                            let in_flight = Arc::clone(&in_flight);
                            let peak = Arc::clone(&peak_in_flight);
                            let gate = Arc::clone(&release_gate);
                            let arrived = Arc::clone(&four_arrived);
                            tokio::spawn(async move {
                                let cur = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                                peak.fetch_max(cur, Ordering::SeqCst);
                                if hold_first_wave && seq == 1 {
                                    if cur == 4 {
                                        arrived.notify_one();
                                    }
                                    gate.notified().await;
                                }
                                in_flight.fetch_sub(1, Ordering::SeqCst);
                                let reply = serde_json::json!({ "id": id, "result": { "seq": seq, "id": id } });
                                let mut guard = w.lock().await;
                                let _ = guard.write_all(format!("{reply}\n").as_bytes()).await;
                                let _ = guard.flush().await;
                            });
                        }
                    });
                    server_tasks.lock().unwrap().push(handle);
                    Ok(channel)
                }
            }
        };

        // Spawn 4 concurrent calls that hold all 4 semaphore permits on ONE pooled channel.
        let mut wave = Vec::new();
        for _ in 0..4 {
            let pool = Arc::clone(&pool);
            let opener = make_opener(
                Arc::clone(&open_count),
                Arc::clone(&in_flight),
                Arc::clone(&peak_in_flight),
                Arc::clone(&release_gate),
                Arc::clone(&four_arrived),
                Arc::clone(&server_tasks),
                Arc::clone(&bus),
                true,
            );
            let rpc_id = next_rpc.fetch_add(1, Ordering::SeqCst);
            wave.push(tokio::spawn(async move {
                pool.dispatch(
                    "org-1",
                    rpc_id,
                    "github.webhook.ingest",
                    serde_json::json!({}),
                    opener,
                    |office, ch| async move { Ok(synthetic_webhook_principal("org-1", &office, &ch)) },
                )
                .await
            }));
        }

        // Wait until all 4 are in flight on the single cached channel.
        tokio::time::timeout(Duration::from_secs(2), four_arrived.notified())
            .await
            .expect("4 concurrent webhooks must reach the pooled channel");
        assert_eq!(open_count.load(Ordering::SeqCst), 1);

        // A 5th concurrent webhook MUST fail closed immediately with RateLimited (never opens a
        // new Web channel or blocks indefinitely).
        let excess_err = pool
            .dispatch(
                "org-1",
                next_rpc.fetch_add(1, Ordering::SeqCst),
                "github.webhook.ingest",
                serde_json::json!({}),
                make_opener(
                    Arc::clone(&open_count),
                    Arc::clone(&in_flight),
                    Arc::clone(&peak_in_flight),
                    Arc::clone(&release_gate),
                    Arc::clone(&four_arrived),
                    Arc::clone(&server_tasks),
                    Arc::clone(&bus),
                    true,
                ),
                |office, ch| async move { Ok(synthetic_webhook_principal("org-1", &office, &ch)) },
            )
            .await
            .unwrap_err();
        assert!(matches!(
            excess_err,
            OfficeFailure::Refused(ref e) if e.code == org_proto::ErrorCode::RateLimited
        ));
        assert_eq!(open_count.load(Ordering::SeqCst), 1);
        assert_eq!(peak_in_flight.load(Ordering::SeqCst), 4);

        // Release the 4 in-flight calls and confirm all 4 complete cleanly on channel #1.
        release_gate.notify_waiters();
        for task in wave {
            let reply = task.await.unwrap().expect("pooled webhook call succeeded");
            assert_eq!(reply["seq"], 1);
        }
        assert_eq!(open_count.load(Ordering::SeqCst), 1);

        // Simulate office drop/restart: abort server task (closing duplex peer) -> read_loop
        // evicts cached channel -> next call cleanly opens replacement channel #2 and succeeds.
        for handle in server_tasks.lock().unwrap().drain(..) {
            handle.abort();
        }
        for _ in 0..50 {
            if pool.channels.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(
            pool.channels.lock().unwrap().is_empty(),
            "dropped webhook channel must be evicted from pool"
        );

        let after_drop = pool
            .dispatch(
                "org-1",
                next_rpc.fetch_add(1, Ordering::SeqCst),
                "github.webhook.ingest",
                serde_json::json!({}),
                make_opener(
                    Arc::clone(&open_count),
                    Arc::clone(&in_flight),
                    Arc::clone(&peak_in_flight),
                    Arc::clone(&release_gate),
                    Arc::clone(&four_arrived),
                    Arc::clone(&server_tasks),
                    Arc::clone(&bus),
                    false,
                ),
                |office, ch| async move { Ok(synthetic_webhook_principal("org-1", &office, &ch)) },
            )
            .await
            .expect("replacement webhook channel must reconnect cleanly");
        assert_eq!(after_drop["seq"], 2);
        assert_eq!(open_count.load(Ordering::SeqCst), 2);

        // Unknown org returns BeforeDispatch(NotFound) without caching any entry.
        let missing = pool
            .dispatch(
                "org-missing",
                next_rpc.fetch_add(1, Ordering::SeqCst),
                "github.webhook.ingest",
                serde_json::json!({}),
                |_| async { Err(OfficeFailure::BeforeDispatch(Failure::NotFound)) },
                |office, ch| async move { Ok(synthetic_webhook_principal("org-missing", &office, &ch)) },
            )
            .await
            .unwrap_err();
        assert!(matches!(missing, OfficeFailure::BeforeDispatch(Failure::NotFound)));
        assert!(!pool.channels.lock().unwrap().contains_key("org-missing"));
    });
}
