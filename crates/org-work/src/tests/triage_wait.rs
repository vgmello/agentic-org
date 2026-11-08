//! A `new` bug whose suggested triager is the owner waits on the owner (§6), and the time
//! figures of bugs: rounds, cycle time, the verification wait.

use org_proto::work::*;

use super::support::*;
use crate::{read_item, OpenWait, WorkCall};

fn triage_span() -> OpenWait {
    OpenWait {
        on_member_id: OWNER.into(),
        reason: WaitReason::Triage,
    }
}

fn waiting(w: &World, id: &str) -> bool {
    w.item(id).waiting.contains(&triage_span())
}

/// The bug as `work.get` reads it, as of now.
fn read(w: &World, id: &str) -> WorkItem {
    let org = w.store.org.lock().expect("lock").clone();
    read_item(&org, w.store.as_ref(), &w.item(id), w.now()).expect("reads")
}

fn undo(w: &World, id: &str, version: u64) -> WorkCall {
    WorkCall::Undo(WorkUndoParams {
        change_id: w.id(),
        target_id: id.into(),
        event_version: version,
    })
}

// ---- the span ----------------------------------------------------------------------------

#[test]
fn a_bug_the_owner_would_triage_waits_on_the_owner_from_its_report() {
    let w = World::new();
    // General's team has no lead: the owner is the suggested triager.
    let id = w.bug_in(None);
    assert!(waiting(&w, &id));
    assert_eq!(w.kinds_after(&id, 0), ["created", "waiting_started"]);
    let started = &w.events(&id)[1];
    assert_eq!(started.actor_id, None, "office bookkeeping");
    assert!(!started.auto);
    assert_eq!(
        started.cause,
        Some(EventRef {
            target_id: id.clone(),
            version: 1
        })
    );
    assert_eq!(
        started.change,
        WorkEventKind::WaitingStarted {
            on_member_id: OWNER.into(),
            reason: WaitReason::Triage
        }
    );
    assert_eq!(read(&w, &id).suggested_triager.as_deref(), Some(OWNER));
}

#[test]
fn a_bug_in_an_area_with_a_lead_does_not_wait_on_the_owner() {
    let w = World::new();
    let id = w.bug_in(Some(AREA_PAY));
    assert!(!waiting(&w, &id));
    assert_eq!(w.kinds_after(&id, 0), ["created"]);
    assert_eq!(read(&w, &id).suggested_triager.as_deref(), Some("riley"));
}

#[test]
fn the_team_lead_stands_in_for_an_area_without_a_lead() {
    let w = World::new();
    let id = w.bug_in(Some(AREA_UI));
    assert!(waiting(&w, &id), "no area lead and no team lead");
    w.store
        .org_mut(|o| o.team_leads.insert(TEAM.into(), "ada".into()));
    assert_eq!(read(&w, &id).suggested_triager.as_deref(), Some("ada"));
    let fresh = w.bug_in(Some(AREA_UI));
    assert!(!waiting(&w, &fresh));
    // A lead who left falls through to the next in line.
    w.store.org_mut(|o| o.set_area_lead(AREA_PAY, Some("off")));
    assert_eq!(read(&w, &fresh).suggested_triager.as_deref(), Some("ada"));
    let id = w.bug_in(Some(AREA_PAY));
    assert_eq!(read(&w, &id).suggested_triager.as_deref(), Some("ada"));
    // A project with no team at all: the owner.
    let p = w.report_params(OTHER_PROJECT);
    let other = p.item_id.clone();
    w.ok(&pat(), WorkCall::BugReport(p));
    assert!(waiting(&w, &other));
}

#[test]
fn the_suggested_verifier_is_the_reporter_while_a_member_else_the_triager() {
    let w = World::new();
    let id = w.bug_in(Some(AREA_PAY));
    assert_eq!(read(&w, &id).suggested_verifier.as_deref(), Some("pat"));
    let by_agent = {
        let mut p = w.report_params(PROJECT);
        p.area_id = Some(AREA_PAY.into());
        let i = p.item_id.clone();
        w.ok(&agent(), WorkCall::BugReport(p));
        i
    };
    assert_eq!(
        read(&w, &by_agent).suggested_verifier.as_deref(),
        Some("ada")
    );
    // The reporter leaves (an offboarded agent, a removed person).
    w.store.org_mut(|o| {
        if let Some(m) = o.members.get_mut("ada") {
            if let Some(a) = m.agent.as_mut() {
                a.status = org_proto::org::AgentStatus::Offboarded;
            }
        }
        o.members.remove("pat");
    });
    assert_eq!(
        read(&w, &by_agent).suggested_verifier.as_deref(),
        Some("riley")
    );
    assert_eq!(read(&w, &id).suggested_verifier.as_deref(), Some("riley"));
}

#[test]
fn rerouting_moves_the_span_with_the_suggested_triager() {
    let w = World::new();
    let id = w.bug_in(None);
    assert!(waiting(&w, &id));
    let v = w.version(&id);
    let mut p = w.bug_update(&id);
    p.area_id = Some(AREA_PAY.into());
    w.ok(&app(), WorkCall::BugUpdate(p));
    assert!(!waiting(&w, &id), "riley leads there now");
    assert_eq!(w.kinds_after(&id, v), ["fields_changed", "waiting_ended"]);
    assert_eq!(w.item(&id).status, ItemStatus::New, "nothing else changes");

    let mut p = w.bug_update(&id);
    p.area_id = Some(GENERAL.into());
    w.ok(&app(), WorkCall::BugUpdate(p));
    assert!(waiting(&w, &id));

    // Rerouting between two areas with the same triager writes no bookkeeping.
    let v = w.version(&id);
    let mut p = w.bug_update(&id);
    p.area_id = Some(AREA_UI.into());
    w.ok(&app(), WorkCall::BugUpdate(p));
    assert_eq!(w.kinds_after(&id, v), ["fields_changed"]);
    assert!(waiting(&w, &id));
}

#[test]
fn triage_close_and_a_reopen_to_new_keep_the_span_right() {
    let w = World::new();
    // Triage ends it.
    let id = w.bug_in(None);
    let v = w.version(&id);
    w.triage(&id);
    assert!(!waiting(&w, &id));
    assert_eq!(w.kinds_after(&id, v), ["triaged", "waiting_ended"]);

    // Closing a new bug ends it with the round.
    let id = w.bug_in(None);
    let p = w.wont_fix_params(&id);
    w.ok(&app(), WorkCall::BugClose(p));
    assert!(!waiting(&w, &id));

    // Reopening a duplicate goes to New and the span holds again; a plain reopen doesn't.
    let original = w.bug_in(Some(AREA_PAY));
    let dup = w.bug_in(None);
    let p = w.duplicate_params(&dup, &original);
    w.ok(&app(), WorkCall::BugClose(p));
    assert!(!waiting(&w, &dup));
    let p = w.reopen_params(&dup);
    w.ok(&app(), WorkCall::BugReopen(p));
    assert!(waiting(&w, &dup));
    let kinds = w.kinds_after(&dup, 0);
    assert_eq!(&kinds[kinds.len() - 2..], ["reopened", "waiting_started"]);

    let closed = w.bug_at(ItemStatus::Closed);
    let p = w.reopen_params(&closed);
    w.ok(&app(), WorkCall::BugReopen(p));
    assert!(!waiting(&w, &closed));
}

#[test]
fn a_waiting_event_never_makes_an_edit_stale() {
    let w = World::new();
    let id = w.bug_in(None);
    assert_eq!(w.version(&id), 2, "created, then the span");
    let mut p = w.bug_update(&id);
    p.expected_version = 1;
    p.title = Some("A clearer title".into());
    w.ok(&app(), WorkCall::BugUpdate(p));
}

#[test]
fn the_office_bookkeeping_is_never_undone() {
    let w = World::new();
    let id = w.bug_in(None);
    assert_eq!(
        w.code(&app(), undo(&w, &id, 2)),
        org_proto::ErrorCode::InvalidRequest
    );
}

// ---- time figures ------------------------------------------------------------------------

#[test]
fn the_triage_wait_is_waiting_on_the_owner_in_the_time_figures() {
    let w = World::new();
    let id = w.bug_in(None);
    w.clock.advance(2 * HOUR);
    w.triage(&id);
    w.clock.advance(8 * HOUR);
    let t = read(&w, &id).time;
    assert_eq!(t.lead_ms, 10 * HOUR);
    assert_eq!(t.waiting_on_owner_ms, 2 * HOUR, "only until the triage");
    assert_eq!(t.blocked_ms, 0);
    assert_eq!(t.rest_ms, 8 * HOUR);
    assert_eq!((t.working_ms, t.idle_ms), (None, None));
    assert!(t.open);
    assert_eq!(t.cycle_ms, None, "nobody started it");
}

#[test]
fn a_bug_in_an_area_with_a_lead_does_not_count_as_waiting() {
    let w = World::new();
    let id = w.bug_in(Some(AREA_PAY));
    w.clock.advance(5 * HOUR);
    let t = read(&w, &id).time;
    assert_eq!(
        (t.lead_ms, t.waiting_on_owner_ms, t.rest_ms),
        (5 * HOUR, 0, 5 * HOUR)
    );
}

#[test]
fn a_bug_closed_while_waiting_stops_counting_there() {
    let w = World::new();
    let id = w.bug_in(None);
    w.clock.advance(3 * HOUR);
    let p = w.wont_fix_params(&id);
    w.ok(&app(), WorkCall::BugClose(p));
    w.clock.advance(100 * HOUR);
    let t = read(&w, &id).time;
    assert_eq!(
        (t.lead_ms, t.waiting_on_owner_ms, t.open),
        (3 * HOUR, 3 * HOUR, false)
    );
}

#[test]
fn a_reopened_bug_has_two_rounds_and_a_cycle_each() {
    let w = World::new();
    let id = w.bug_in(Some(AREA_PAY));
    w.clock.advance(HOUR);
    w.triage(&id);
    w.clock.advance(HOUR);
    let task = w.fix_task(&id);
    w.set(&task, ItemStatus::InProgress); // the bug starts here
    w.clock.advance(2 * HOUR);
    w.set(&task, ItemStatus::Done); // and is fixed here
    assert_eq!(w.item(&id).status, ItemStatus::Fixed);
    w.clock.advance(HOUR);
    let p = w.verify_params(&id, VerifyOutcome::Fixed);
    w.ok(&pat(), WorkCall::BugVerify(p)); // closed at 5 h
    let t = read(&w, &id).time;
    assert_eq!(
        (t.lead_ms, t.cycle_ms, t.rounds, t.open),
        (5 * HOUR, Some(3 * HOUR), 1, false)
    );

    // Closed time isn't lead time.
    w.clock.advance(10 * HOUR);
    let p = w.reopen_params(&id);
    w.ok(&pat(), WorkCall::BugReopen(p));
    w.clock.advance(2 * HOUR);
    let p = w.fixed_params(&id);
    w.ok(&app(), WorkCall::BugMarkFixed(p)); // fixed by hand: its cycle starts and ends here
    w.clock.advance(HOUR);
    let p = w.verify_params(&id, VerifyOutcome::Fixed);
    w.ok(&pat(), WorkCall::BugVerify(p));
    let t = read(&w, &id).time;
    assert_eq!(t.rounds, 2);
    assert_eq!(t.lead_ms, 5 * HOUR + 3 * HOUR);
    assert_eq!(t.cycle_ms, Some(3 * HOUR + HOUR));
    assert!(!t.open);
}

#[test]
fn the_wait_for_verification_is_the_owners_when_the_owner_reported_the_bug() {
    let w = World::new();
    let mut p = w.report_params(PROJECT);
    p.area_id = Some(AREA_PAY.into());
    let id = p.item_id.clone();
    w.ok(&app(), WorkCall::BugReport(p));
    w.triage(&id);
    w.clock.advance(HOUR);
    let p = w.fixed_params(&id);
    w.ok(&app(), WorkCall::BugMarkFixed(p));
    w.clock.advance(3 * HOUR);
    let t = read(&w, &id).time;
    assert_eq!(t.lead_ms, 4 * HOUR);
    assert_eq!(
        t.waiting_on_owner_ms,
        3 * HOUR,
        "fixed, and the owner is to verify"
    );

    // Sent back: it is not waiting on the owner any more.
    let p = w.verify_params(&id, VerifyOutcome::StillHappens);
    w.ok(&app(), WorkCall::BugVerify(p));
    w.clock.advance(HOUR);
    let t = read(&w, &id).time;
    assert_eq!(
        (t.lead_ms, t.waiting_on_owner_ms, t.rounds),
        (5 * HOUR, 3 * HOUR, 1)
    );

    // Another person's bug doesn't wait on the owner when fixed.
    let other = w.bug_in(Some(AREA_PAY));
    w.triage(&other);
    let p = w.fixed_params(&other);
    w.ok(&app(), WorkCall::BugMarkFixed(p));
    w.clock.advance(2 * HOUR);
    assert_eq!(read(&w, &other).time.waiting_on_owner_ms, 0);
}

#[test]
fn undoing_an_automatic_fix_stops_the_verification_wait() {
    let w = World::new();
    let mut p = w.report_params(PROJECT);
    p.area_id = Some(AREA_PAY.into());
    let id = p.item_id.clone();
    w.ok(&app(), WorkCall::BugReport(p));
    w.triage(&id);
    let task = w.fix_task(&id);
    w.set(&task, ItemStatus::Done);
    let auto = w.version(&id);
    w.clock.advance(HOUR);
    w.ok(&app(), undo(&w, &id, auto));
    w.clock.advance(2 * HOUR);
    let t = read(&w, &id).time;
    assert_eq!(t.lead_ms, 3 * HOUR);
    assert_eq!(t.waiting_on_owner_ms, HOUR, "only while it was fixed");
}

#[test]
fn bug_figures_are_the_same_however_the_events_are_batched() {
    let w = World::new();
    let id = w.bug_in(None);
    w.clock.advance(HOUR);
    w.triage(&id);
    let task = w.fix_task(&id);
    w.clock.advance(HOUR);
    w.set(&task, ItemStatus::InProgress);
    w.clock.advance(HOUR);
    w.set(&task, ItemStatus::Done);
    w.clock.advance(HOUR);
    let p = w.verify_params(&id, VerifyOutcome::StillHappens);
    w.ok(&pat(), WorkCall::BugVerify(p));
    w.clock.advance(HOUR);
    let p = w.wont_fix_params(&id);
    w.ok(&app(), WorkCall::BugClose(p));
    w.clock.advance(HOUR);
    let p = w.reopen_params(&id);
    w.ok(&app(), WorkCall::BugReopen(p));
    w.clock.advance(HOUR);

    let events = w.events(&id);
    let as_of = w.now();
    let whole = crate::time_figures(ItemKind::Bug, "pat", OWNER, &events, as_of);
    for split in 0..=events.len() {
        let mut tally = crate::TimeTally::new(ItemKind::Bug, "pat", OWNER);
        for e in &events[..split] {
            tally.push(e);
        }
        // Stored between batches, as an office may.
        let stored = serde_json::to_string(&tally).expect("encodes");
        let mut tally: crate::TimeTally = serde_json::from_str(&stored).expect("decodes");
        for e in &events[split..] {
            tally.push(e);
        }
        assert_eq!(tally.figures(as_of), whole, "split at {split}");
    }
    assert_eq!(
        whole.waiting_on_owner_ms + whole.blocked_ms + whole.rest_ms,
        whole.lead_ms
    );
    assert_eq!(whole.rounds, 2);
    assert!(whole.open);
}
