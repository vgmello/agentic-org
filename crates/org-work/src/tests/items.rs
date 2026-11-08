//! Comments, watching, links, deleting, recipients and identity.

use org_proto::work::*;
use org_proto::ErrorCode;

use super::support::*;
use crate::testkit::MemStore;
use crate::{read_item, Caller, WorkCall};

fn question(w: &World, id: &str, asks: &str) -> WorkCommentParams {
    let mut c = w.comment(id, "Which payment API?");
    c.comment_kind = Some(CommentKind::Question);
    c.asks = Some(asks.into());
    c
}

fn answer(w: &World, id: &str, q: &str) -> WorkCommentParams {
    let mut c = w.comment(id, "Stripe.");
    c.comment_kind = Some(CommentKind::Answer);
    c.answers = Some(q.into());
    c
}

fn started(q: &str) -> WorkEventKind {
    WorkEventKind::WaitingStarted {
        on_member_id: OWNER.into(),
        reason: WaitReason::Question {
            comment_id: q.into(),
        },
    }
}

fn ended(q: &str) -> WorkEventKind {
    WorkEventKind::WaitingEnded {
        on_member_id: OWNER.into(),
        reason: WaitReason::Question {
            comment_id: q.into(),
        },
    }
}

#[test]
fn each_question_to_the_owner_has_its_own_span_closed_by_its_first_answer() {
    let w = World::new();
    let id = w.task();
    let q1 = question(&w, &id, "owner");
    let q1_id = q1.comment_id.clone();
    w.ok(&agent(), WorkCall::Comment(q1));
    let comment = &w.events(&id)[1];
    assert!(matches!(&comment.change, WorkEventKind::Comment { asks: Some(a), .. } if a == OWNER));
    let span = w.last_event(&id);
    assert_eq!(span.version, 3);
    assert_eq!(span.change, started(&q1_id));
    assert_eq!((span.actor_id.as_deref(), span.auto), (None, false));
    assert_eq!(span.cause.as_ref().map(|c| c.version), Some(2));

    let q2 = question(&w, &id, OWNER);
    let q2_id = q2.comment_id.clone();
    w.ok(&agent(), WorkCall::Comment(q2));
    assert_eq!(w.last_event(&id).change, started(&q2_id));
    w.ok(&app(), WorkCall::Comment(answer(&w, &id, &q1_id)));
    assert_eq!(w.last_event(&id).change, ended(&q1_id));
    assert_eq!(w.item(&id).waiting.len(), 1);
    // A second answer to an answered question records only the comment.
    let n = w.events(&id).len();
    w.ok(&app(), WorkCall::Comment(answer(&w, &id, &q1_id)));
    assert_eq!(w.events(&id).len(), n + 1);
    w.ok(&app(), WorkCall::Comment(answer(&w, &id, &q2_id)));
    assert_eq!(w.last_event(&id).change, ended(&q2_id));
    assert!(w.item(&id).waiting.is_empty());
}

#[test]
fn a_rounds_end_closes_every_open_span_and_a_reopen_opens_the_ones_that_still_hold() {
    let w = World::new();
    let id = w.task();
    let q1 = question(&w, &id, OWNER);
    let q1_id = q1.comment_id.clone();
    w.ok(&agent(), WorkCall::Comment(q1));
    let q2 = question(&w, &id, OWNER);
    let q2_id = q2.comment_id.clone();
    w.ok(&agent(), WorkCall::Comment(q2));
    let v = w.version(&id);
    w.set(&id, ItemStatus::Done);
    let events = w.events(&id);
    let after: Vec<WorkEventKind> = events[v as usize..]
        .iter()
        .map(|e| e.change.clone())
        .collect();
    assert!(matches!(after[0], WorkEventKind::StatusChanged { .. }));
    assert_eq!(after[1..], [ended(&q1_id), ended(&q2_id)]);
    assert!(events[v as usize + 1..]
        .iter()
        .all(|e| e.cause.as_ref().map(|c| c.version) == Some(v + 1)));
    assert!(w.item(&id).waiting.is_empty());
    // Answered while done: nothing to end. Reopened: only the open question waits again.
    w.ok(&app(), WorkCall::Comment(answer(&w, &id, &q1_id)));
    assert!(matches!(
        w.last_event(&id).change,
        WorkEventKind::Comment { .. }
    ));
    w.set(&id, ItemStatus::Todo);
    assert_eq!(w.last_event(&id).change, started(&q2_id));
    // A question asked while done opens nothing until the reopen.
    w.set(&id, ItemStatus::Done);
    w.ok(&agent(), WorkCall::Comment(question(&w, &id, OWNER)));
    assert!(matches!(
        w.last_event(&id).change,
        WorkEventKind::Comment { .. }
    ));
    assert!(w.item(&id).waiting.is_empty());
}

#[test]
fn a_question_to_someone_else_opens_no_span() {
    let w = World::new();
    let id = w.task();
    w.ok(&agent(), WorkCall::Comment(question(&w, &id, "riley")));
    assert!(matches!(
        w.last_event(&id).change,
        WorkEventKind::Comment { .. }
    ));
}

#[test]
fn comment_refusals() {
    let w = World::new();
    let id = w.task();
    let mut c = w.comment(&id, "x");
    c.asks = Some(OWNER.into());
    assert_eq!(
        w.code(&agent(), WorkCall::Comment(c)),
        ErrorCode::InvalidRequest
    );
    let mut c = w.comment(&id, "x");
    c.comment_kind = Some(CommentKind::Answer);
    assert_eq!(
        w.code(&agent(), WorkCall::Comment(c)),
        ErrorCode::InvalidRequest
    );
    let mut c = w.comment(&id, "x");
    c.answers = Some(w.id());
    assert_eq!(
        w.code(&agent(), WorkCall::Comment(c)),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        w.code(&agent(), WorkCall::Comment(answer(&w, &id, &w.id()))),
        ErrorCode::NotFound
    );
    assert_eq!(
        w.code(&agent(), WorkCall::Comment(question(&w, &id, "off"))),
        ErrorCode::UnknownRecipient
    );
    let mut c = w.comment(&id, "   ");
    c.body = "   ".into();
    assert_eq!(
        w.code(&agent(), WorkCall::Comment(c)),
        ErrorCode::InvalidRequest
    );
    let mut c = w.comment(&id, "x");
    c.comment_kind = Some(CommentKind::Unknown);
    assert_eq!(
        w.code(&agent(), WorkCall::Comment(c)),
        ErrorCode::InvalidRequest
    );
    // A comment id is a client-made id like any other.
    let mut c = w.comment(&id, "x");
    c.comment_id = id.clone();
    assert_eq!(
        w.code(&agent(), WorkCall::Comment(c)),
        ErrorCode::AlreadyExists
    );
    assert_eq!(
        w.code(&agent(), WorkCall::Comment(w.comment(&w.id(), "x"))),
        ErrorCode::NotFound
    );
}

#[test]
fn comment_with_watch_and_work_watch() {
    let w = World::new();
    let id = w.task();
    let mut c = w.comment(&id, "following");
    c.watch = true;
    w.ok(&agent(), WorkCall::Comment(c));
    assert!(w.item(&id).watchers.contains(&"ada".to_owned()));
    assert!(matches!(
        w.last_event(&id).change,
        WorkEventKind::Comment { watch: true, .. }
    ));
    let watch = |watching| {
        WorkCall::Watch(WorkWatchParams {
            change_id: w.id(),
            item_id: id.clone(),
            watching,
        })
    };
    // Already watching: nothing to record.
    let n = w.events(&id).len();
    let a = w.ok(&agent(), watch(true));
    assert_eq!((a.position, w.events(&id).len()), (None, n));
    w.ok(&agent(), watch(false));
    assert!(!w.item(&id).watchers.contains(&"ada".to_owned()));
    assert_eq!(
        w.last_event(&id).change,
        WorkEventKind::Watched {
            member_id: "ada".into(),
            watching: false
        }
    );
    // The creator (the app's owner here) always watches.
    assert_eq!(w.code(&app(), watch(false)), ErrorCode::InvalidRequest);
}

fn link(w: &World, from: &str, kind: LinkKind, to: LinkTarget) -> (String, WorkCall) {
    let link_id = w.id();
    let call = WorkCall::Link(WorkLinkParams {
        change_id: w.id(),
        link_id: link_id.clone(),
        from_item_id: from.into(),
        kind,
        to,
    });
    (link_id, call)
}

fn to_item(id: &str) -> LinkTarget {
    LinkTarget::Item { item_id: id.into() }
}

#[test]
fn relates_to_links_to_items_and_docs() {
    let w = World::new();
    let a = w.task();
    let b = w.task();
    let (l1, call) = link(&w, &a, LinkKind::RelatesTo, to_item(&b));
    w.ok(&agent(), call);
    let (_, call) = link(
        &w,
        &a,
        LinkKind::RelatesTo,
        LinkTarget::Doc {
            project_id: PROJECT.into(),
            doc_key: "ADR-0004".into(),
        },
    );
    w.ok(&agent(), call);
    assert_eq!(w.item(&a).live_links().count(), 2);
    let WorkEventKind::Linked { link: made } = w.last_event(&a).change else {
        panic!("linked")
    };
    assert_eq!(
        (made.from_item_id.as_str(), made.created_by.as_str()),
        (a.as_str(), "ada")
    );

    // The same link again, a link to itself, other kinds, unknown targets.
    let (_, call) = link(&w, &a, LinkKind::RelatesTo, to_item(&b));
    assert_eq!(w.code(&agent(), call), ErrorCode::AlreadyExists);
    let (_, call) = link(&w, &a, LinkKind::RelatesTo, to_item(&a));
    assert_eq!(w.code(&agent(), call), ErrorCode::InvalidRequest);
    for kind in [
        LinkKind::Fixes,
        LinkKind::PartOf,
        LinkKind::FromRequest,
        LinkKind::PaysBack,
    ] {
        let (_, call) = link(&w, &a, kind, to_item(&b));
        assert_eq!(
            w.code(&agent(), call),
            ErrorCode::InvalidRequest,
            "{kind:?}"
        );
    }
    let (_, call) = link(&w, &a, LinkKind::RelatesTo, to_item(&w.id()));
    assert_eq!(w.code(&agent(), call), ErrorCode::NotFound);
    let (_, call) = link(&w, &a, LinkKind::RelatesTo, LinkTarget::Unknown);
    assert_eq!(w.code(&agent(), call), ErrorCode::InvalidRequest);

    // Unlink, then it's gone.
    let unlink = |id: &str| {
        WorkCall::Unlink(WorkUnlinkParams {
            change_id: w.id(),
            link_id: id.into(),
        })
    };
    w.ok(&agent(), unlink(&l1));
    assert_eq!(w.item(&a).live_links().count(), 1);
    assert_eq!(w.code(&agent(), unlink(&l1)), ErrorCode::NotFound);
}

#[test]
fn part_of_links_end_only_through_the_checklist() {
    let w = World::new();
    let (_, step) = w.task_with_step(false);
    let (task, call) = w.make_task(&step);
    w.ok(&agent(), call);
    let link_id = w.item(&task).part_of().unwrap().id.clone();
    let call = WorkCall::Unlink(WorkUnlinkParams {
        change_id: w.id(),
        link_id,
    });
    assert_eq!(w.code(&agent(), call), ErrorCode::InvalidRequest);
}

#[test]
fn an_item_has_at_most_100_links() {
    let w = World::new();
    let a = w.task();
    for i in 0..100 {
        let (_, call) = link(
            &w,
            &a,
            LinkKind::RelatesTo,
            LinkTarget::Doc {
                project_id: PROJECT.into(),
                doc_key: format!("docs/n{i}.md"),
            },
        );
        w.ok(&agent(), call);
    }
    let (_, call) = link(
        &w,
        &a,
        LinkKind::RelatesTo,
        LinkTarget::Doc {
            project_id: PROJECT.into(),
            doc_key: "docs/one-more.md".into(),
        },
    );
    assert_eq!(w.code(&agent(), call), ErrorCode::InvalidRequest);
}

#[test]
fn delete_makes_a_tombstone_that_links_read_as_deleted() {
    let w = World::new();
    let a = w.task();
    let b = w.task();
    let (_, call) = link(&w, &a, LinkKind::RelatesTo, to_item(&b));
    w.ok(&agent(), call);
    w.ok(&app(), w.delete(&b));
    let item = w.item(&b);
    assert!(item.deleted);
    assert_eq!(item.number, Some(2), "the number stays");
    assert_eq!(w.last_event(&b).change, WorkEventKind::Deleted);
    assert_eq!(w.code(&app(), w.delete(&b)), ErrorCode::InvalidRequest);
    // Linking to it now is refused; a's link reads "deleted".
    let (_, call) = link(&w, &w.task(), LinkKind::RelatesTo, to_item(&b));
    assert_eq!(w.code(&agent(), call), ErrorCode::InvalidRequest);
    let store: &MemStore = &w.store;
    let org = w.store.org.lock().unwrap().clone();
    let full = read_item(&org, store, &w.item(&a), w.now()).unwrap();
    assert!(full.links[0].target_deleted);
    let full_b = read_item(&org, store, &w.item(&b), w.now()).unwrap();
    assert_eq!(full_b.backlinks.len(), 1);
}

#[test]
fn recipients() {
    let w = World::new();
    for (who, ok) in [
        ("ada", true),
        ("pau", true),
        ("pat", true),
        (OWNER, true),
        ("off", false),
        ("nobody", false),
    ] {
        let mut p = w.create_params(PROJECT);
        p.assignee_id = Some(who.into());
        let out = w.call(&app(), WorkCall::TaskCreate(p));
        match (ok, out.answer()) {
            (true, Ok(_)) => {}
            (false, Err(e)) => assert_eq!(e.code, ErrorCode::UnknownRecipient, "{who}"),
            (_, other) => panic!("{who}: {other:?}"),
        }
    }
    // A removed person is gone from the organization.
    w.store.org_mut(|o| o.members.remove("pat"));
    let id = w.task();
    let call = WorkCall::TaskAssign(TaskAssignParams {
        change_id: w.id(),
        item_id: id.clone(),
        expected_version: 1,
        assignee_id: Some("pat".into()),
    });
    assert_eq!(w.code(&app(), call), ErrorCode::UnknownRecipient);
    // Unassigning needs no recipient.
    let call = WorkCall::TaskAssign(TaskAssignParams {
        change_id: w.id(),
        item_id: id,
        expected_version: 1,
        assignee_id: None,
    });
    w.ok(&app(), call);
}

#[test]
fn the_actor_is_always_the_bound_member() {
    let w = World::new();
    let p = w.create_params(PROJECT);
    let id = p.item_id.clone();
    w.ok(&Caller::agent("riley"), WorkCall::TaskCreate(p));
    assert_eq!(w.item(&id).created_by, "riley");
    w.set(&id, ItemStatus::InProgress);
    assert_eq!(w.last_event(&id).actor_id.as_deref(), Some(OWNER));
    // No params field can name an actor: the params don't accept one.
    let with_actor = serde_json::json!({
        "change_id": w.id(), "item_id": w.id(), "project_id": PROJECT, "title": "x",
        "created_by": "owner1", "actor": "owner1"
    });
    let p: TaskCreateParams = serde_json::from_value(with_actor).unwrap();
    let id = p.item_id.clone();
    w.ok(&agent(), WorkCall::TaskCreate(p));
    assert_eq!(w.item(&id).created_by, "ada");
}

#[test]
fn create_refusals() {
    let w = World::new();
    type Edit = Box<dyn Fn(&mut TaskCreateParams)>;
    let cases: Vec<(Edit, ErrorCode)> = vec![
        (
            Box::new(|p| p.project_id = "nope".into()),
            ErrorCode::NotFound,
        ),
        (
            Box::new(|p| p.project_id = ARCHIVED.into()),
            ErrorCode::InvalidRequest,
        ),
        (Box::new(|p| p.title = "".into()), ErrorCode::InvalidRequest),
        (
            Box::new(|p| p.title = "x".repeat(121)),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.title = "a\nb".into()),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.item_id = "t1".into()),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.team_id = Some("team-z".into())),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.feature_id = Some("f".into())),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| {
                p.fixes = vec![NewFixLink {
                    link_id: "l".into(),
                    bug_id: "b".into(),
                }]
            }),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.priority = Some(Priority::Unknown)),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.description = Some("x".repeat(WORK_DESCRIPTION_MAX_CHARS + 1))),
            ErrorCode::InvalidRequest,
        ),
        (
            Box::new(|p| p.assignee_id = Some("off".into())),
            ErrorCode::UnknownRecipient,
        ),
    ];
    for (i, (f, code)) in cases.into_iter().enumerate() {
        let mut p = w.create_params(PROJECT);
        f(&mut p);
        assert_eq!(w.code(&app(), WorkCall::TaskCreate(p)), code, "case {i}");
    }
    // A taken id, and a step id given twice.
    let id = w.task();
    let mut p = w.create_params(PROJECT);
    p.item_id = id;
    assert_eq!(
        w.code(&app(), WorkCall::TaskCreate(p)),
        ErrorCode::AlreadyExists
    );
    let mut p = w.create_params(PROJECT);
    let s = w.id();
    p.checklist = vec![
        NewStep {
            step_id: s.clone(),
            text: "a".into(),
            required: false,
        },
        NewStep {
            step_id: s,
            text: "b".into(),
            required: false,
        },
    ];
    assert_eq!(
        w.code(&app(), WorkCall::TaskCreate(p)),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn events_are_never_dated_before_the_previous_one() {
    let w = World::new();
    let id = w.task();
    w.clock.set(T0 - HOUR);
    // Its change id must not look expired or ahead: date it now.
    w.set(&id, ItemStatus::InProgress);
    let events = w.events(&id);
    assert_eq!(events[1].at, events[0].at);
}

#[test]
fn deleting_an_item_ends_its_waiting_spans() {
    let w = World::new();
    let id = w.task();
    let q = question(&w, &id, OWNER);
    let q_id = q.comment_id.clone();
    w.ok(&agent(), WorkCall::Comment(q));
    w.ok(&app(), w.delete(&id));
    let events = w.events(&id);
    let n = events.len();
    assert_eq!(events[n - 2].change, WorkEventKind::Deleted);
    assert_eq!(events[n - 1].change, ended(&q_id));
    assert!(w.item(&id).waiting.is_empty());
}

#[test]
fn a_connection_that_isnt_the_apps_acts_only_as_an_agent() {
    let w = World::new();
    for who in [OWNER, "pat", "nobody"] {
        let p = w.create_params(PROJECT);
        assert_eq!(
            w.code(&Caller::agent(who), WorkCall::TaskCreate(p)),
            ErrorCode::NotAuthorized,
            "{who}"
        );
    }
    // The app may act as the owner or a person.
    w.ok(
        &Caller::app("pat"),
        WorkCall::TaskCreate(w.create_params(PROJECT)),
    );
}

#[test]
fn writers_never_send_unknown() {
    let w = World::new();
    let a = w.task();
    let b = w.task();
    let (_, call) = link(&w, &a, LinkKind::Unknown, to_item(&b));
    assert_eq!(w.code(&agent(), call), ErrorCode::InvalidRequest);
    let mut p = w.status(&a, ItemStatus::Done);
    p.resolution = Some(Resolution::Unknown);
    assert_eq!(
        w.code(&agent(), WorkCall::TaskSetStatus(p)),
        ErrorCode::InvalidRequest
    );
}
