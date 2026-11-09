//! The office's side of the messaging ports (`org_mailbox`): [`OfficeMessageStore`] implements
//! `MessageStore` over `office.db`'s message tables, and [`OfficeDirectory`] implements
//! `MemberDirectory` over `org.db`. They are separate databases: no transaction spans both.
//! The SQL is in `store::messages`; this file only maps its results to the ports' types.

use std::sync::Arc;

use org_mailbox::{
    CommitMessageOutcome, HeaderQuery, Mailbox, MemberDirectory, MessageEvents, MessageStore,
    NewStoredMessage, OfficeIdentity, PeekUnreadOutcome, StoreError, StoredMessage, SystemClock,
};
use org_proto::msg::HeaderPage;
use org_proto::{Member, Position};

use crate::directory::resolve_member;
use crate::error::OfficeError;
use crate::model::OfficeRole;
use crate::office::Office;
use crate::store::{messages, org as org_db};

/// An error the ports report: opaque, its cause only for the server's log (an `OfficeError`'s
/// text never holds a path, SQL or a secret).
fn failure(error: OfficeError) -> StoreError {
    StoreError::new(error)
}

/// `MessageStore` over this office's `office.db`.
#[derive(Clone)]
pub struct OfficeMessageStore(Arc<Office>);

/// `MemberDirectory` over this office's `org.db`, at the position it has applied.
#[derive(Clone)]
pub struct OfficeDirectory(Arc<Office>);

impl Office {
    /// The messages of this office, as the mailbox's storage port.
    pub fn message_store(self: &Arc<Self>) -> OfficeMessageStore {
        OfficeMessageStore(Arc::clone(self))
    }

    /// The office's mailbox: its message store and member directory under the messaging rules,
    /// with `events` told of what is sent and read. Reads (or makes) the incarnation, so it can
    /// fail on storage.
    pub fn mailbox(
        self: &Arc<Self>,
        events: Arc<dyn MessageEvents>,
    ) -> Result<Mailbox, OfficeError> {
        let incarnation = self
            .local_store()
            .write(|db| Ok((messages::incarnation(db)?, None)))?;
        let identity = OfficeIdentity {
            office_id: self.info().office_id.clone(),
            incarnation,
            is_hq: matches!(self.info().role, OfficeRole::Hq),
        };
        Ok(Mailbox::new(
            Arc::new(self.message_store()),
            Arc::new(self.member_directory()),
            identity,
            Arc::new(SystemClock),
            events,
        ))
    }

    /// The members of this office's organization, as the mailbox's directory port.
    pub fn member_directory(self: &Arc<Self>) -> OfficeDirectory {
        OfficeDirectory(Arc::clone(self))
    }
}

impl MessageStore for OfficeMessageStore {
    fn commit_message(&self, msg: NewStoredMessage) -> Result<CommitMessageOutcome, StoreError> {
        messages::write(self.0.local_store(), move |db| {
            messages::commit_message(db, msg.clone())
        })
        .map_err(failure)?
    }

    fn claim_read(
        &self,
        to_member_id: &str,
        id: Option<&str>,
        now_utc: &str,
    ) -> Result<Option<StoredMessage>, StoreError> {
        let (member, id, now) = (
            to_member_id.to_owned(),
            id.map(str::to_owned),
            now_utc.to_owned(),
        );
        messages::write(self.0.local_store(), move |db| {
            messages::claim_read(db, &member, id.as_deref(), &now)
        })
        .map_err(failure)?
    }

    fn peek_next_unread(
        &self,
        to_member_id: &str,
        from_member_id: Option<&str>,
        urgent_only: bool,
        now_utc: &str,
    ) -> Result<PeekUnreadOutcome, StoreError> {
        self.0
            .local_store()
            .read(|db| {
                messages::peek_next_unread(db, to_member_id, from_member_id, urgent_only, now_utc)
            })
            .map_err(failure)
    }

    fn get_message(&self, id: &str) -> Result<Option<StoredMessage>, StoreError> {
        self.0
            .local_store()
            .read(|db| messages::get_message(db, id))
            .map_err(failure)
    }

    fn list_headers(&self, query: HeaderQuery) -> Result<HeaderPage, StoreError> {
        self.0
            .local_store()
            .read(|db| messages::list_headers(db, &query))
            .map_err(failure)
    }

    fn find_reply_for_ask(
        &self,
        ask_id: &str,
        responder_id: &str,
    ) -> Result<Option<StoredMessage>, StoreError> {
        self.0
            .local_store()
            .read(|db| messages::find_reply_for_ask(db, ask_id, responder_id))
            .map_err(failure)
    }
}

impl MemberDirectory for OfficeDirectory {
    fn resolve_member(&self, id_or_handle: &str) -> Result<Option<(Member, Position)>, StoreError> {
        let snapshot = self.0.org_store().read(org_db::snapshot).map_err(failure)?;
        // Two members with one handle is a storage error: the Mailbox then refuses, closed.
        match resolve_member(&snapshot, id_or_handle) {
            Ok(found) => Ok(found.map(|member| (member.clone(), snapshot.position))),
            Err(_) => Err(StoreError::new("a handle names two members")),
        }
    }
}

#[cfg(test)]
mod tests {
    //! The store conformance suite from `org_mailbox` run against the SQL backend, and the checks
    //! of what is SQL's own: the tables, a restart, an upgrade, the thread summaries.

    use std::sync::Arc;

    use org_mailbox::testkit::fixture::{members, OFFICE_ID, POSITION};
    use org_proto::msg::{
        Message, MessageHeader, MessageLifecycleState as State, MessagePriority, Meta, KIND_MSG,
    };
    use org_proto::{Member, Position};
    use tempfile::TempDir;

    use super::*;
    use crate::key::Key;
    use crate::model::{NewOffice, OfficeRole};

    fn new_office() -> NewOffice {
        NewOffice {
            office_id: OFFICE_ID.into(),
            office_name: "HQ".into(),
            role: OfficeRole::Hq,
            org_id: "ORGSUITE".into(),
            org_name: "Suite".into(),
        }
    }

    /// The key the host would send (the office never makes one).
    fn test_key() -> Key {
        Key::from_bytes([5; 32])
    }

    fn office_in(dir: &TempDir) -> Arc<Office> {
        let folder = dir.path().join("offices").join(OFFICE_ID);
        let office = Office::create(&folder, &new_office(), test_key()).expect("an office");
        office.seed_organization_for_test(&members(), POSITION);
        Arc::new(office)
    }

    // ---- the conformance suite ---------------------------------------------------------------

    /// The store half of a backend: it keeps its office's folder alive for as long as it lives.
    #[derive(Clone)]
    struct Sql {
        store: OfficeMessageStore,
        _folder: Arc<TempDir>,
    }

    #[derive(Clone)]
    struct SqlDirectory {
        directory: OfficeDirectory,
        _folder: Arc<TempDir>,
    }

    impl MessageStore for Sql {
        fn commit_message(
            &self,
            msg: NewStoredMessage,
        ) -> Result<CommitMessageOutcome, StoreError> {
            self.store.commit_message(msg)
        }
        fn claim_read(
            &self,
            to_member_id: &str,
            id: Option<&str>,
            now_utc: &str,
        ) -> Result<Option<StoredMessage>, StoreError> {
            self.store.claim_read(to_member_id, id, now_utc)
        }
        fn peek_next_unread(
            &self,
            to_member_id: &str,
            from_member_id: Option<&str>,
            urgent_only: bool,
            now_utc: &str,
        ) -> Result<PeekUnreadOutcome, StoreError> {
            self.store
                .peek_next_unread(to_member_id, from_member_id, urgent_only, now_utc)
        }
        fn get_message(&self, id: &str) -> Result<Option<StoredMessage>, StoreError> {
            self.store.get_message(id)
        }
        fn list_headers(&self, query: HeaderQuery) -> Result<HeaderPage, StoreError> {
            self.store.list_headers(query)
        }
        fn find_reply_for_ask(
            &self,
            ask_id: &str,
            responder_id: &str,
        ) -> Result<Option<StoredMessage>, StoreError> {
            self.store.find_reply_for_ask(ask_id, responder_id)
        }
    }

    impl MemberDirectory for SqlDirectory {
        fn resolve_member(
            &self,
            id_or_handle: &str,
        ) -> Result<Option<(Member, Position)>, StoreError> {
            self.directory.resolve_member(id_or_handle)
        }
    }

    /// A fresh office on disk with `members` at `position`, as the (store, directory) pair.
    fn open_sql(members: &[Member], position: Position) -> (Sql, SqlDirectory) {
        let dir = Arc::new(TempDir::new().expect("a temporary folder"));
        let office = office_in(&dir);
        office.seed_organization_for_test(members, position);
        (
            Sql {
                store: office.message_store(),
                _folder: Arc::clone(&dir),
            },
            SqlDirectory {
                directory: office.member_directory(),
                _folder: dir,
            },
        )
    }

    org_mailbox::mailbox_conformance_tests!(open_sql);

    // ---- what is SQL's own ---------------------------------------------------------------------

    const ADA: &str = "01JCMADA000000000000000000";
    const PAT: &str = "01JCMPAT000000000000000000";

    fn message(
        n: u32,
        from: &str,
        to: &str,
        reply_to: Option<&str>,
        thread: &str,
    ) -> NewStoredMessage {
        let id = format!("01JCMSG{n:019}");
        let body = format!("héllo {n} ✓");
        let mut meta = Meta::new();
        meta.insert(
            "k".into(),
            serde_json::json!({"nested": [1, 2, {"x": null}]}),
        );
        NewStoredMessage {
            message: Message {
                header: MessageHeader {
                    id: id.clone(),
                    thread_id: if thread.is_empty() { id } else { thread.into() },
                    reply_to: reply_to.map(Into::into),
                    from_member_id: from.into(),
                    from_display_name: "Sender".into(),
                    to_member_id: to.into(),
                    to_display_name: "Recipient".into(),
                    kind: KIND_MSG.into(),
                    priority: MessagePriority::Normal,
                    urgent: false,
                    subject: Some(format!("subject {n}")),
                    created_at: format!("2026-11-08T12:00:{n:02}.000Z"),
                    expires_at: None,
                    ttl_ms: None,
                    body_bytes: body.len() as u64 + 2,
                    meta_bytes: 20,
                    local_seq: 0,
                    state: State::Unread,
                    triage_gen: 0,
                    snoozed_until: None,
                    snooze_after_task: false,
                    snooze_note: None,
                    read_at: None,
                    expired_waiting_person: false,
                },
                body,
                meta,
            },
            origin_office_id: OFFICE_ID.into(),
            origin_incarnation: "01JCINCARNATION00000000000".into(),
            origin_seq: None,
            org_position: POSITION,
            content_hash: [n as u8; 32],
            initial_state: State::Unread,
        }
    }

    fn count(office: &Office, sql: &'static str) -> i64 {
        office
            .local_store()
            .read(|db| {
                db.query_row(sql, (), |row| row.get(0))
                    .map_err(OfficeError::storage)
            })
            .unwrap()
    }

    #[test]
    fn a_message_survives_a_restart_with_its_body_meta_and_sequence_and_the_counter_goes_on() {
        let dir = TempDir::new().unwrap();
        let office = office_in(&dir);
        let store = office.message_store();
        let first = store
            .commit_message(message(1, ADA, PAT, None, ""))
            .unwrap();
        assert!(first.inserted);
        assert_eq!(first.message.message.header.local_seq, 1);
        drop(store);
        let folder = office.folder().to_owned();
        drop(office);

        let again = Arc::new(Office::open(&folder, test_key()).unwrap());
        let store = again.message_store();
        let read = store
            .get_message(&first.message.message.header.id)
            .unwrap()
            .unwrap();
        assert_eq!(read, first.message, "a message changed across a restart");
        assert_eq!(read.message.body, "héllo 1 ✓");
        assert_eq!(
            read.message.meta["k"],
            serde_json::json!({"nested": [1, 2, {"x": null}]})
        );
        // The sequence is persisted: the next message follows, it does not start over.
        let second = store
            .commit_message(message(2, ADA, PAT, None, ""))
            .unwrap();
        assert_eq!(second.message.message.header.local_seq, 2);
        assert_eq!(second.message.origin_seq, 2);
    }

    #[test]
    fn an_office_db_made_before_messaging_gets_its_tables_when_it_is_opened() {
        use crate::store::{Opening, Store};
        let dir = TempDir::new().unwrap();
        let office = office_in(&dir);
        let folder = office.folder().to_owned();
        drop(office);
        // Put office.db back as version 1 made it: identity only, no message tables.
        let key = test_key();
        std::fs::remove_file(folder.join("office.db")).unwrap();
        for sidecar in ["office.db-wal", "office.db-shm"] {
            let _ = std::fs::remove_file(folder.join(sidecar));
        }
        let old = crate::store::Schema {
            version: 1,
            create: |tx| {
                tx.execute_batch(
                    "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;",
                )
            },
            upgrade: |_, _| Ok(()),
        };
        let store = Store::open(&folder.join("office.db"), &key, Opening::New, &old).unwrap();
        store
            .write(|db| {
                db.execute(
                    "INSERT INTO meta (key, value) VALUES ('office_id', ?1)",
                    [OFFICE_ID],
                )
                .map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
        drop(store);

        let office = Arc::new(Office::open(&folder, test_key()).unwrap());

        assert_eq!(
            count(
                &office,
                "SELECT COUNT(*) FROM sqlite_master WHERE name LIKE 'msg_%' AND type = 'table'"
            ),
            6
        );
        assert_eq!(count(&office, "PRAGMA user_version"), 3);
        assert_eq!(
            count(
                &office,
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'member_status'"
            ),
            1
        );
        // ...and it works: the identity it had is kept, and a message commits.
        let stored = office
            .message_store()
            .commit_message(message(1, ADA, PAT, None, ""))
            .unwrap();
        assert!(stored.inserted);
    }

    #[test]
    fn a_thread_summary_counts_new_messages_only_and_keeps_the_earliest_and_the_latest() {
        let dir = TempDir::new().unwrap();
        let office = office_in(&dir);
        let store = office.message_store();
        let root = message(5, ADA, PAT, None, "");
        let thread = root.message.header.id.clone();
        store.commit_message(root.clone()).unwrap();
        // A replay of the root does not count again.
        assert!(!store.commit_message(root).unwrap().inserted);
        // A reply, then one that arrives out of order with an EARLIER time.
        store
            .commit_message(message(9, PAT, ADA, Some(&thread), &thread))
            .unwrap();
        store
            .commit_message(message(
                7,
                PAT,
                "01JCMREMY00000000000000000",
                Some(&thread),
                &thread,
            ))
            .unwrap();

        let row = |col: &str| -> String {
            office
                .local_store()
                .read(|db| {
                    db.query_row(
                        &format!(
                            "SELECT CAST({col} AS TEXT) FROM msg_threads WHERE thread_id = ?1"
                        ),
                        [&thread],
                        |row| row.get(0),
                    )
                    .map_err(OfficeError::storage)
                })
                .unwrap()
        };
        assert_eq!(row("message_count"), "3");
        assert_eq!(row("created_at"), "2026-11-08T12:00:05.000Z"); // the earliest
        assert_eq!(row("last_msg_at"), "2026-11-08T12:00:09.000Z"); // the latest
        assert_eq!(row("subject"), "subject 5"); // the root's
        let people: Vec<String> = serde_json::from_str(&row("participants_json")).unwrap();
        assert_eq!(people, [ADA, PAT, "01JCMREMY00000000000000000"]);
        assert_eq!(
            count(&office, "SELECT COUNT(*) FROM msg_thread_participants"),
            3
        );
        assert_eq!(count(&office, "SELECT COUNT(*) FROM msg_threads"), 1);
    }

    #[test]
    fn a_thread_whose_root_arrives_last_takes_the_roots_subject_and_time() {
        let dir = TempDir::new().unwrap();
        let office = office_in(&dir);
        let store = office.message_store();
        let root = message(5, ADA, PAT, None, "");
        let thread = root.message.header.id.clone();
        // A reply is stored first (a satellite may see the middle of a thread before its root).
        store
            .commit_message(message(9, PAT, ADA, Some(&thread), &thread))
            .unwrap();
        let text = |col: &str| -> String {
            office
                .local_store()
                .read(|db| {
                    db.query_row(
                        &format!(
                            "SELECT CAST({col} AS TEXT) FROM msg_threads WHERE thread_id = ?1"
                        ),
                        [&thread],
                        |row| row.get(0),
                    )
                    .map_err(OfficeError::storage)
                })
                .unwrap()
        };
        assert_eq!(text("subject"), "subject 9");
        assert_eq!(text("created_at"), "2026-11-08T12:00:09.000Z");

        store.commit_message(root).unwrap();

        assert_eq!(text("subject"), "subject 5", "the root's subject wins");
        assert_eq!(
            text("created_at"),
            "2026-11-08T12:00:05.000Z",
            "the earliest time wins"
        );
        assert_eq!(
            text("last_msg_at"),
            "2026-11-08T12:00:09.000Z",
            "the latest stays"
        );
        assert_eq!(text("message_count"), "2");
    }

    #[test]
    fn the_unread_index_holds_exactly_the_open_messages() {
        let dir = TempDir::new().unwrap();
        let office = office_in(&dir);
        let store = office.message_store();
        store
            .commit_message(message(1, ADA, PAT, None, ""))
            .unwrap();
        store
            .commit_message(message(2, ADA, PAT, None, ""))
            .unwrap();
        let mut read_already = message(3, ADA, PAT, None, "");
        read_already.initial_state = State::Read;
        store.commit_message(read_already).unwrap();
        assert_eq!(count(&office, "SELECT COUNT(*) FROM msg_unread"), 2);
        assert_eq!(count(&office, "SELECT COUNT(*) FROM msg_headers"), 3);
        assert_eq!(count(&office, "SELECT COUNT(*) FROM msg_bodies"), 3);
        assert_eq!(count(&office, "SELECT COUNT(*) FROM msg_state"), 3);

        store
            .claim_read(PAT, None, "2026-11-08T12:01:00.000Z")
            .unwrap()
            .unwrap();

        assert_eq!(count(&office, "SELECT COUNT(*) FROM msg_unread"), 1);
        assert_eq!(
            count(
                &office,
                "SELECT COUNT(*) FROM msg_state WHERE state = 'read'"
            ),
            2
        );
    }

    #[test]
    fn an_ask_expires_exactly_at_its_deadline_plus_the_grace_and_not_a_millisecond_before() {
        let dir = TempDir::new().unwrap();
        let office = office_in(&dir);
        let store = office.message_store();
        let mut ask = message(1, ADA, PAT, None, "");
        // Due at 12:00:10.000; the grace is a quarter of the ttl (here 1 s).
        ask.message.header.expires_at = Some("2026-11-08T12:00:10.000Z".into());
        ask.message.header.ttl_ms = Some(4000);
        let id = ask.message.header.id.clone();
        store.commit_message(ask).unwrap();

        let peek = |now: &str| store.peek_next_unread(PAT, None, false, now).unwrap();
        let before = peek("2026-11-08T12:00:10.999Z");
        assert_eq!(
            before.message.map(|m| m.message.header.id),
            Some(id.clone())
        );
        assert!(before.expired_ids.is_empty());
        let at = peek("2026-11-08T12:00:11.000Z");
        assert!(at.message.is_none());
        assert_eq!(at.expired_ids, [id]);
    }

    #[test]
    fn the_office_mailbox_sends_lists_and_reads_over_the_real_stores() {
        use org_mailbox::NoMessageEvents;
        use org_proto::msg::{MsgInboxParams, MsgReadParams, MsgSendParams, KIND_MSG};

        let dir = TempDir::new().unwrap();
        let office = office_in(&dir);
        let mailbox = office.mailbox(Arc::new(NoMessageEvents)).unwrap();
        let id = crate::ulid::generate().unwrap();
        let sent = mailbox
            .send(
                ADA,
                MsgSendParams {
                    id: id.clone(),
                    to: PAT.into(),
                    kind: KIND_MSG.into(),
                    priority: None,
                    subject: Some("hello".into()),
                    body: "from ada".into(),
                    meta: Default::default(),
                    reply_to: None,
                },
            )
            .unwrap();
        assert_eq!(sent.id, id);

        let inbox = mailbox
            .inbox(
                PAT,
                MsgInboxParams {
                    from: None,
                    thread_id: None,
                    include_all: false,
                    cursor: None,
                    limit: None,
                },
            )
            .unwrap();
        assert_eq!(inbox.headers.len(), 1);
        let read = mailbox
            .read(
                PAT,
                MsgReadParams {
                    id: Some(id.clone()),
                },
            )
            .unwrap();
        assert_eq!(read.message.unwrap().body, "from ada");
        // Read now: nothing unread is left.
        let unread = mailbox
            .inbox(
                PAT,
                MsgInboxParams {
                    from: None,
                    thread_id: None,
                    include_all: false,
                    cursor: None,
                    limit: None,
                },
            )
            .unwrap();
        assert!(unread.headers.is_empty());
    }

    #[test]
    fn spool_replay_after_commit_before_unlink_is_durable_across_restart() {
        use org_mailbox::NoMessageEvents;
        use org_proto::msg::{SpooledSend, SPOOL_VERSION};
        use org_spool::office_ingest::{Delivery, Drain};
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let office = office_in(&dir);
        let folder = office.folder().to_owned();
        let send = SpooledSend {
            v: SPOOL_VERSION,
            office_id: OFFICE_ID.into(),
            from: "ada".into(),
            id: crate::ulid::generate().unwrap(),
            to: PAT.into(),
            kind: KIND_MSG.into(),
            priority: MessagePriority::Normal,
            subject: None,
            body: "offline".into(),
            meta: Default::default(),
            reply_to: None,
            created_at: "2026-01-01T00:00:00.000Z".into(),
        };
        Drain::default()
            .drain(&folder, |_, _| panic!("empty"), |_| {})
            .unwrap();
        let path = folder.join("spool/new").join(format!("{}.json", send.id));
        std::fs::write(&path, serde_json::to_vec(&send).unwrap()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mailbox = office.mailbox(Arc::new(NoMessageEvents)).unwrap();
        let first = mailbox.ingest_spooled(send.clone()).unwrap();
        // Simulate a crash after the store commits and before the spool file is removed.
        drop(mailbox);
        drop(office);
        let office = Arc::new(Office::open(&folder, test_key()).unwrap());
        let mailbox = office.mailbox(Arc::new(NoMessageEvents)).unwrap();
        Drain::default()
            .drain(
                &folder,
                |id, bytes| {
                    let replay = serde_json::from_slice(bytes).unwrap();
                    assert_eq!(id, send.id);
                    assert_eq!(mailbox.ingest_spooled(replay).unwrap(), first);
                    Delivery::Accepted
                },
                |_| panic!("valid replay refused"),
            )
            .unwrap();
        assert!(!path.exists());
        assert_eq!(count(&office, "SELECT COUNT(*) FROM msg_headers"), 1);
        assert_eq!(
            count(&office, "SELECT SUM(message_count) FROM msg_threads"),
            1
        );
        assert_eq!(count(&office, "SELECT COUNT(*) FROM msg_unread"), 1);
    }

    #[test]
    fn the_incarnation_is_made_once_and_survives_a_reopen() {
        use org_mailbox::NoMessageEvents;
        let dir = TempDir::new().unwrap();
        let office = office_in(&dir);
        let first = office
            .local_store()
            .write(|db| Ok((messages::incarnation(db)?, None)))
            .unwrap();
        let again = office
            .local_store()
            .write(|db| Ok((messages::incarnation(db)?, None)))
            .unwrap();
        assert_eq!(first, again);
        assert_eq!(first.len(), 26);
        office.mailbox(Arc::new(NoMessageEvents)).unwrap();
        let folder = dir.path().join("offices").join(OFFICE_ID);
        let Ok(office) = Arc::try_unwrap(office) else {
            panic!("the only owner")
        };
        drop(office);
        let reopened = Arc::new(Office::open(&folder, test_key()).unwrap());
        let after = reopened
            .local_store()
            .write(|db| Ok((messages::incarnation(db)?, None)))
            .unwrap();
        assert_eq!(first, after);
    }

    #[test]
    fn a_cursor_beyond_the_sequence_range_lists_from_the_newest() {
        let dir = TempDir::new().unwrap();
        let office = office_in(&dir);
        let store = office.message_store();
        store
            .commit_message(message(1, ADA, PAT, None, ""))
            .unwrap();
        let page = store
            .list_headers(HeaderQuery {
                scope: org_mailbox::HeaderScope::All,
                from_member_id: None,
                thread_id: None,
                unread_only: false,
                include_snoozed: false,
                cursor: Some(u64::MAX),
                limit: 10,
            })
            .unwrap();
        assert_eq!(page.headers.len(), 1);
    }

    #[test]
    fn peeking_and_claiming_the_next_message_bring_its_body_not_just_its_header() {
        let dir = TempDir::new().unwrap();
        let office = office_in(&dir);
        let store = office.message_store();
        let first = message(1, ADA, PAT, None, "");
        let (body, meta) = (first.message.body.clone(), first.message.meta.clone());
        assert!(!body.is_empty() && !meta.is_empty());
        store.commit_message(first).unwrap();
        store
            .commit_message(message(2, ADA, PAT, None, ""))
            .unwrap();
        let now = "2026-11-08T12:01:00.000Z";
        let peeked = store.peek_next_unread(PAT, None, false, now).unwrap();
        let peeked = peeked.message.unwrap().message;
        assert_eq!((peeked.body, peeked.meta), (body.clone(), meta.clone()));
        let claimed = store.claim_read(PAT, None, now).unwrap().unwrap();
        assert_eq!((claimed.message.body, claimed.message.meta), (body, meta));
    }

    #[test]
    fn a_garbled_sequence_counter_is_an_error_and_not_a_restart_from_zero() {
        let dir = TempDir::new().unwrap();
        let office = office_in(&dir);
        office
            .local_store()
            .write(|db| {
                db.execute(
                    "INSERT INTO meta (key, value) VALUES ('message_sequence', 'many')",
                    (),
                )
                .map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
        let store = office.message_store();
        assert!(store
            .commit_message(message(1, ADA, PAT, None, ""))
            .is_err());
    }

    #[test]
    fn an_inbox_or_thread_page_is_answered_from_its_index_not_a_walk_of_every_header() {
        use org_mailbox::HeaderScope;
        let dir = TempDir::new().unwrap();
        let office = office_in(&dir);
        let plan = |scope: HeaderScope, by_thread: bool| -> String {
            office
                .local_store()
                .read(|db| {
                    let sql = format!(
                        "EXPLAIN QUERY PLAN {}",
                        messages::headers_sql(&scope, by_thread)
                    );
                    let mut statement = db.prepare(&sql).map_err(OfficeError::storage)?;
                    let rows = statement
                        .query_map(
                            rusqlite::params![PAT, None::<String>, Some("T"), 9, 0, 0, 5],
                            |row| row.get::<_, String>(3),
                        )
                        .map_err(OfficeError::storage)?;
                    Ok(rows.map(Result::unwrap).collect::<Vec<_>>().join("\n"))
                })
                .unwrap()
        };
        assert!(plan(HeaderScope::Inbox(PAT.into()), false).contains("idx_msg_to_seq"));
        assert!(plan(HeaderScope::All, true).contains("idx_msg_thread_seq"));
    }

    #[test]
    fn claiming_the_next_message_skips_an_expired_ask_and_leaves_it_unread() {
        let dir = TempDir::new().unwrap();
        let office = office_in(&dir);
        let store = office.message_store();
        let mut ask = message(1, ADA, PAT, None, "");
        ask.message.header.expires_at = Some("2026-11-08T12:00:10.000Z".into());
        ask.message.header.ttl_ms = Some(4000);
        let expired_id = ask.message.header.id.clone();
        let live = message(2, ADA, PAT, None, "");
        let live_id = live.message.header.id.clone();
        store.commit_message(ask).unwrap();
        store.commit_message(live).unwrap();

        let claimed = store
            .claim_read(PAT, None, "2026-11-08T12:00:20.000Z")
            .unwrap()
            .unwrap();
        assert_eq!(claimed.message.header.id, live_id);
        let left = store.get_message(&expired_id).unwrap().unwrap();
        assert_eq!(left.message.header.state, State::Unread);
    }

    #[test]
    fn a_message_committed_as_read_has_the_time_it_was_made_as_its_read_time() {
        let dir = TempDir::new().unwrap();
        let office = office_in(&dir);
        let store = office.message_store();
        let mut read = message(4, ADA, PAT, None, "");
        read.initial_state = State::Read;
        let committed = store.commit_message(read).unwrap();
        let header = |m: &StoredMessage| m.message.header.clone();
        assert_eq!(header(&committed.message).state, State::Read);
        assert_eq!(
            header(&committed.message).read_at.as_deref(),
            Some("2026-11-08T12:00:04.000Z")
        );
        let stored = store
            .get_message(&header(&committed.message).id)
            .unwrap()
            .unwrap();
        assert_eq!(header(&stored), header(&committed.message));
    }

    #[test]
    fn the_database_allows_one_delivery_failed_notice_per_message_and_one_header_per_origin() {
        let dir = TempDir::new().unwrap();
        let office = office_in(&dir);
        let store = office.message_store();
        let ask = message(1, ADA, PAT, None, "");
        let ask_id = ask.message.header.id.clone();
        store.commit_message(ask).unwrap();
        let notice = |n: u32| {
            let mut notice = message(n, "office", ADA, Some(&ask_id), &ask_id);
            notice.message.header.kind = "delivery_failed".into();
            notice
        };
        store.commit_message(notice(2)).unwrap();
        // A second notice for the same message is refused by the database, and leaves no trace.
        assert!(store.commit_message(notice(3)).is_err());
        assert_eq!(count(&office, "SELECT COUNT(*) FROM msg_headers"), 2);
        assert_eq!(count(&office, "SELECT COUNT(*) FROM msg_bodies"), 2);
        assert_eq!(count(&office, "SELECT COUNT(*) FROM msg_unread"), 2);
        assert_eq!(
            count(
                &office,
                "SELECT COUNT(*) FROM msg_threads WHERE message_count != 2"
            ),
            0
        );
        // The sequence moved on only for what committed: the failed attempt took no number.
        let next = store
            .commit_message(message(4, ADA, PAT, None, ""))
            .unwrap();
        assert_eq!(next.message.message.header.local_seq, 3);
    }

    #[test]
    fn a_message_from_another_office_is_stored_once_per_origin_whatever_its_id() {
        let dir = TempDir::new().unwrap();
        let office = office_in(&dir);
        let store = office.message_store();
        let mut far = message(1, ADA, PAT, None, "");
        far.origin_office_id = "01JCOFFICEFAR0000000000000".into();
        far.origin_seq = Some(77);
        let first = store.commit_message(far.clone()).unwrap();
        assert!(first.inserted);
        assert_eq!(first.message.origin_seq, 77);
        // The same origin position again (another id, as a re-sent stream item could be): the
        // stored message comes back and nothing is added.
        let mut again = message(2, ADA, PAT, None, "");
        again.origin_office_id = far.origin_office_id.clone();
        again.origin_seq = Some(77);
        let replay = store.commit_message(again).unwrap();
        assert!(!replay.inserted);
        assert_eq!(replay.message.message.header.id, far.message.header.id);
        assert_eq!(count(&office, "SELECT COUNT(*) FROM msg_headers"), 1);
        // A zero origin sequence is not a position.
        let mut zero = message(3, ADA, PAT, None, "");
        zero.origin_seq = Some(0);
        assert!(matches!(
            store.commit_message(zero),
            Err(StoreError::InvalidTransition)
        ));
    }
}
