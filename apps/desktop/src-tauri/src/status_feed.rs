//! Live office status: follows the host's `office.status_changed` so the Offices screen need not poll.
//!
//! One connection of its own to the host: `hello` (and its answer) first, then `subscribe`, then `office.list`
//! on the same connection, so no change slips between the list and the events that follow it. The list and every
//! later change go to the UI as [`StatusUpdate`]s. When the connection closes (the host restarted or was
//! stopped) the feed reconnects with a growing pause, and each new connection starts with a fresh list, so a
//! change made while it was away is not lost. The feed never starts the host; it only listens.

use org_client::{Client, ClientError};
use org_proto::{OfficeInfo, OfficeStatusChanged};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// What the feed tells the UI. `Offices` replaces everything it knew (the start, and after a reconnect);
/// `Office` replaces one office's record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StatusUpdate {
    Offices { offices: Vec<OfficeInfo> },
    Office { office: OfficeInfo },
}

/// Where updates go (the UI's event channel).
pub type StatusSink = Arc<dyn Fn(StatusUpdate) + Send + Sync>;

const FIRST_PAUSE: Duration = Duration::from_secs(1);
const LONGEST_PAUSE: Duration = Duration::from_secs(30);
/// How often a quiet connection looks at `stop`.
const LOOK: Duration = Duration::from_millis(250);

/// Follow the host in `home` until `stop` is set, calling `emit` with each update.
pub fn follow(home: &Path, stop: &AtomicBool, emit: &dyn Fn(StatusUpdate)) {
    let mut pause = FIRST_PAUSE;
    while !stop.load(Ordering::SeqCst) {
        match follow_once(home, stop, emit) {
            // It was up and then went away: try again soon.
            Ok(()) => pause = FIRST_PAUSE,
            // Never got going (no host, not ready, another version): ask less and less often.
            Err(_) => pause = (pause * 2).min(LONGEST_PAUSE),
        }
        wait(pause, stop);
    }
}

fn wait(pause: Duration, stop: &AtomicBool) {
    let until = Instant::now() + pause;
    while Instant::now() < until && !stop.load(Ordering::SeqCst) {
        std::thread::sleep(LOOK.min(until.saturating_duration_since(Instant::now())));
    }
}

/// One connection: `Err` if it never reached the list, `Ok` once it ended after delivering it.
fn follow_once(
    home: &Path,
    stop: &AtomicBool,
    emit: &dyn Fn(StatusUpdate),
) -> Result<(), ClientError> {
    let client = Client::connect_host_in(home)?;
    client.hello()?;
    let events = client.subscribe()?;
    let offices = client.list_offices()?;
    emit(StatusUpdate::Offices { offices });
    while !stop.load(Ordering::SeqCst) {
        match events.recv_timeout(LOOK) {
            Ok(event) => {
                // An event this app doesn't know, or can't read, is not a reason to drop the connection.
                if let Some(Ok(data)) = event.data::<OfficeStatusChanged>() {
                    emit(StatusUpdate::Office {
                        office: data.office,
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
    use org_proto::frame::{read_message, write_message};
    use org_proto::location::host_socket_path;
    use org_proto::{
        Empty, Event, Hello, HelloResult, ListOfficesResult, OfficeList, OfficeRole, OfficeStatus,
        OfficeStatusChangedData, Request, Response, Subscribe,
    };
    use std::io::BufReader;
    use std::os::unix::net::UnixListener;
    use std::sync::Mutex;

    fn office(status: OfficeStatus) -> OfficeInfo {
        OfficeInfo {
            id: "hq1".into(),
            organization_id: "org1".into(),
            name: "HQ".into(),
            role: OfficeRole::Hq,
            host: "This Mac".into(),
            endpoint: None,
            status,
            status_reason: None,
            next_restart_at: None,
        }
    }

    fn changed(status: OfficeStatus) -> Event {
        Event::new::<OfficeStatusChanged>(
            1,
            &OfficeStatusChangedData {
                office: office(status),
            },
        )
        .unwrap()
    }

    /// A host that answers `hello`, `subscribe` and `office.list` (the list says `Active`), then sends each
    /// connection the next batch of `events` and hangs up if `hang_up` is set. Counts the connections.
    fn host(
        home: &Path,
        mut batches: Vec<Vec<Event>>,
        hang_up: bool,
    ) -> Arc<Mutex<Vec<&'static str>>> {
        let listener = UnixListener::bind(host_socket_path(home)).unwrap();
        let log = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&log);
        batches.reverse();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let events = batches.pop().unwrap_or_default();
                let seen = Arc::clone(&seen);
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut writer = stream;
                    while let Ok(Some(r)) = read_message::<Request>(&mut reader) {
                        let (name, response) = match r.method.as_str() {
                            "hello" => (
                                "hello",
                                Response::ok::<Hello>(r.id, &HelloResult::host("test", "This Mac"))
                                    .unwrap(),
                            ),
                            "subscribe" => (
                                "subscribe",
                                Response::ok::<Subscribe>(r.id, &Empty {}).unwrap(),
                            ),
                            "office.list" => (
                                "office.list",
                                Response::ok::<OfficeList>(
                                    r.id,
                                    &ListOfficesResult {
                                        offices: vec![office(OfficeStatus::Active)],
                                    },
                                )
                                .unwrap(),
                            ),
                            _ => return,
                        };
                        seen.lock().unwrap().push(name);
                        if write_message(&mut writer, &response).is_err() {
                            return;
                        }
                        if name == "office.list" {
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
        log
    }

    /// Runs the feed on a thread until `want` updates arrived (or 10 s), and returns them.
    fn collect(home: &Path, want: usize) -> Vec<StatusUpdate> {
        let stop = Arc::new(AtomicBool::new(false));
        let got = Arc::new(Mutex::new(Vec::new()));
        let (s, g, h) = (Arc::clone(&stop), Arc::clone(&got), home.to_path_buf());
        let thread = std::thread::spawn(move || follow(&h, &s, &|u| g.lock().unwrap().push(u)));
        let until = Instant::now() + Duration::from_secs(10);
        while got.lock().unwrap().len() < want && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        stop.store(true, Ordering::SeqCst);
        thread.join().unwrap();
        let updates = got.lock().unwrap().clone();
        updates
    }

    fn short_home() -> tempfile::TempDir {
        let home = tempfile::Builder::new().prefix("ao").tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("run")).unwrap();
        home
    }

    #[test]
    fn it_lists_the_offices_then_passes_on_each_change() {
        let home = short_home();
        let log = host(
            home.path(),
            vec![vec![changed(OfficeStatus::Offline)]],
            false,
        );
        let updates = collect(home.path(), 2);
        assert_eq!(
            updates,
            vec![
                StatusUpdate::Offices {
                    offices: vec![office(OfficeStatus::Active)]
                },
                StatusUpdate::Office {
                    office: office(OfficeStatus::Offline)
                },
            ]
        );
        assert_eq!(
            *log.lock().unwrap(),
            vec!["hello", "subscribe", "office.list"],
            "hello is answered before anything else, and the list comes after subscribing"
        );
    }

    #[test]
    fn it_ignores_events_it_cannot_read_and_keeps_listening() {
        let home = short_home();
        let unreadable = Event {
            event: "office.status_changed".into(),
            seq: 1,
            data: serde_json::json!({ "office": 5 }),
        };
        let unknown = Event {
            event: "something.new".into(),
            seq: 2,
            data: serde_json::json!({}),
        };
        host(
            home.path(),
            vec![vec![
                unreadable,
                unknown,
                changed(OfficeStatus::Unavailable),
            ]],
            false,
        );
        let updates = collect(home.path(), 2);
        assert_eq!(updates.len(), 2);
        assert_eq!(
            updates[1],
            StatusUpdate::Office {
                office: office(OfficeStatus::Unavailable)
            }
        );
    }

    #[test]
    fn when_the_host_hangs_up_it_reconnects_and_lists_again() {
        let home = short_home();
        let log = host(
            home.path(),
            vec![
                vec![changed(OfficeStatus::Offline)],
                vec![changed(OfficeStatus::Active)],
            ],
            true,
        );
        let updates = collect(home.path(), 4);
        let kinds: Vec<_> = updates
            .iter()
            .map(|u| matches!(u, StatusUpdate::Offices { .. }))
            .collect();
        assert_eq!(
            kinds,
            [true, false, true, false],
            "a fresh list after the reconnect"
        );
        assert_eq!(
            log.lock()
                .unwrap()
                .iter()
                .filter(|m| **m == "hello")
                .count(),
            2
        );
    }

    #[test]
    fn with_no_host_it_says_nothing_and_stops_when_told() {
        let home = short_home();
        let stop = Arc::new(AtomicBool::new(false));
        let told = Arc::clone(&stop);
        let h = home.path().to_path_buf();
        let thread = std::thread::spawn(move || {
            let heard = Mutex::new(0);
            follow(&h, &told, &|_| *heard.lock().unwrap() += 1);
            let n = *heard.lock().unwrap();
            n
        });
        std::thread::sleep(Duration::from_millis(300));
        stop.store(true, Ordering::SeqCst);
        assert_eq!(thread.join().unwrap(), 0);
    }
}
