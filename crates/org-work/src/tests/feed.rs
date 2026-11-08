//! `work.changed` and the feed: one row and one position per committed call.

use org_proto::org::Position;
use org_proto::work::*;

use super::support::*;
use crate::TxnOutput;

#[test]
fn a_committed_call_answers_with_its_position_and_sends_its_changes() {
    let w = World::new();
    w.store
        .org_mut(|o| o.position = Position { epoch: 1, seq: 7 });
    let (parent, step) = w.task_with_step(false);
    let (task, call) = w.make_task(&step);
    let change_id = call.change_id().to_owned();
    let TxnOutput::Committed { accepted, changed } = w.call(&agent(), call) else {
        panic!("committed")
    };
    let position = accepted.position.expect("a position");
    assert_eq!(
        position,
        WorkPosition {
            incarnation: 1,
            seq: 2
        }
    );
    assert_eq!(changed.len(), 1);
    let data = &changed[0];
    assert_eq!((data.position, data.part, data.parts), (position, 1, 1));
    assert_eq!(data.change_id.as_deref(), Some(change_id.as_str()));
    let kinds: Vec<String> = data
        .changes
        .iter()
        .map(|c| match c {
            WorkChange::ItemUpserted { item } => format!("item {}", item.id == parent),
            WorkChange::EventAdded { event } => format!("event {}", event.target_id == task),
            _ => "other".into(),
        })
        .collect();
    assert_eq!(
        kinds,
        ["item true", "event false", "item false", "event true"]
    );

    let feed = w.store.snapshot().feed;
    let row = feed.last().unwrap();
    assert_eq!(row.change_id.as_deref(), Some(change_id.as_str()));
    assert_eq!(row.org_position, Position { epoch: 1, seq: 7 });
    assert_eq!(
        row.refs,
        vec![
            EventRef {
                target_id: parent.clone(),
                version: 2
            },
            EventRef {
                target_id: task.clone(),
                version: 1
            },
        ]
    );
    assert!(w
        .events(&task)
        .iter()
        .all(|e| e.org_position == Position { epoch: 1, seq: 7 }));
}

#[test]
fn a_call_that_changes_nothing_sends_nothing() {
    let w = World::new();
    let (_, step) = w.task_with_step(false);
    let rows = w.store.snapshot().feed.len();
    let TxnOutput::Committed { accepted, changed } = w.call(&agent(), w.check(&step, false)) else {
        panic!("committed")
    };
    assert_eq!((accepted.position, changed.len()), (None, 0));
    assert_eq!(w.store.snapshot().feed.len(), rows);
}

#[test]
fn changes_over_the_size_limit_are_split_into_numbered_parts_in_order() {
    let w = World::new();
    let id = w.task();
    let item = w.item(&id).summary();
    let changes: Vec<WorkChange> = (0..50)
        .map(|_| WorkChange::ItemUpserted { item: item.clone() })
        .collect();
    let one = serde_json::to_vec(&changes[0]).unwrap().len();
    let position = WorkPosition {
        incarnation: 1,
        seq: 9,
    };
    let parts = crate::feed::split(position, Some("c1"), changes.clone(), 512 + 10 * one + 20);
    assert!(parts.len() >= 5);
    let n = parts.len() as u32;
    for (i, p) in parts.iter().enumerate() {
        assert_eq!((p.part, p.parts, p.position), (i as u32 + 1, n, position));
        assert_eq!(p.change_id.as_deref(), Some("c1"));
        assert!(serde_json::to_vec(p).unwrap().len() <= 512 + 10 * one + 20);
    }
    let rejoined: Vec<WorkChange> = parts.into_iter().flat_map(|p| p.changes).collect();
    assert_eq!(rejoined, changes);
}
