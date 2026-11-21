//! The worker's custody adapter against the host's rule (`test_support::FakeWriter`):
//! what it admits, how it confirms, and what the office refuses (M5, L8).
use super::*;
use crate::hq_creation::tests::{code, key, World, INSTANCE, OFFICE, OWNER};
use crate::test_support::{test_admission, FakeWriter};
use org_office::rules::OrgCommand;
use org_office::Office;
use org_proto::org::TeamCreateParams;

/// The top team: the creation's home team.
const TOP_TEAM: &str = "019a8928-1337-7456-8456-0123456789ae";

fn team_create(n: u64) -> OrgCommand {
    OrgCommand::TeamCreate(TeamCreateParams {
        command_id: format!("019a0000-0000-7000-8000-{n:012x}"),
        team_id: format!("01900000-0000-7000-8000-{n:012x}"),
        name: format!("Team {n}"),
        owner_team_id: Some(TOP_TEAM.into()),
    })
}

/// The office's journal segments, oldest first.
fn segments(world: &World) -> Vec<std::path::PathBuf> {
    let identity = &world.writer.admission().identity;
    let dir = world
        .roots
        .durable
        .join("recovery/offices")
        .join(&identity.office_id)
        .join("journals/acceptance")
        .join(&identity.journal_incarnation)
        .join("segments");
    let mut found: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "log"))
        .collect();
    found.sort();
    found
}

/// The causes behind an office error, as the worker logs them.
fn causes(error: &org_office::OfficeError) -> String {
    let mut text = String::new();
    let mut cause = std::error::Error::source(error);
    while let Some(inner) = cause {
        text.push_str(&inner.to_string());
        text.push('\n');
        cause = inner.source();
    }
    text
}

#[test]
fn the_admission_must_be_valid_and_for_this_instance_and_office() {
    let world = World::new();
    let admission = world.writer.admission().clone();
    let refused = |changed: Admission| {
        let writer = FakeWriter::new(changed);
        let (worker, _thread) = writer.channel();
        install(worker, INSTANCE, OFFICE).is_err()
    };
    assert!(!refused(admission.clone()));
    let mut other = admission.clone();
    other.identity.instance_id = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071ff".into();
    assert!(refused(other));
    let mut other = admission.clone();
    other.identity.office_id = "019a8928-1337-7456-8456-0123456789ff".into();
    assert!(refused(other));
    // The owner account in another form than the host's.
    let mut other = admission.clone();
    other.owner_account = "test-account".into();
    assert!(refused(other));
    let mut other = admission.clone();
    other.version = 1;
    assert!(refused(other));
    let mut other = admission;
    other.runtime_root = other.durable_root.clone();
    assert!(refused(other));
    // No admission at all: the host closed the channel.
    let (worker, host) = UnixStream::pair().unwrap();
    drop(host);
    assert!(install(worker, INSTANCE, OFFICE).is_err());
}

/// An admission for other roots or another organization is refused by the office: it
/// checks them against the intent its creation froze.
#[test]
fn the_office_refuses_custody_or_roots_that_differ_from_its_creation() {
    let world = World::new();
    world.prepare(Some(key()), code()).unwrap().close().unwrap();
    let admission = world.writer.admission();
    let elsewhere = tempfile::tempdir().unwrap();
    let elsewhere = elsewhere.path().canonicalize().unwrap();
    let (durable, runtime) = (elsewhere.join("d"), elsewhere.join("r"));
    for made in [&durable, &runtime] {
        std::fs::create_dir_all(made).unwrap();
    }
    let moved = FakeWriter::new(test_admission(
        INSTANCE,
        &admission.identity.organization_id,
        OFFICE,
        OWNER,
        &durable,
        &runtime,
    ));
    let (custody, roots) = moved.admitted();
    assert!(Office::open(&world.folder, Some(key()), INSTANCE, roots, custody).is_err());
    let mut foreign = admission.clone();
    foreign.identity.organization_id = "019a8928-1337-7123-8123-0123456789ff".into();
    let (custody, roots) = FakeWriter::new(foreign).admitted();
    assert!(Office::open(&world.folder, Some(key()), INSTANCE, roots, custody).is_err());
    world.open(Some(key())).unwrap();
}

/// L8: the office confirms once per group flush, never once per operation. Three
/// `team.create` committed at once are confirmed in fewer confirmations than operations.
#[test]
fn the_adapter_confirms_per_flush_not_per_operation() {
    let world = World::new();
    let office = std::sync::Arc::new(world.prepare(Some(key()), code()).unwrap());
    let before = world.writer.confirms();
    // The three submits start together, so they meet in one group flush.
    let start = std::sync::Barrier::new(3);
    std::thread::scope(|scope| {
        for n in 1..=3 {
            let office = std::sync::Arc::clone(&office);
            let start = &start;
            scope.spawn(move || {
                start.wait();
                let accepted = office.submit_org(OWNER, team_create(n)).unwrap().unwrap();
                assert_eq!(accepted.status, org_proto::org::CommandStatus::Committed);
            });
        }
    });
    let confirms = world.writer.confirms() - before;
    assert!(confirms >= 1, "the commits were never confirmed");
    assert!(confirms < 3, "{confirms} confirmations for 3 operations");
    // Every confirmation was accepted: the host holds the office's flushed head.
    let head = world.writer.head().unwrap();
    assert!(head.ordinal >= 4, "{head:?}");
    let teams = office.org_snapshot().unwrap().teams.len();
    std::sync::Arc::try_unwrap(office).unwrap().close().unwrap();
    let reopened = world.open(Some(key())).unwrap();
    assert_eq!(reopened.org_snapshot().unwrap().teams.len(), teams);
}

/// M5 (a): one flipped byte in an entry at or below the head the host confirmed: the
/// office refuses to open, and the cause names the ordinal.
#[test]
fn a_flipped_byte_at_or_below_the_confirmed_head_refuses_and_names_the_ordinal() {
    let world = World::new();
    let office = world.prepare(Some(key()), code()).unwrap();
    office.submit_org(OWNER, team_create(1)).unwrap().unwrap();
    drop(office);
    let confirmed = world.writer.head().unwrap();
    assert!(confirmed.ordinal >= 2, "{confirmed:?}");
    let segment = segments(&world).pop().expect("a journal segment");
    let mut bytes = std::fs::read(&segment).unwrap();
    // A byte of the first entry: entry 1, below the confirmed head.
    let at = 64;
    bytes[at] ^= 0x01;
    std::fs::write(&segment, &bytes).unwrap();
    let error = world.open(Some(key())).unwrap_err();
    let causes = causes(&error);
    assert!(causes.contains("journal entry 1 is corrupt"), "{causes}");
    // Never truncated away.
    assert_eq!(std::fs::read(&segment).unwrap(), bytes);
}

/// M5 (b): a journal behind the head the host confirmed (an older copy of the segments)
/// is refused.
#[test]
fn an_older_segment_copy_behind_the_host_checkpoint_is_refused() {
    let world = World::new();
    let office = world.prepare(Some(key()), code()).unwrap();
    drop(office);
    let saved: Vec<_> = segments(&world)
        .into_iter()
        .map(|path| (path.clone(), std::fs::read(&path).unwrap()))
        .collect();
    let office = world.open(Some(key())).unwrap();
    office.submit_org(OWNER, team_create(1)).unwrap().unwrap();
    drop(office);
    for (path, bytes) in &saved {
        std::fs::write(path, bytes).unwrap();
    }
    let error = world.open(Some(key())).unwrap_err();
    let causes = causes(&error);
    assert!(causes.contains("behind the confirmed head"), "{causes}");
}

/// M5 (c): a closed custody channel fails closed. Once the host's end closes, a write
/// that needs its confirmation fails, and the channel is never used again, even for a
/// request the host would answer.
#[test]
fn a_closed_custody_channel_fails_closed() {
    let world = World::new();
    world.prepare(Some(key()), code()).unwrap().close().unwrap();
    let (worker, host) = UnixStream::pair().unwrap();
    let host_end = host.try_clone().unwrap();
    let thread = world.writer.serve(host);
    let admitted = install(worker, INSTANCE, OFFICE).unwrap();
    let authority = std::sync::Arc::clone(&admitted.custody.authority);
    let identity = admitted.custody.identity.clone();
    let office = Office::open(
        &world.folder,
        Some(key()),
        INSTANCE,
        admitted.roots,
        admitted.custody,
    )
    .unwrap();
    let confirmed = world.writer.head();
    // The host goes away.
    host_end.shutdown(std::net::Shutdown::Both).unwrap();
    thread.join().unwrap();
    let refused = office.submit_org(OWNER, team_create(1));
    assert!(
        !matches!(refused, Ok(Ok(_))),
        "a write was answered without the host's confirmation: {refused:?}"
    );
    assert_eq!(world.writer.head(), confirmed);
    // Fails closed: nothing is sent on the channel again.
    assert!(authority.checkpoint(&identity).is_err());
    assert!(office.submit_org(OWNER, team_create(2)).is_err() || office.fatal().is_some());
}
