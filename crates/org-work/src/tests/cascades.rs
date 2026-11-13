//! What organization changes mean for open bugs (§2.5): a deleted area's open bugs move to its
//! replacement and closed ones stay; the triage span follows the suggested triager; and running
//! it again changes nothing.

use org_proto::work::*;

use super::support::*;
use crate::{OpenWait, TxnOutput, WorkCall};

fn spans(w: &World, id: &str) -> usize {
    w.item(id)
        .waiting
        .iter()
        .filter(|s| {
            *s == &OpenWait {
                on_member_id: OWNER.into(),
                reason: WaitReason::Triage,
            }
        })
        .count()
}

fn reconcile(w: &World) -> TxnOutput {
    w.svc.reconcile_bugs().expect("no storage error")
}

fn changed(out: &TxnOutput) -> bool {
    match out {
        TxnOutput::Committed { accepted, .. } => accepted.position.is_some(),
        other => panic!("expected a commit, got {other:?}"),
    }
}

#[test]
fn nothing_to_do_commits_nothing() {
    let w = World::new();
    w.bug();
    w.bug_in(None);
    let feed = w.store.snapshot().feed.len();
    let out = reconcile(&w);
    assert!(!changed(&out));
    assert_eq!(w.store.snapshot().feed.len(), feed, "no feed row");
}

#[test]
fn b111_reconciliation_rejects_numbered_projectless_bug_before_any_domain_writes() {
    let w = World::new();
    w.bug_in(Some(AREA_UI));
    let invalid = w.bug_in(Some(AREA_UI));
    {
        let mut state = w.store.work.lock().unwrap();
        let bug = state.items.get_mut(&invalid).unwrap();
        bug.number = Some(7);
        bug.project_id = None;
    }
    w.store.org_mut(|o| o.delete_area(AREA_UI, AREA_PAY));
    let before = w.store.snapshot();
    let out = reconcile(&w);
    let TxnOutput::Refused(error) = out else {
        panic!("numbered projectless bug must refuse reconciliation: {out:?}");
    };
    assert_eq!(error.code, org_proto::ErrorCode::InvalidRequest);
    assert_eq!(error.message, "a numbered task or bug must have a project");
    let after = w.store.snapshot();
    assert_eq!(after.items, before.items);
    assert_eq!(after.events, before.events);
    assert_eq!(after.numbers, before.numbers);
    assert_eq!(after.feed, before.feed);
}

#[test]
fn deleting_an_area_moves_its_open_bugs_and_leaves_the_closed_ones() {
    let w = World::new();
    let new = w.bug_in(Some(AREA_UI));
    let triaged = {
        let id = w.bug_in(Some(AREA_UI));
        w.triage(&id);
        id
    };
    let in_progress = {
        let id = w.bug_in(Some(AREA_UI));
        w.triage(&id);
        let t = w.fix_task(&id);
        w.set(&t, ItemStatus::InProgress);
        id
    };
    let fixed = {
        let id = w.bug_in(Some(AREA_UI));
        w.triage(&id);
        let p = w.fixed_params(&id);
        w.ok(&app(), WorkCall::BugMarkFixed(p));
        id
    };
    let closed = {
        let id = w.bug_in(Some(AREA_UI));
        let p = w.wont_fix_params(&id);
        w.ok(&app(), WorkCall::BugClose(p));
        id
    };
    let elsewhere = w.bug_in(Some(AREA_PAY));
    let deleted_bug = {
        let id = w.bug_in(Some(AREA_UI));
        w.ok(&app(), w.delete(&id));
        id
    };
    let versions: Vec<u64> = [&new, &closed, &elsewhere, &deleted_bug]
        .iter()
        .map(|i| w.version(i))
        .collect();

    // Not yet: the office has not applied the deletion.
    assert!(!changed(&reconcile(&w)));
    w.store.org_mut(|o| o.delete_area(AREA_UI, AREA_PAY));
    let out = reconcile(&w);
    assert!(changed(&out));

    for id in [&new, &triaged, &in_progress, &fixed] {
        assert_eq!(w.item(id).bug.area_id.as_deref(), Some(AREA_PAY), "{id}");
    }
    assert_eq!(
        w.item(&closed).bug.area_id.as_deref(),
        Some(AREA_UI),
        "closed keeps it"
    );
    assert_eq!(w.item(&elsewhere).bug.area_id.as_deref(), Some(AREA_PAY));
    assert_eq!(w.item(&deleted_bug).bug.area_id.as_deref(), Some(AREA_UI));
    assert_eq!(
        [&new, &closed, &elsewhere, &deleted_bug]
            .map(|i| w.version(i))
            .to_vec(),
        [versions[0] + 2, versions[1], versions[2], versions[3]],
        "the new bug also loses its triage span; the others are untouched"
    );
    assert_eq!(w.item(&triaged).status, ItemStatus::Triaged, "status kept");

    // A system event: no actor, not automatic, never undone.
    let e = w.events(&triaged).pop().expect("event");
    assert_eq!((e.actor_id, e.auto, e.cause.clone()), (None, false, None));
    assert_eq!(
        e.change,
        WorkEventKind::FieldsChanged {
            fields: vec![FieldChange {
                field: "area_id".into(),
                from: Some(AREA_UI.into()),
                to: Some(AREA_PAY.into()),
            }]
        }
    );
    assert_eq!(
        w.code(
            &app(),
            WorkCall::Undo(WorkUndoParams {
                change_id: w.id(),
                target_id: triaged.clone(),
                event_version: e.version,
            })
        ),
        org_proto::ErrorCode::InvalidRequest
    );

    // One committed call: one feed row for every bug it changed, and no change id.
    let TxnOutput::Committed { accepted, changed } = out else {
        unreachable!()
    };
    assert_eq!(accepted.items.len(), 4);
    let row = w.store.snapshot().feed.pop().expect("feed row");
    assert_eq!(row.change_id, None);
    assert_eq!(Some(row.position), accepted.position);
    assert!(!changed.is_empty());

    // Idempotent: again, nothing.
    let feed = w.store.snapshot().feed.len();
    assert!(!changed_flag(&reconcile(&w)));
    assert_eq!(w.store.snapshot().feed.len(), feed);
}

fn changed_flag(out: &TxnOutput) -> bool {
    changed(out)
}

#[test]
fn a_replacement_that_was_itself_deleted_is_followed() {
    let w = World::new();
    let id = w.bug_in(Some(AREA_UI));
    w.triage(&id);
    w.store.org_mut(|o| {
        o.delete_area(AREA_UI, AREA_PAY);
        o.delete_area(AREA_PAY, GENERAL);
    });
    reconcile(&w);
    assert_eq!(w.item(&id).bug.area_id.as_deref(), Some(GENERAL));
}

#[test]
fn a_deleted_area_with_no_live_replacement_leaves_its_bugs_alone() {
    let w = World::new();
    let id = w.bug_in(Some(AREA_UI));
    w.triage(&id);
    w.store.org_mut(|o| {
        o.delete_area(AREA_UI, "gone");
    });
    let v = w.version(&id);
    assert!(!changed(&reconcile(&w)));
    assert_eq!(w.version(&id), v);
}

#[test]
fn a_moved_new_bug_waits_on_the_owner_if_its_new_triager_is_the_owner() {
    let w = World::new();
    let id = w.bug_in(Some(AREA_PAY)); // riley leads
    assert_eq!(spans(&w, &id), 0);
    w.store.org_mut(|o| o.delete_area(AREA_PAY, GENERAL));
    reconcile(&w);
    let bug = w.item(&id);
    assert_eq!(bug.bug.area_id.as_deref(), Some(GENERAL));
    assert_eq!(spans(&w, &id), 1);
    assert_eq!(w.kinds_after(&id, 1), ["fields_changed", "waiting_started"]);
    reconcile(&w);
    assert_eq!(spans(&w, &id), 1, "opened once");
}

#[test]
fn a_changed_lead_opens_and_closes_the_span_once() {
    let w = World::new();
    let id = w.bug_in(Some(AREA_PAY));
    let triaged = w.bug_in(Some(AREA_PAY));
    w.triage(&triaged);
    assert_eq!(spans(&w, &id), 0);

    // The lead is removed (the organization's cascade cleared it): the owner triages.
    w.store.org_mut(|o| o.set_area_lead(AREA_PAY, None));
    assert!(changed(&reconcile(&w)));
    assert_eq!(spans(&w, &id), 1);
    assert_eq!(spans(&w, &triaged), 0, "only new bugs wait for triage");
    assert!(!changed(&reconcile(&w)), "once");
    assert_eq!(spans(&w, &id), 1);

    // A lead again: the span closes.
    w.store
        .org_mut(|o| o.set_area_lead(AREA_PAY, Some("riley")));
    assert!(changed(&reconcile(&w)));
    assert_eq!(spans(&w, &id), 0);
    assert_eq!(w.kinds_after(&id, 1), ["waiting_started", "waiting_ended"]);
    assert!(!changed(&reconcile(&w)));

    // A team lead appointed for an area with no lead also takes the wait away.
    let ui = w.bug_in(Some(AREA_UI));
    assert_eq!(spans(&w, &ui), 1);
    w.store.org_mut(|o| {
        o.team_leads.insert(TEAM.into(), "riley".into());
    });
    reconcile(&w);
    assert_eq!(spans(&w, &ui), 0);
}

#[test]
fn the_time_figures_see_the_span_a_cascade_writes() {
    let w = World::new();
    let id = w.bug_in(Some(AREA_PAY));
    w.clock.advance(2 * HOUR);
    w.store.org_mut(|o| o.set_area_lead(AREA_PAY, None));
    reconcile(&w);
    w.clock.advance(3 * HOUR);
    w.triage(&id);
    w.clock.advance(HOUR);
    let org = w.store.org.lock().expect("lock").clone();
    let t = crate::read_item(&org, w.store.as_ref(), &w.item(&id), w.now())
        .expect("reads")
        .time;
    assert_eq!((t.lead_ms, t.waiting_on_owner_ms), (6 * HOUR, 3 * HOUR));
}
