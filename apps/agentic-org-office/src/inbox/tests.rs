use super::*;
use org_proto::inbox::InboxClears;
use std::sync::Mutex;

fn item(reference: &str) -> InboxItem {
    InboxItem {
        reference: reference.into(),
        source: "org".into(),
        kind: InboxKind::Problem,
        member: None,
        project: None,
        team: None,
        summary: "Place agent".into(),
        context: None,
        recommendation: None,
        options: None,
        created_at_ms: None,
        expires_at_ms: None,
        state: InboxState::Open,
        handled_at_ms: None,
        outcome: None,
        clears: InboxClears::Placed,
        dismissable: false,
        actions: vec![],
    }
}

type QueryRecord = (
    usize,
    Option<String>,
    Option<String>,
    Option<Keyset>,
    InboxState,
    Option<Vec<InboxKind>>,
);

struct Fake {
    name: &'static str,
    counts: InboxCounts,
    fail: bool,
    seen: Mutex<Vec<QueryRecord>>,
}
impl Fake {
    fn new(name: &'static str, counts: InboxCounts) -> Self {
        Self {
            name,
            counts,
            fail: false,
            seen: Mutex::default(),
        }
    }
}
impl Source for Fake {
    fn name(&self) -> &'static str {
        self.name
    }
    fn read(&self, query: &Query<'_>) -> Result<SourceRead, SourceFailure> {
        self.seen.lock().unwrap().push((
            query.limit,
            query.project.map(str::to_owned),
            query.team.map(str::to_owned),
            query.after.cloned(),
            query.state,
            query.kinds.map(<[_]>::to_vec),
        ));
        if self.fail {
            return Err(SourceFailure);
        }
        Ok(SourceRead {
            items: if query.limit == 0 {
                vec![]
            } else {
                vec![item(self.name)]
            },
            counts: self.counts.clone(),
        })
    }
}

#[test]
fn list_and_recount_use_the_same_exact_unfiltered_counts_past_the_item_cap() {
    let a = Fake::new(
        "org",
        InboxCounts {
            approvals: 501,
            questions: 2,
            decisions: 3,
            problems: 4,
            total: 511,
        },
    );
    let b = Fake::new(
        "work",
        InboxCounts {
            approvals: 1,
            questions: 4,
            decisions: 5,
            problems: 6,
            total: 18,
        },
    );
    let sources: [&dyn Source; 2] = [&a, &b];
    let after = Keyset {
        created_at_ms: Some(42),
        reference: "tie".into(),
    };
    let query = Query {
        state: InboxState::Handled,
        kinds: Some(&[InboxKind::Question]),
        project: Some("P"),
        team: Some("T"),
        after: Some(&after),
        limit: 100,
    };
    let list = read(&Clock::new("test".into()), &sources, &query).1;
    assert_eq!(
        list.items
            .iter()
            .map(|x| x.reference.as_str())
            .collect::<Vec<_>>(),
        ["org", "work"]
    );
    assert!(list.totals.degraded.is_empty());
    let totals = recount(&sources);
    assert_eq!(totals.counts, list.totals.counts);
    assert_eq!(
        totals.counts,
        InboxCounts {
            approvals: 502,
            questions: 6,
            decisions: 8,
            problems: 10,
            total: 529
        }
    );
    for source in [&a, &b] {
        let seen = source.seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(
            seen[0],
            (
                100,
                Some("P".into()),
                Some("T".into()),
                Some(after.clone()),
                InboxState::Handled,
                Some(vec![InboxKind::Question])
            )
        );
        assert_eq!(seen[1], (0, None, None, None, InboxState::Open, None));
    }
}

#[test]
fn failure_omits_that_sources_items_and_counts_and_marks_both_paths_degraded() {
    let good = Fake::new(
        "org",
        InboxCounts {
            problems: 2,
            total: 2,
            ..InboxCounts::default()
        },
    );
    let mut bad = Fake::new(
        "messages",
        InboxCounts {
            questions: 500,
            total: 500,
            ..InboxCounts::default()
        },
    );
    bad.fail = true;
    let query = Query {
        state: InboxState::Open,
        kinds: None,
        project: None,
        team: None,
        after: None,
        limit: 100,
    };
    let result = read(&Clock::new("test".into()), &[&good, &bad], &query).1;
    assert_eq!(
        result
            .items
            .iter()
            .map(|x| x.reference.as_str())
            .collect::<Vec<_>>(),
        ["org"]
    );
    assert_eq!(result.totals.counts, good.counts);
    assert_eq!(result.totals.degraded, ["messages"]);
    let hint = recount(&[&good, &bad]);
    assert_eq!(hint.counts, good.counts);
    assert_eq!(hint.degraded, ["messages"]);
}

#[test]
fn each_count_overflow_degrades_the_whole_source_without_partial_items_or_totals() {
    for field in 0..5 {
        let mut a = InboxCounts::default();
        let mut b = InboxCounts::default();
        let a_fields = [
            &mut a.approvals,
            &mut a.questions,
            &mut a.decisions,
            &mut a.problems,
            &mut a.total,
        ];
        let b_fields = [
            &mut b.approvals,
            &mut b.questions,
            &mut b.decisions,
            &mut b.problems,
            &mut b.total,
        ];
        *a_fields[field] = u64::MAX;
        *b_fields[field] = 1;
        let good = Fake::new("org", a.clone());
        let bad = Fake::new("work", b);
        let query = Query {
            state: InboxState::Open,
            kinds: None,
            project: None,
            team: None,
            after: None,
            limit: 1,
        };
        let result = read(&Clock::new("test".into()), &[&good, &bad], &query).1;
        assert_eq!(result.items.len(), 1, "field{field}");
        assert_eq!(result.totals.counts, a, "field{field}");
        assert_eq!(result.totals.degraded, ["work"], "field{field}");
    }
}

#[test]
fn clock_samples_are_immutable_monotonic_and_change_epoch_on_a_new_instance() {
    let clock = Clock::new("instance-one".into());
    let before = clock.as_of();
    clock.changed();
    clock.changed();
    assert_eq!(before.seq, 0);
    assert_eq!(clock.as_of().seq, 2);
    assert_eq!(clock.as_of().epoch, before.epoch);
    assert_ne!(
        Clock::new("instance-two".into()).as_of().epoch,
        before.epoch
    );
    clock.seq.store(u64::MAX, Ordering::Release);
    assert!(std::panic::catch_unwind(|| clock.changed()).is_err());
    assert_eq!(clock.as_of().seq, u64::MAX);
}

#[test]
fn as_of_is_sampled_before_the_first_source_even_when_that_read_commits() {
    struct Commits<'a>(&'a Clock);
    impl Source for Commits<'_> {
        fn name(&self) -> &'static str {
            "org"
        }
        fn read(&self, _: &Query<'_>) -> Result<SourceRead, SourceFailure> {
            self.0.changed();
            Ok(SourceRead {
                items: vec![],
                counts: InboxCounts::default(),
            })
        }
    }
    let clock = Clock::new("instance".into());
    let source = Commits(&clock);
    let query = Query {
        state: InboxState::Open,
        kinds: None,
        project: None,
        team: None,
        after: None,
        limit: 100,
    };
    let (position, _) = read(&clock, &[&source], &query);
    assert_eq!(position.seq, 0);
    assert_eq!(clock.as_of().seq, 1);
}

#[test]
fn merged_keyset_pages_reach_past_each_source_cap_with_ties_and_unknown_times_last() {
    struct Paged {
        name: &'static str,
        items: Vec<InboxItem>,
    }
    impl Source for Paged {
        fn name(&self) -> &'static str {
            self.name
        }
        fn read(&self, query: &Query<'_>) -> Result<SourceRead, SourceFailure> {
            let mut items: Vec<_> = self
                .items
                .iter()
                .filter(|item| query.after.is_none_or(|after| after.admits(item)))
                .cloned()
                .collect();
            items.sort_by(|a, b| {
                b.created_at_ms
                    .cmp(&a.created_at_ms)
                    .then_with(|| a.reference.cmp(&b.reference))
            });
            items.truncate(query.limit.min(500));
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
    let make = |name: &'static str| Paged {
        name,
        items: (0..603)
            .rev()
            .map(|i| {
                let mut row = item(&format!("{name}:{i:04}"));
                row.created_at_ms = if i % 3 == 0 {
                    None
                } else {
                    Some((i % 5) as u64)
                };
                row
            })
            .collect(),
    };
    let a = make("org");
    let b = make("work");
    let mut expected: Vec<_> = a.items.iter().chain(&b.items).cloned().collect();
    expected.sort_by(|a, b| {
        b.created_at_ms
            .cmp(&a.created_at_ms)
            .then_with(|| a.reference.cmp(&b.reference))
    });
    let clock = Clock::new("instance".into());
    let mut after = None;
    let mut actual = vec![];
    for _ in 0..20 {
        let query = Query {
            state: InboxState::Open,
            kinds: None,
            project: None,
            team: None,
            after: after.as_ref(),
            limit: 100,
        };
        let (_, page) = read(&clock, &[&b, &a], &query);
        assert_eq!(page.totals.counts.total, 1206);
        assert!(page.totals.degraded.is_empty());
        assert!(page.items.len() <= 100);
        if page.items.is_empty() {
            break;
        }
        after = page.items.last().map(Keyset::of);
        actual.extend(page.items);
    }
    assert_eq!(
        actual.iter().map(|i| &i.reference).collect::<Vec<_>>(),
        expected.iter().map(|i| &i.reference).collect::<Vec<_>>()
    );
    assert_eq!(actual.len(), 1206);
    assert!(actual.first().unwrap().created_at_ms.is_some());
    assert_eq!(actual.last().unwrap().created_at_ms, None);
    assert_eq!(recount(&[&a, &b]).counts.total, 1206);
}

#[test]
fn collector_bounds_source_text_before_returning_it() {
    struct Long;
    impl Source for Long {
        fn name(&self) -> &'static str {
            "org"
        }
        fn read(&self, _: &Query<'_>) -> Result<SourceRead, SourceFailure> {
            let mut row = item("org:long");
            row.summary = "é".repeat(512);
            row.context = Some("\u{1}".repeat(2048));
            row.recommendation = Some("\"".repeat(1024));
            row.options = Some(vec!["\\".repeat(128); 9]);
            Ok(SourceRead {
                items: vec![row],
                counts: InboxCounts {
                    problems: 1,
                    total: 1,
                    ..InboxCounts::default()
                },
            })
        }
    }
    let query = Query {
        state: InboxState::Open,
        kinds: None,
        project: None,
        team: None,
        after: None,
        limit: 1,
    };
    let (_, page) = read(&Clock::new("instance".into()), &[&Long], &query);
    let row = &page.items[0];
    for (text, cap) in [
        (&row.summary, 512),
        (row.context.as_ref().unwrap(), 2048),
        (row.recommendation.as_ref().unwrap(), 1024),
    ] {
        assert!(org_proto::msg::json_string_len(text) <= cap);
        assert!(text.ends_with('…'));
    }
    assert_eq!(row.options.as_ref().unwrap().len(), 8);
    assert!(row
        .options
        .as_ref()
        .unwrap()
        .iter()
        .all(|s| org_proto::msg::json_string_len(s) <= 128 && s.ends_with('…')));
}
