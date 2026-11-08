//! Live organization changes: follows an organization's HQ office so the org screens need not be reloaded by hand.
//!
//! One connection per organization, its own (not the one commands and reads share): `hello`, bound as the app,
//! `org.snapshot` for the position, then `subscribe` from that position on the same bound connection (a second
//! `msg.bind` would end the subscription). Every `org.changed` is passed on as the position it reached, and the
//! UI reloads when that position is newer than the one it shows. When the connection closes (the office
//! restarted, the app's token changed) the feed reconnects with a growing pause, binds again, and starts from a
//! fresh snapshot, so a change made while it was away is not lost. The feed never starts anything.

use crate::pacing::{next_pause, wait, FIRST_PAUSE, LOOK};
use org_client::{Client, ClientError};
use org_proto::{Empty, OrgChanged, OrgSnapshotCall, Position};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;

/// What the feed tells the UI: the organization changed, and the log is now at `position`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OrgUpdate<'a> {
    pub organization_id: &'a str,
    pub position: Position,
}

/// Where connections to an organization's HQ office come from.
pub trait OrgSource {
    /// A new connection to the HQ office of `organization_id`, said `hello` on and bound as the app. Not shared:
    /// the feed keeps it for as long as its subscription lasts.
    fn open_hq(&self, organization_id: &str) -> Result<Client, ClientError>;
}

/// Follow `organization_id` until `stop` is set, calling `emit` with each update.
pub fn follow(
    source: &dyn OrgSource,
    organization_id: &str,
    stop: &AtomicBool,
    emit: &dyn Fn(OrgUpdate),
) {
    let mut pause = FIRST_PAUSE;
    while !stop.load(Ordering::SeqCst) {
        let delivered = follow_once(source, organization_id, stop, emit).is_ok();
        wait(pause, stop);
        pause = next_pause(pause, delivered);
    }
}

/// One connection: `Err` if it never got as far as the first snapshot's subscription, `Ok` once it ended after.
fn follow_once(
    source: &dyn OrgSource,
    organization_id: &str,
    stop: &AtomicBool,
    emit: &dyn Fn(OrgUpdate),
) -> Result<(), ClientError> {
    let client = source.open_hq(organization_id)?;
    let snapshot = client.call::<OrgSnapshotCall>(&Empty {})?;
    let events = client.subscribe_from(Some(snapshot.position))?;
    // The snapshot is the baseline: the UI may have missed changes while the feed was away.
    emit(OrgUpdate {
        organization_id,
        position: snapshot.position,
    });
    while !stop.load(Ordering::SeqCst) {
        match events.recv_timeout(LOOK) {
            Ok(event) => {
                if let Some(Ok(data)) = event.data::<OrgChanged>() {
                    emit(OrgUpdate {
                        organization_id,
                        position: data.position,
                    });
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::org_mock::empty_org;
    use org_proto::frame::{read_message, write_message};
    use org_proto::org::OrgChangedData;
    use org_proto::{Event, Hello, HelloResult, Request, Response, Subscribe};
    use std::io::BufReader;
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    fn at(seq: u64) -> Position {
        Position { epoch: 1, seq }
    }

    fn changed(seq: u64) -> Event {
        Event::new::<OrgChanged>(
            seq,
            &OrgChangedData {
                position: at(seq),
                command_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
                changes: vec![],
            },
        )
        .unwrap()
    }

    /// An HQ office whose snapshot is at `seq 1`: each connection gets the next batch of events right after it
    /// subscribes, and is hung up on afterwards when `hang_up` is set. Records what each `subscribe` asked for.
    struct Office {
        path: PathBuf,
        _dir: tempfile::TempDir,
        subscribes: Arc<Mutex<Vec<serde_json::Value>>>,
        opened: Mutex<usize>,
    }

    impl OrgSource for Office {
        fn open_hq(&self, _: &str) -> Result<Client, ClientError> {
            *self.opened.lock().unwrap() += 1;
            let client = Client::connect(&self.path)?;
            client.hello()?;
            Ok(client)
        }
    }

    fn office(batches: Vec<Vec<Event>>, hang_up: bool) -> Office {
        office_refusing(batches, hang_up, 0)
    }

    /// Like `office`, answering `snapshot_required` to the first `refusals` subscribes.
    fn office_refusing(mut batches: Vec<Vec<Event>>, hang_up: bool, refusals: usize) -> Office {
        let refusals = Arc::new(std::sync::atomic::AtomicUsize::new(refusals));
        let dir = tempfile::Builder::new().prefix("ao").tempdir().unwrap();
        let path = dir.path().join("hq.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let subscribes = Arc::new(Mutex::new(Vec::new()));
        let asked = Arc::clone(&subscribes);
        batches.reverse();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let events = batches.pop().unwrap_or_default();
                let asked = Arc::clone(&asked);
                let refusals = Arc::clone(&refusals);
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut writer = stream;
                    while let Ok(Some(r)) = read_message::<Request>(&mut reader) {
                        let response = match r.method.as_str() {
                            "hello" => {
                                Response::ok::<Hello>(r.id, &HelloResult::host("test", "This Mac"))
                                    .unwrap()
                            }
                            "org.snapshot" => {
                                let mut snapshot = empty_org();
                                snapshot.position = at(1);
                                Response::ok::<OrgSnapshotCall>(r.id, &snapshot).unwrap()
                            }
                            "subscribe" => {
                                asked.lock().unwrap().push(r.params.clone());
                                if refusals
                                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                                        n.checked_sub(1)
                                    })
                                    .is_ok()
                                {
                                    Response::error(
                                        r.id,
                                        org_proto::ApiError::new(
                                            org_proto::ErrorCode::SnapshotRequired,
                                            "take a new snapshot",
                                        ),
                                    )
                                } else {
                                    Response::ok::<Subscribe>(r.id, &Empty {}).unwrap()
                                }
                            }
                            _ => return,
                        };
                        let subscribed = r.method == "subscribe";
                        if write_message(&mut writer, &response).is_err() {
                            return;
                        }
                        if subscribed {
                            for event in &events {
                                let _ = write_message(&mut writer, event);
                            }
                            if hang_up {
                                return;
                            }
                        }
                    }
                });
            }
        });
        Office {
            path,
            _dir: dir,
            subscribes,
            opened: Mutex::new(0),
        }
    }

    fn collect(source: &Office, want: usize) -> Vec<(String, Position)> {
        let stop = Arc::new(AtomicBool::new(false));
        let got = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&got);
        let until = Instant::now() + Duration::from_secs(10);
        std::thread::scope(|scope| {
            let flag = Arc::clone(&stop);
            let thread = scope.spawn(move || {
                follow(source, "org1", &flag, &|u| {
                    seen.lock()
                        .unwrap()
                        .push((u.organization_id.to_string(), u.position));
                })
            });
            while got.lock().unwrap().len() < want && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(20));
            }
            stop.store(true, Ordering::SeqCst);
            thread.join().unwrap();
        });
        let updates = got.lock().unwrap().clone();
        updates
    }

    #[test]
    fn it_starts_from_the_snapshot_and_passes_on_each_position() {
        let source = office(vec![vec![changed(2), changed(3)]], false);
        let updates = collect(&source, 3);
        assert_eq!(
            updates,
            vec![
                ("org1".to_string(), at(1)),
                ("org1".to_string(), at(2)),
                ("org1".to_string(), at(3)),
            ]
        );
        let asked = source.subscribes.lock().unwrap();
        assert_eq!(asked.len(), 1);
        assert_eq!(
            asked[0]["from_position"],
            serde_json::json!({ "epoch": 1, "seq": 1 }),
            "subscribed from the snapshot's position, on the connection that took it"
        );
    }

    #[test]
    fn it_skips_events_it_does_not_know() {
        let unknown = Event {
            event: "something.new".into(),
            seq: 9,
            data: serde_json::json!({}),
        };
        let source = office(vec![vec![unknown, changed(2)]], false);
        let updates = collect(&source, 2);
        assert_eq!(updates.last().unwrap().1, at(2));
        assert_eq!(updates.len(), 2);
    }

    #[test]
    fn when_the_office_hangs_up_it_connects_again_and_starts_from_a_new_snapshot() {
        let source = office(vec![vec![changed(2)], vec![changed(3)]], true);
        let updates = collect(&source, 4);
        let positions: Vec<_> = updates.iter().map(|(_, p)| p.seq).collect();
        assert_eq!(positions, [1, 2, 1, 3]);
        assert_eq!(*source.opened.lock().unwrap(), 2);
    }

    #[test]
    fn when_the_office_cannot_resume_it_takes_a_new_snapshot_and_subscribes_again() {
        let source = office_refusing(vec![vec![], vec![changed(2)]], false, 1);
        let updates = collect(&source, 2);
        assert_eq!(updates.last().unwrap().1, at(2));
        assert_eq!(
            *source.opened.lock().unwrap(),
            2,
            "a new connection, a new bind, a new snapshot"
        );
        assert_eq!(source.subscribes.lock().unwrap().len(), 2);
    }

    #[test]
    fn with_no_office_it_says_nothing_and_stops_when_told() {
        struct Nobody;
        impl OrgSource for Nobody {
            fn open_hq(&self, _: &str) -> Result<Client, ClientError> {
                Err(ClientError::OfficeNotReachable)
            }
        }
        let stop = Arc::new(AtomicBool::new(false));
        let told = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            let heard = Mutex::new(0);
            follow(&Nobody, "org1", &told, &|_| *heard.lock().unwrap() += 1);
            let n = *heard.lock().unwrap();
            n
        });
        std::thread::sleep(Duration::from_millis(300));
        stop.store(true, Ordering::SeqCst);
        assert_eq!(thread.join().unwrap(), 0);
    }
}
