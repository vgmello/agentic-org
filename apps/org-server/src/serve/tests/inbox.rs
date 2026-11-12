use super::*;
use crate::inbox::{Clock, Hq, Query, Source, SourceFailure, SourceRead};
use org_mailbox::testkit::fixture::{ADA_ID, OWNER_ID, PAT_ID, POSITION};
use org_proto::inbox::{InboxCounts, InboxItem, InboxList, InboxListParams};
use serde_json::{json, Value};

#[path = "inbox_hint.rs"]
mod hint;

#[path = "inbox_acceptance.rs"]
mod acceptance;

struct Rows {
    name: &'static str,
    items: Vec<InboxItem>,
    fail: AtomicBool,
    reads: AtomicUsize,
}
impl Source for Rows {
    fn name(&self) -> &'static str {
        self.name
    }
    fn read(&self, query: &Query<'_>) -> Result<SourceRead, SourceFailure> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            return Err(SourceFailure);
        }
        let mut items: Vec<_> = self
            .items
            .iter()
            .filter(|item| {
                item.state == query.state && query.after.is_none_or(|after| after.admits(item))
            })
            .cloned()
            .collect();
        items.sort_by_key(|item| {
            (
                std::cmp::Reverse(item.created_at_ms),
                item.reference.clone(),
            )
        });
        items.truncate(query.limit);
        Ok(SourceRead {
            items,
            counts: InboxCounts {
                problems: self.items.len() as u64,
                total: self.items.len() as u64,
                ..InboxCounts::default()
            },
        })
    }
}
fn rows(count: usize) -> Arc<Rows> {
    Arc::new(Rows {
        name: "fixture",
        items: (0..count)
            .map(|n| {
                serde_json::from_value(json!({
        "ref":format!("row{n:04}"),"source":"fixture","kind":"problem","summary":"Place agent",
        "created_at_ms":if n < 3 { Some(1) } else { None },
        "state":"open","clears":"placed","dismissable":false,"actions":[]
    })).unwrap()
            })
            .collect(),
        fail: AtomicBool::new(false),
        reads: AtomicUsize::new(0),
    })
}
fn hq(source: Arc<Rows>) -> Hq {
    Hq {
        sources: vec![source],
        clock: Clock::new("test-instance".into()),
    }
}
fn request(params: Value) -> Request {
    let mut request = Request::new::<InboxList>(
        1,
        &serde_json::from_value::<InboxListParams>(json!({"state":"open"})).unwrap(),
    )
    .unwrap();
    request.params = params;
    request
}
fn owner() -> Binding {
    Binding::Member {
        id: OWNER_ID.into(),
        with_app_token: true,
    }
}
fn shared() -> Shared {
    let mut ports = ports(Box::new(|| panic!("not an inbox source")));
    ports.submit = Box::new(|_| panic!("inbox must never submit org writes"));
    ports.work = Box::new(|_, _| panic!("inbox must never write work"));
    ports.status = Box::new(|_, _| panic!("inbox must never set status"));
    Shared::new(&test_info(), Path::new("unused.sock"), ports, token())
}

#[test]
fn inbox_only_registered_hq_advertises_and_serves() {
    let mut shared = shared();
    assert!(shared.inbox.is_none());
    assert!(!shared.hello.supports("inbox.list"));
    assert_eq!(
        code_of(answer(&request(json!({"state":"open"})), &shared, &mut owner()).outcome),
        ErrorCode::UnknownMethod
    );
    for role in [OfficeRole::Satellite, OfficeRole::Unknown] {
        shared.hello.office.as_mut().unwrap().office.role = role;
        shared.register_inbox(hq(rows(1)));
        assert!(shared.inbox.is_none());
        assert!(!shared.hello.supports("inbox.list"));
        assert_eq!(
            code_of(answer(&request(json!({"state":"open"})), &shared, &mut owner()).outcome),
            ErrorCode::UnknownMethod
        );
    }
    shared.hello.office.as_mut().unwrap().office.role = OfficeRole::Hq;
    shared.register_inbox(hq(rows(1)));
    assert!(shared.inbox.is_some());
    assert!(shared.hello.supports("inbox.list"));
    assert!(!shared.hello.supports("inbox.changed"));
    assert_eq!(
        answer(&request(json!({"state":"open"})), &shared, &mut owner())
            .into_result::<InboxList>()
            .unwrap()
            .items
            .len(),
        1
    );
}

struct Directory {
    member: Mutex<Option<org_proto::Member>>,
    failed: AtomicBool,
}
impl MemberDirectory for Directory {
    fn resolve_member(
        &self,
        _: &str,
    ) -> Result<Option<(org_proto::Member, org_proto::Position)>, org_mailbox::StoreError> {
        if self.failed.load(Ordering::SeqCst) {
            return Err(org_mailbox::StoreError::new("SECRET lookup cause"));
        }
        Ok(self
            .member
            .lock()
            .unwrap()
            .clone()
            .map(|member| (member, POSITION)))
    }
}

#[test]
fn inbox_live_owner_is_one_canonical_person_check_and_fails_closed_without_source_reads() {
    let mut shared = shared();
    let source = rows(1);
    shared.register_inbox(hq(Arc::clone(&source)));
    let req = request(json!({"state":"open"}));
    for mut denied in [
        Binding::Anonymous,
        Binding::App,
        Binding::Member {
            id: PAT_ID.into(),
            with_app_token: true,
        },
        Binding::Member {
            id: ADA_ID.into(),
            with_app_token: false,
        },
    ] {
        for params in [
            json!({"state":"open"}),
            json!({"state":"bogus"}),
            json!({"state":"open","cursor":"x"}),
        ] {
            assert_eq!(
                code_of(answer(&request(params), &shared, &mut denied).outcome),
                ErrorCode::NotAuthorized,
                "authorization must precede parameter and cursor validation"
            );
        }
    }
    assert_eq!(source.reads.load(Ordering::SeqCst), 0);
    assert_eq!(
        answer(&req, &shared, &mut owner())
            .into_result::<InboxList>()
            .unwrap()
            .items
            .len(),
        1
    );
    let canonical = org_mailbox::testkit::fixture::members()
        .into_iter()
        .find(|member| member.owner)
        .unwrap();
    let directory = Arc::new(Directory {
        member: Mutex::new(Some(canonical.clone())),
        failed: AtomicBool::new(false),
    });
    shared.ports.directory = directory.clone();
    assert!(live_owner(&shared, &owner()).unwrap());
    for mode in 0..5 {
        let mut member = canonical.clone();
        match mode {
            0 => member.owner = false,
            1 => member.id = PAT_ID.into(),
            2 => member.kind = MemberKind::Agent,
            3 => member.kind = MemberKind::Unknown,
            _ => {}
        }
        *directory.member.lock().unwrap() = (mode != 4).then_some(member);
        assert!(!live_owner(&shared, &owner()).unwrap());
        assert_eq!(
            code_of(answer(&req, &shared, &mut owner()).outcome),
            ErrorCode::NotAuthorized
        );
    }
    *directory.member.lock().unwrap() = Some(canonical);
    directory.failed.store(true, Ordering::SeqCst);
    assert!(live_owner(&shared, &owner()).is_err());
    let response = answer(&req, &shared, &mut owner());
    assert_eq!(code_of(response.outcome.clone()), ErrorCode::NotAuthorized);
    assert!(!serde_json::to_string(&response).unwrap().contains("SECRET"));
    assert_eq!(source.reads.load(Ordering::SeqCst), 1);
    directory.failed.store(false, Ordering::SeqCst);
    assert_eq!(
        answer(&req, &shared, &mut owner())
            .into_result::<InboxList>()
            .unwrap()
            .items
            .len(),
        1
    );
}

#[test]
fn inbox_limits_unknown_filters_and_bad_cursors_are_refused_before_reading_sources() {
    let mut shared = shared();
    let source = rows(110);
    shared.register_inbox(hq(Arc::clone(&source)));
    for params in [
        json!({}),
        json!({"state":"future"}),
        json!({"state":"open","kinds":[]}),
        json!({"state":"handled","kinds":[]}),
        json!({"state":"open","kinds":["future"]}),
        json!({"state":"open","kinds":["problem","future"]}),
        json!({"state":"open","limit":0}),
        json!({"state":"open","limit":101}),
        json!({"state":"open","limit":4294967296u64}),
        json!({"state":"open","limit":"50"}),
        json!({"state":"open","team":""}),
        json!({"state":"open","project":"bad/id"}),
    ] {
        assert_eq!(
            code_of(answer(&request(params.clone()), &shared, &mut owner()).outcome),
            ErrorCode::InvalidRequest,
            "{params}"
        );
    }
    for cursor in [
        "",
        "v1",
        "v1:-",
        "v2:-:row",
        "v1:-:",
        "v1:not-a-number:row",
        "v1:18446744073709551616:row",
        "v1:+1:row",
        "v1:01:row",
    ] {
        assert_eq!(
            code_of(
                answer(
                    &request(json!({"state":"open","cursor":cursor})),
                    &shared,
                    &mut owner()
                )
                .outcome
            ),
            ErrorCode::InvalidRequest,
            "{cursor}"
        );
    }
    assert_eq!(source.reads.load(Ordering::SeqCst), 0);
    for (limit, expected) in [(None, 50), (Some(1), 1), (Some(100), 100)] {
        let result = answer(
            &request(json!({"state":"open","limit":limit})),
            &shared,
            &mut owner(),
        )
        .into_result::<InboxList>()
        .unwrap();
        assert_eq!(result.items.len(), expected);
        assert!(result.next_cursor.is_some());
        assert_eq!(result.counts.total, 110);
        assert_eq!(result.as_of.epoch, "test-instance");
    }
    let handled = answer(
        &request(json!({"state":"handled","project":"valid","team":"valid","kinds":["problem"]})),
        &shared,
        &mut owner(),
    )
    .into_result::<InboxList>()
    .expect("a known nonempty kinds filter remains valid");
    assert!(handled.items.is_empty() && handled.next_cursor.is_none());
    assert_eq!(handled.counts.total, 110);
}

#[test]
fn inbox_pages_cross_ties_and_unknown_times_and_last_cursor_is_absent() {
    let mut shared = shared();
    shared.register_inbox(hq(rows(7)));
    let mut cursor = None;
    let mut references = vec![];
    for _ in 0..4 {
        let response = answer(
            &request(json!({"state":"open","limit":2,"cursor":cursor})),
            &shared,
            &mut owner(),
        );
        let result = response.into_result::<InboxList>().unwrap();
        references.extend(result.items.iter().map(|item| item.reference.clone()));
        cursor = result.next_cursor.clone();
        if cursor.is_none() {
            assert!(!serde_json::to_value(result)
                .unwrap()
                .as_object()
                .unwrap()
                .contains_key("next_cursor"));
            break;
        }
    }
    assert_eq!(
        references,
        (0..7).map(|n| format!("row{n:04}")).collect::<Vec<_>>()
    );
    assert!(cursor.is_none());
    shared.inbox = Some(hq(rows(2)));
    let exact = answer(
        &request(json!({"state":"open","limit":2})),
        &shared,
        &mut owner(),
    )
    .into_result::<InboxList>()
    .unwrap();
    assert_eq!(exact.items.len(), 2);
    assert!(
        exact.next_cursor.is_none(),
        "exactly full final page has no successor"
    );
    // Both timestamp endpoints are valid, and refs are opaque even when they contain ':'.
    for time in [0, u64::MAX] {
        let result = answer(
            &request(json!({"state":"open","cursor":format!("v1:{time}:arbitrary:reference")})),
            &shared,
            &mut owner(),
        );
        assert!(result.into_result::<InboxList>().is_ok());
    }
}

#[test]
fn inbox_degraded_cursor_survives_then_fresh_list_recovers_earlier_source_rows() {
    let mut shared = shared();
    let later = rows(6);
    let early = Arc::new(Rows {
        name: "recovered",
        items: vec![serde_json::from_value(
            json!({"ref":"a-first","source":"recovered","kind":"problem","summary":"Earlier",
        "created_at_ms":2,"state":"open","clears":"placed","dismissable":false,"actions":[]}),
        )
        .unwrap()],
        fail: AtomicBool::new(true),
        reads: AtomicUsize::new(0),
    });
    shared.register_inbox(Hq {
        sources: vec![later, early.clone()],
        clock: Clock::new("test".into()),
    });
    let first = answer(
        &request(json!({"state":"open","limit":2})),
        &shared,
        &mut owner(),
    )
    .into_result::<InboxList>()
    .unwrap();
    assert_eq!(first.degraded, ["recovered"]);
    assert_eq!(first.counts.total, 6);
    assert!(first.next_cursor.is_some());
    early.fail.store(false, Ordering::SeqCst);
    let second = answer(
        &request(json!({"state":"open","limit":2,"cursor":first.next_cursor})),
        &shared,
        &mut owner(),
    )
    .into_result::<InboxList>()
    .unwrap();
    assert!(second.degraded.is_empty());
    assert_eq!(second.counts.total, 7);
    assert!(!second.items.iter().any(|item| item.reference == "a-first"));
    let fresh = answer(
        &request(json!({"state":"open","limit":2})),
        &shared,
        &mut owner(),
    )
    .into_result::<InboxList>()
    .unwrap();
    assert_eq!(fresh.items[0].reference, "a-first");
    assert_eq!(fresh.counts.total, 7);
}
