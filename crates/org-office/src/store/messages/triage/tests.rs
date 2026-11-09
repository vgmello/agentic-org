use std::sync::Arc;
use std::time::{Duration, Instant};

use org_mailbox::testkit::fixture::{members, ADA_ID, INCARNATION, OFFICE_ID, PAT_ID, POSITION};
use org_mailbox::{MessageStore, NoMessageEvents, TriageStore};
use org_proto::msg::{MsgSendParams, MsgWaitParams, KIND_MSG};
use tempfile::TempDir;

use super::*;
use crate::key::Key;
use crate::model::{NewOffice, OfficeRole};
use crate::office::Office;

const NOW: &str = "2026-11-09T12:00:00.000Z";
const DUE: &str = "2026-11-09T13:00:00.000Z";

fn key() -> Key {
    Key::from_bytes([5; 32])
}

fn office(dir: &TempDir) -> Arc<Office> {
    let office = Office::create(
        dir.path().join(OFFICE_ID),
        &NewOffice {
            office_id: OFFICE_ID.into(),
            office_name: "HQ".into(),
            role: OfficeRole::Hq,
            org_id: "ORGTEST".into(),
            org_name: "Test".into(),
        },
        key(),
    )
    .unwrap();
    office.seed_organization_for_test(&members(), POSITION);
    Arc::new(office)
}

fn send(office: &Arc<Office>) -> String {
    let id = crate::ulid::generate().unwrap();
    office
        .mailbox(Arc::new(NoMessageEvents))
        .unwrap()
        .send(
            PAT_ID,
            MsgSendParams {
                id: id.clone(),
                to: ADA_ID.into(),
                kind: KIND_MSG.into(),
                priority: None,
                subject: None,
                body: "private payload".into(),
                meta: Default::default(),
                reply_to: None,
            },
        )
        .unwrap();
    id
}

fn change(id: &str, state: State) -> LocalStateTransition {
    LocalStateTransition {
        id: id.into(),
        to_member_id: ADA_ID.into(),
        state,
        snoozed_until: None,
        snooze_after_task: false,
        reason: None,
        now_utc: NOW.into(),
        owner_office_id: OFFICE_ID.into(),
        origin_incarnation: INCARNATION.into(),
        org_position: POSITION,
        expired_waiting_person: false,
    }
}

fn snooze(id: &str) -> LocalStateTransition {
    LocalStateTransition {
        snoozed_until: Some(DUE.into()),
        reason: Some("later".into()),
        ..change(id, State::Snoozed)
    }
}

fn number(office: &Office, sql: &str) -> i64 {
    office
        .local_store()
        .read(|db| db.query_row(sql, [], |row| row.get(0)).map_err(storage))
        .unwrap()
}

fn execute(office: &Office, sql: &'static str) {
    office
        .local_store()
        .write(move |db| {
            db.execute_batch(sql).map_err(storage)?;
            Ok(((), None))
        })
        .unwrap();
}

fn schema(office: &Office) -> Vec<(String, String)> {
    office
        .local_store()
        .read(|db| {
            let mut statement = db
                .prepare("SELECT name, sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name")
                .map_err(storage)?;
            let rows = statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(storage)?;
            rows.collect::<rusqlite::Result<_>>().map_err(storage)
        })
        .unwrap()
}

#[test]
fn schema_three_upgrades_additively_and_replaying_the_upgrade_is_harmless() {
    let dir = TempDir::new().unwrap();
    let office = office(&dir);
    let id = send(&office);
    let before = office.message_store().get_message(&id).unwrap().unwrap();
    // Exactly schema 3: retain messages, counters, identity and status, but no new column.
    execute(
        &office,
        "ALTER TABLE msg_state DROP COLUMN triaged_at; PRAGMA user_version = 3;",
    );
    let folder = office.folder().to_owned();
    drop(office);
    let reopened = Arc::new(Office::open(&folder, key()).unwrap());
    assert_eq!(number(&reopened, "PRAGMA user_version"), 4);
    assert_eq!(
        reopened.message_store().get_message(&id).unwrap().unwrap(),
        before
    );
    assert_eq!(
        number(
            &reopened,
            "SELECT COUNT(*) FROM msg_state WHERE triaged_at IS NULL"
        ),
        1
    );
    // New creation and an upgrade produce the same tables, columns and indexes.
    let fresh = TempDir::new().unwrap();
    assert_eq!(schema(&reopened), schema(&self::office(&fresh)));
    let store = reopened.message_store();
    store.update_state_local(snooze(&id)).unwrap();
    let snoozed = store.get_message(&id).unwrap().unwrap();
    drop(store);
    // Replay from 3 even though the column is present, and do it again on next open.
    execute(&reopened, "PRAGMA user_version = 3;");
    drop(reopened);
    let reopened = Arc::new(Office::open(&folder, key()).unwrap());
    assert_eq!(
        reopened.message_store().get_message(&id).unwrap().unwrap(),
        snoozed
    );
    assert_eq!(
        number(
            &reopened,
            "SELECT COUNT(*) FROM msg_state WHERE triaged_at IS NOT NULL"
        ),
        1
    );
    drop(reopened);
    assert_eq!(
        number(&Office::open(folder, key()).unwrap(), "PRAGMA user_version"),
        4
    );
}

#[test]
fn triage_and_the_hot_index_survive_reads_requeues_and_a_restart() {
    let dir = TempDir::new().unwrap();
    let office = office(&dir);
    let id = send(&office);
    let store = office.message_store();
    store.claim_read(ADA_ID, Some(&id), NOW).unwrap();
    let snoozed = store.update_state_local(snooze(&id)).unwrap();
    assert_eq!(
        snoozed.read_at, None,
        "snoozing a read message clears read_at"
    );
    assert_eq!(number(&office, "SELECT COUNT(*) FROM msg_unread WHERE snoozed_until IS NOT NULL AND triage_gen = 1 AND triaged_at IS NOT NULL"), 1);
    store.claim_read(ADA_ID, Some(&id), NOW).unwrap();
    assert_eq!(number(&office, "SELECT COUNT(*) FROM msg_unread"), 0);
    store
        .update_state_local(change(&id, State::Unread))
        .unwrap();
    assert_eq!(number(&office, "SELECT COUNT(*) FROM msg_unread WHERE triaged_at IS NOT NULL AND snoozed_until IS NULL AND snooze_after_task = 0"), 1);
    let before = store.get_message(&id).unwrap().unwrap();
    let folder = office.folder().to_owned();
    drop(store);
    drop(office);
    let office = Arc::new(Office::open(folder, key()).unwrap());
    let store = office.message_store();
    assert_eq!(store.get_message(&id).unwrap().unwrap(), before);
    assert!(
        store
            .check_notify(ADA_ID, false, false, "2099-01-01T00:00:00.000Z", DUE)
            .unwrap()
            .is_empty(),
        "reading and requeueing must not make an old snooze sweepable"
    );
    let done = LocalStateTransition {
        reason: Some("handled".into()),
        ..change(&id, State::Done)
    };
    let header = store.update_state_local(done).unwrap();
    assert_eq!(header.snooze_note, None);
    assert_eq!(number(&office, "SELECT COUNT(*) FROM msg_unread"), 0);
    assert_eq!(
        number(
            &office,
            "SELECT COUNT(*) FROM msg_state WHERE state = 'done' AND reason = 'handled'"
        ),
        1
    );
    drop(store);
    let folder = office.folder().to_owned();
    drop(office);
    assert_eq!(
        Arc::new(Office::open(folder, key()).unwrap())
            .message_store()
            .get_message(&id)
            .unwrap()
            .unwrap()
            .message
            .header,
        header
    );
}

#[test]
fn failed_hot_index_write_rolls_back_state_flags_and_sequence() {
    let dir = TempDir::new().unwrap();
    let office = office(&dir);
    let id = send(&office);
    let store = office.message_store();
    let before = store.get_message(&id).unwrap().unwrap();
    let sequence = number(
        &office,
        "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'message_sequence'",
    );
    execute(&office, "CREATE TRIGGER refuse_hot BEFORE INSERT ON msg_unread BEGIN SELECT RAISE(ABORT, 'test failure'); END;");
    assert!(store.update_state_local(snooze(&id)).is_err());
    let flag = LocalStateTransition {
        expired_waiting_person: true,
        ..change(&id, State::Unread)
    };
    assert!(store.update_state_local(flag).is_err());
    assert_eq!(store.get_message(&id).unwrap().unwrap(), before);
    assert_eq!(number(&office, "SELECT COUNT(*) FROM msg_unread"), 1);
    assert_eq!(
        number(
            &office,
            "SELECT COUNT(*) FROM msg_state WHERE triaged_at IS NOT NULL OR origin_seq != 0"
        ),
        0
    );
    assert_eq!(
        number(
            &office,
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'message_sequence'"
        ),
        sequence
    );
    execute(&office, "DROP TRIGGER refuse_hot;");
    assert_eq!(store.update_state_local(snooze(&id)).unwrap().triage_gen, 1);
}

#[test]
fn notify_rolls_back_every_release_if_a_later_release_fails() {
    let dir = TempDir::new().unwrap();
    let office = office(&dir);
    let store = office.message_store();
    for _ in 0..2 {
        store.update_state_local(snooze(&send(&office))).unwrap();
    }
    let sequence = number(
        &office,
        "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'message_sequence'",
    );
    // The second release fails regardless of iteration order.
    execute(&office, "CREATE TRIGGER refuse_second BEFORE UPDATE ON msg_state WHEN NEW.state = 'unread' AND (SELECT COUNT(*) FROM msg_state WHERE state = 'unread') > 0 BEGIN SELECT RAISE(ABORT, 'test failure'); END;");
    assert!(store.check_notify(ADA_ID, false, false, NOW, DUE).is_err());
    assert_eq!(
        number(
            &office,
            "SELECT COUNT(*) FROM msg_state WHERE state = 'snoozed' AND triage_gen = 1"
        ),
        2
    );
    assert_eq!(
        number(
            &office,
            "SELECT COUNT(*) FROM msg_unread WHERE snoozed_until IS NOT NULL AND triage_gen = 1"
        ),
        2
    );
    assert_eq!(
        number(
            &office,
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'message_sequence'"
        ),
        sequence
    );
    execute(&office, "DROP TRIGGER refuse_second;");
    let released = store.check_notify(ADA_ID, false, false, NOW, DUE).unwrap();
    assert_eq!(released.len(), 2);
    assert!(released
        .iter()
        .all(|c| c.reason == NotifyReason::Due && c.header.triage_gen == 2));
    assert_eq!(
        number(
            &office,
            "SELECT COUNT(*) FROM msg_unread WHERE snoozed_until IS NULL AND triage_gen = 2"
        ),
        2
    );
}

#[test]
fn active_timers_and_after_task_snoozes_survive_reopen_and_seed_the_office_mailbox() {
    let dir = TempDir::new().unwrap();
    let office = office(&dir);
    let id = send(&office);
    let after_task = send(&office);
    let until = org_mailbox::format_utc(crate::time::now_ms() + 500).unwrap();
    let store = office.message_store();
    store
        .update_state_local(LocalStateTransition {
            snoozed_until: Some(until.clone()),
            ..snooze(&id)
        })
        .unwrap();
    store
        .update_state_local(LocalStateTransition {
            snooze_after_task: true,
            ..change(&after_task, State::Snoozed)
        })
        .unwrap();
    let folder = office.folder().to_owned();
    drop(store);
    drop(office);
    let office = Arc::new(Office::open(folder, key()).unwrap());
    assert_eq!(
        office
            .message_store()
            .list_active_snooze_deadlines()
            .unwrap(),
        [SnoozeDeadline {
            id: id.clone(),
            to_member_id: ADA_ID.into(),
            at_utc: until
        }]
    );
    let mailbox = office.mailbox(Arc::new(NoMessageEvents)).unwrap();
    let (_handle, cancel) = org_mailbox::cancellation_pair();
    let result = mailbox
        .wait(
            ADA_ID,
            MsgWaitParams {
                timeout_ms: 3000,
                from: None,
                urgent_only: false,
            },
            Instant::now() + Duration::from_secs(5),
            &cancel,
        )
        .unwrap();
    assert_eq!(result.message.unwrap().header.id, id);
    assert_eq!(
        office
            .message_store()
            .get_message(&after_task)
            .unwrap()
            .unwrap()
            .message
            .header
            .state,
        State::Snoozed
    );
    assert!(office
        .message_store()
        .list_active_snooze_deadlines()
        .unwrap()
        .is_empty());
}

#[test]
fn transitions_and_notify_never_read_the_cold_body_table() {
    let dir = TempDir::new().unwrap();
    let office = office(&dir);
    let id = send(&office);
    // A pruned body does not prevent triage; delivery APIs need the body separately.
    execute(&office, "DELETE FROM msg_bodies;");
    let store = office.message_store();
    assert_eq!(
        store.update_state_local(snooze(&id)).unwrap().state,
        State::Snoozed
    );
    assert_eq!(
        store
            .check_notify(ADA_ID, false, false, NOW, DUE)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .update_state_local(change(&id, State::Done))
            .unwrap()
            .state,
        State::Done
    );
}

#[test]
fn triage_is_invisible_until_commit_and_local_edits_stamp_the_state_sequence() {
    let dir = TempDir::new().unwrap();
    let office = office(&dir);
    let id = send(&office);
    let initial = number(
        &office,
        "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'message_sequence'",
    );
    let (entered, reached) = std::sync::mpsc::sync_channel(1);
    let (release, gate) = std::sync::mpsc::sync_channel(1);
    let writer = {
        let office = Arc::clone(&office);
        let transition = snooze(&id);
        std::thread::spawn(move || {
            super::super::write(office.local_store(), move |db| {
                let header = update_state_local(db, &transition)?;
                entered.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok(header)
            })
        })
    };
    reached.recv_timeout(Duration::from_secs(10)).unwrap();
    // Another pooled connection sees the old state/index/counter while the writer waits.
    assert_eq!(
        office
            .message_store()
            .get_message(&id)
            .unwrap()
            .unwrap()
            .message
            .header
            .state,
        State::Unread
    );
    assert_eq!(
        number(
            &office,
            "SELECT COUNT(*) FROM msg_unread WHERE snoozed_until IS NULL"
        ),
        1
    );
    assert_eq!(
        number(
            &office,
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'message_sequence'"
        ),
        initial
    );
    release.send(()).unwrap();
    let header = writer.join().unwrap().unwrap().unwrap();
    assert_eq!(header.state, State::Snoozed);
    assert_eq!(
        office
            .message_store()
            .get_message(&id)
            .unwrap()
            .unwrap()
            .message
            .header,
        header
    );
    assert_eq!(number(&office, "SELECT COUNT(*) FROM msg_state WHERE origin_seq = 2 AND org_epoch = 1 AND org_seq = 42 AND owner_office_id = '01JCOFFICEHQ00000000000000' AND origin_incarnation = '01JCINCARNATION00000000000'"), 1);
    office
        .message_store()
        .check_notify(ADA_ID, false, false, NOW, DUE)
        .unwrap();
    assert_eq!(number(&office, "SELECT COUNT(*) FROM msg_state WHERE origin_seq = 3 AND org_epoch = 1 AND org_seq = 42 AND owner_office_id = '01JCOFFICEHQ00000000000000' AND origin_incarnation = '01JCINCARNATION00000000000'"), 1);
    let store = office.message_store();
    let done = store.update_state_local(change(&id, State::Done)).unwrap();
    let sequence = number(
        &office,
        "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'message_sequence'",
    );
    assert_eq!(
        store.update_state_local(change(&id, State::Done)).unwrap(),
        done
    );
    assert_eq!(
        number(
            &office,
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'message_sequence'"
        ),
        sequence
    );
}
