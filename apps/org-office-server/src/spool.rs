//! Offline fallback startup pass and five-second backstop. No watcher dependency or event
//! loss window: every pass scans the authoritative directory, including after a restart.
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use org_mailbox::{Mailbox, MailboxError};
use org_proto::msg::SpooledSend;
use org_spool::office_ingest::{self, Delivery};

const POLL: Duration = Duration::from_secs(5);

fn pass(
    drainer: &mut office_ingest::Drain,
    folder: &Path,
    mailbox: &Mailbox,
) -> std::io::Result<()> {
    drainer.drain(
        folder,
        |id, bytes| {
            let send: SpooledSend = match serde_json::from_slice(bytes) {
                Ok(send) => send,
                Err(_) => return Delivery::Refused("malformed spooled send"),
            };
            if send.id != id {
                return Delivery::Refused("filename and message id differ");
            }
            match mailbox.ingest_spooled(send) {
                Ok(_) => Delivery::Accepted,
                Err(MailboxError::StorageError | MailboxError::Unavailable) => {
                    Delivery::Retry("message storage or routing unavailable")
                }
                Err(MailboxError::NotAuthorized | MailboxError::NotBound) => {
                    Delivery::Refused("sender is not an active hand-run agent")
                }
                Err(MailboxError::TooLarge) => Delivery::Refused("message exceeds size limit"),
                Err(MailboxError::UnknownRecipient) => Delivery::Refused("unknown recipient"),
                Err(MailboxError::AlreadyExists) => {
                    Delivery::Refused("message id content conflict")
                }
                Err(MailboxError::NotFound) => Delivery::Refused("reply parent is not visible"),
                Err(_) => Delivery::Refused("invalid spooled send"),
            }
        },
        |notice| {
            eprintln!(
                "org-office-server: spool id={} reason={}",
                notice.id, notice.reason
            )
        },
    )
}

/// Joins before the office drops; shutdown wakes the poll immediately.
pub(crate) struct Ingest {
    stop: Arc<(Mutex<bool>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl Ingest {
    /// Run one bounded startup pass before accepting connections or Ready.
    pub(crate) fn start(folder: &Path, mailbox: Arc<Mailbox>) -> std::io::Result<Self> {
        let mut drainer = office_ingest::Drain::default();
        pass(&mut drainer, folder, &mailbox)?;
        let folder = folder.to_owned();
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let worker_stop = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("org-office-spool".into())
            .spawn(move || {
                let (lock, wake) = &*worker_stop;
                loop {
                    let stopped = lock.lock().unwrap_or_else(|p| p.into_inner());
                    let (stopped, _) = wake
                        .wait_timeout_while(stopped, POLL, |stop| !*stop)
                        .unwrap_or_else(|p| p.into_inner());
                    if *stopped {
                        break;
                    }
                    drop(stopped);
                    if pass(&mut drainer, &folder, &mailbox).is_err() {
                        eprintln!("org-office-server: spool scan failed; retrying");
                    }
                }
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Ingest {
    fn drop(&mut self) {
        let (lock, wake) = &*self.stop;
        *lock.lock().unwrap_or_else(|p| p.into_inner()) = true;
        wake.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use org_mailbox::testkit::fixture::*;
    use org_mailbox::testkit::{InMemoryDirectory, InMemoryStore};
    use org_mailbox::{MessageStore, NoMessageEvents, OfficeIdentity, SystemClock};
    use org_proto::msg::{MessagePriority, SPOOL_VERSION};
    use org_proto::{Member, MemberKind, RunMode};
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Instant;
    use tempfile::TempDir;

    fn setup(members: Vec<Member>) -> (TempDir, Arc<Mailbox>, InMemoryStore) {
        let dir = TempDir::new().unwrap();
        let store = InMemoryStore::new();
        let mailbox = Arc::new(Mailbox::new(
            Arc::new(store.clone()),
            Arc::new(InMemoryDirectory::new(members, POSITION)),
            OfficeIdentity {
                office_id: OFFICE_ID.into(),
                incarnation: INCARNATION.into(),
                is_hq: true,
            },
            Arc::new(SystemClock),
            Arc::new(NoMessageEvents),
        ));
        (dir, mailbox, store)
    }

    fn send() -> SpooledSend {
        SpooledSend {
            v: SPOOL_VERSION,
            office_id: OFFICE_ID.into(),
            from: ADA_HANDLE.into(),
            id: msg_id(1),
            to: REMY_HANDLE.into(),
            kind: "msg".into(),
            priority: MessagePriority::Normal,
            subject: None,
            body: "PRIVATE_BODY".into(),
            meta: BTreeMap::from([("private".into(), serde_json::json!("PRIVATE_META"))]),
            reply_to: None,
            created_at: "2026-01-01T00:00:00.000Z".into(),
        }
    }

    fn place(folder: &Path, send: &SpooledSend) -> std::path::PathBuf {
        office_ingest::Drain::default()
            .drain(folder, |_, _| panic!("empty"), |_| {})
            .unwrap();
        let path = folder.join("spool/new").join(format!("{}.json", send.id));
        let staged = folder.join("staged-send");
        fs::write(&staged, serde_json::to_vec(send).unwrap()).unwrap();
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o600)).unwrap();
        fs::rename(staged, &path).unwrap();
        path
    }

    #[test]
    fn busy_startup_lock_defers_ingest_until_a_later_pass() {
        let (dir, mailbox, store) = setup(members());
        let send = send();
        let path = place(dir.path(), &send);
        let lock = fs::File::open(dir.path().join("spool/.meta.lock")).unwrap();
        lock.lock().unwrap();
        let ingest = Ingest::start(dir.path(), mailbox.clone()).unwrap();
        assert!(path.exists());
        assert!(store.get_message(&send.id).unwrap().is_none());
        lock.unlock().unwrap();
        pass(&mut office_ingest::Drain::default(), dir.path(), &mailbox).unwrap();
        assert!(!path.exists());
        drop(ingest);
    }

    #[test]
    fn startup_runs_one_budget_and_poll_resumes_the_backlog() {
        let (dir, mailbox, store) = setup(members());
        office_ingest::Drain::default()
            .drain(dir.path(), |_, _| panic!("empty"), |_| {})
            .unwrap();
        let mut ids = Vec::new();
        for _ in 0..140 {
            let mut send = send();
            send.id = org_spool::generate_ulid();
            let path = dir
                .path()
                .join("spool/new")
                .join(format!("{}.json", send.id));
            fs::write(&path, serde_json::to_vec(&send).unwrap()).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
            ids.push(send.id);
        }
        let ingest = Ingest::start(dir.path(), mailbox).unwrap();
        assert_eq!(
            ids.iter()
                .filter(|id| store.get_message(id).unwrap().is_some())
                .count(),
            128
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while fs::read_dir(dir.path().join("spool/new")).unwrap().count() != 0 {
            assert!(Instant::now() < deadline, "poll did not resume backlog");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ids
            .iter()
            .all(|id| store.get_message(id).unwrap().is_some()));
        drop(ingest);
    }

    #[test]
    fn startup_and_running_ingest_and_shutdown() {
        let (dir, mailbox, store) = setup(members());
        let first = send();
        let path = place(dir.path(), &first);
        let ingest = Ingest::start(dir.path(), mailbox).unwrap();
        assert!(
            !path.exists(),
            "startup pass must finish before start returns"
        );
        assert_eq!(
            store.get_message(&first.id).unwrap().unwrap().message.body,
            first.body
        );
        let mut second = send();
        second.id = msg_id(2);
        let path = place(dir.path(), &second);
        let started = Instant::now();
        while path.exists() && started.elapsed() < POLL + Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!path.exists(), "running poll did not ingest");
        assert!(store.get_message(&second.id).unwrap().is_some());
        let stopped = Instant::now();
        drop(ingest);
        assert!(stopped.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn every_sender_refusal_is_quarantined() {
        let mut roster = members();
        for (i, mode) in [
            RunMode::AlwaysOn,
            RunMode::OnCall,
            RunMode::Scheduled,
            RunMode::Unknown,
        ]
        .into_iter()
        .enumerate()
        {
            let mut agent = roster.iter().find(|m| m.id == ADA_ID).unwrap().clone();
            agent.id = format!("RUNAGENT{i}");
            agent.agent.as_mut().unwrap().messaging_name = format!("runagent{i}");
            agent.agent.as_mut().unwrap().run_mode = Some(mode);
            roster.push(agent);
        }
        let mut unknown = roster.iter().find(|m| m.id == ADA_ID).unwrap().clone();
        unknown.id = "UNKNOWN_KIND".into();
        unknown.kind = MemberKind::Unknown;
        roster.push(unknown);
        let mut owner_agent = roster.iter().find(|m| m.id == ADA_ID).unwrap().clone();
        owner_agent.id = "OWNER_AGENT".into();
        owner_agent.owner = true;
        roster.push(owner_agent);
        let mut no_profile = roster.iter().find(|m| m.id == ADA_ID).unwrap().clone();
        no_profile.id = "NO_PROFILE".into();
        no_profile.agent = None;
        roster.push(no_profile);
        let mut run = roster.iter().find(|m| m.id == ADA_ID).unwrap().clone();
        run.id = "run:office/run".into();
        roster.push(run);
        let (dir, mailbox, store) = setup(roster);
        for from in [
            PAT_HANDLE,
            PAT_ID,
            "owner",
            OWNER_ID,
            "office",
            "OFFICE",
            "run:office/run",
            "remy",
            "gone",
            "odd",
            "unknown",
            "UNKNOWN_KIND",
            "runagent0",
            "runagent1",
            "runagent2",
            "runagent3",
            "OWNER_AGENT",
            "NO_PROFILE",
        ] {
            let mut send = send();
            send.from = from.into();
            place(dir.path(), &send);
            pass(&mut office_ingest::Drain::default(), dir.path(), &mailbox).unwrap();
            assert_eq!(
                fs::read_dir(dir.path().join("spool/new")).unwrap().count(),
                0,
                "{from}"
            );
            assert!(store.get_message(&send.id).unwrap().is_none(), "{from}");
        }
        assert_eq!(
            fs::read_dir(dir.path().join("spool/dead")).unwrap().count(),
            18
        );
    }

    #[test]
    fn invalid_envelopes_and_cross_office_routing_are_refused() {
        let (dir, mailbox, store) = setup(members());
        let base = serde_json::to_value(send()).unwrap();
        let mut rejected = 0;
        for (field, changed) in [
            ("v", serde_json::json!(99)),
            ("office_id", serde_json::json!("other")),
            ("id", serde_json::json!(msg_id(2))),
            ("priority", serde_json::json!("future")),
            ("kind", serde_json::json!("request")),
            ("kind", serde_json::json!("delivery_failed")),
            ("to", serde_json::json!("office")),
            ("to", serde_json::json!("missing")),
            ("reply_to", serde_json::json!(msg_id(999))),
            ("created_at", serde_json::json!("bad")),
            (
                "body",
                serde_json::json!("x".repeat(org_proto::msg::MAX_BODY_BYTES + 1)),
            ),
        ] {
            let path = place(dir.path(), &send());
            let mut value = base.clone();
            value[field] = changed;
            fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            pass(&mut office_ingest::Drain::default(), dir.path(), &mailbox).unwrap();
            assert!(!path.exists(), "{field}");
            assert!(store.get_message(&send().id).unwrap().is_none());
            assert!(store.get_message(&msg_id(2)).unwrap().is_none());
            rejected += 1;
            assert_eq!(
                fs::read_dir(dir.path().join("spool/dead")).unwrap().count(),
                rejected,
                "{field}"
            );
        }
        let path = place(dir.path(), &send());
        fs::write(&path, b"{malformed PRIVATE_BODY").unwrap();
        pass(&mut office_ingest::Drain::default(), dir.path(), &mailbox).unwrap();
        assert!(!path.exists());
        let mut far = send();
        far.to = FAR_HANDLE.into();
        let path = place(dir.path(), &far);
        pass(&mut office_ingest::Drain::default(), dir.path(), &mailbox).unwrap();
        assert!(!path.exists(), "cross-office routing must be refused");
        assert!(store.get_message(&far.id).unwrap().is_none());
        assert_eq!(
            fs::read_dir(dir.path().join("spool/dead")).unwrap().count(),
            rejected + 2
        );
    }
}
