//! The SQLite FTS5 side of search against the engine-independent rules: the bundled SQLite
//! has FTS5; its tokenizer reads every word as [`org_office::search::text`] does; its `bm25()`
//! equals [`org_office::search::rank::bm25`]; no query text ever makes a `MATCH` fail; and a search
//! over hundreds of documents answers in well under 5 ms.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use org_proto::search::SearchEntityType;
use rusqlite::{params, Connection};

use super::{fts5, messages, shared};
use crate::messaging::create_messaging_schema;
use crate::schema::Schema;
use org_office::search::plan::{Filters, SearchPlan};
use org_office::search::ports::MessageScope;
use org_office::search::rank::{analyze_corpus, bm25};
use org_office::search::records::{DocMeta, SearchDoc};
use org_office::search::text::{query_terms, tokens};

const ORG: &str = "0195a000-0000-7000-8000-0000000000a1";

fn index() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(&fts5::create_index("t")).unwrap();
    db
}

fn insert(db: &Connection, row: i64, fields: &[String; 5]) {
    let [a, b, c, d, e] = fields;
    db.execute(
        "INSERT INTO t (rowid, f_ref, f_title, f_tags, f_body, f_extra) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![row, a, b, c, d, e],
    )
    .unwrap();
}

const SAMPLES: [&str; 8] = [
    "Deploying the T-HQ12 fix: src/main.rs, Café @alpha",
    "naïve RÉSUMÉ Ångström ação øre æble łódź đak ßtraße",
    "caresses ponies ties caress cats feed agreed plastered bled motoring sing conflated",
    "troubled sized hopping tanned falling hissing fizzed failing filing happy sky",
    "relational conditional valenci digitizer generalization operator hopefulness formality",
    "sensitivity sensibility electrical goodness adjustable replacement adoption communism",
    "activate effective probate controll roll deployment deploying at a an x-y_z.w/v@u",
    "日本語 emoji, numbers 12345 v1.2.3 user@example.com ../../etc/passwd",
];

#[test]
fn fts5_is_built_in_and_reads_every_word_as_the_rust_tokenizer_does() {
    let db = index();
    db.execute_batch("CREATE VIRTUAL TABLE v USING fts5vocab(t, instance);")
        .unwrap();
    for (row, text) in SAMPLES.iter().enumerate() {
        let fields = ["", "", "", text, ""].map(str::to_owned);
        insert(&db, row as i64 + 1, &fields);
        let mut statement = db
            .prepare("SELECT term FROM v WHERE doc = ?1 ORDER BY offset")
            .unwrap();
        let fts5: Vec<String> = statement
            .query_map([row as i64 + 1], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let rust: Vec<String> = tokens(text).map(|t| t.term).collect();
        assert_eq!(rust, fts5, "{text}");
    }
}

fn doc(fields: [&str; 5]) -> SearchDoc {
    SearchDoc {
        entity_type: SearchEntityType::Task,
        entity_id: String::new(),
        meta: DocMeta::default(),
        fields: fields.map(str::to_owned),
    }
}

#[test]
fn fts5_bm25_with_the_field_weights_is_the_rust_score() {
    let corpus = vec![
        doc([
            "T-HQ1",
            "Rotate signing keys",
            "security ops",
            "Rotate the keys weekly; rotation runs at night",
            "comment: keys rotated",
        ]),
        doc([
            "T-HQ2",
            "Signing service outage",
            "incident",
            "The signing service failed",
            "checklist: page on-call",
        ]),
        doc(["P-3", "Key Management", "", "", ""]),
        doc([
            "alpha",
            "Alpha Agent",
            "engineer agent",
            "Focus: key rotation and signing",
            "",
        ]),
        doc(["", "", "", "short", ""]),
    ];
    let db = index();
    for (row, d) in corpus.iter().enumerate() {
        insert(&db, row as i64 + 1, &d.fields);
    }
    for q in [
        "rotate",
        "signing keys",
        "key",
        "t-hq",
        "rotation night",
        "signing service outage",
    ] {
        let terms = query_terms(q);
        let (stats, matches) = analyze_corpus(&corpus, &terms);
        let mut statement = db
            .prepare("SELECT rowid, -bm25(t, 10.0, 6.0, 4.0, 2.0, 1.0) FROM t WHERE t MATCH ?1")
            .unwrap();
        let rows: Vec<(i64, f64)> = statement
            .query_map([fts5::match_all(&terms)], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let expected: Vec<usize> = (0..corpus.len())
            .filter(|i| matches[*i].matches_all())
            .collect();
        assert_eq!(rows.len(), expected.len(), "{q}");
        for (row, score) in rows {
            let i = usize::try_from(row - 1).unwrap();
            assert!(matches[i].matches_all(), "{q}: row {row}");
            let rust = bm25(&stats, &matches[i]);
            assert!(
                (rust - score).abs() < 1e-12,
                "{q}: row {row} {rust} != {score}"
            );
        }
    }
}

#[test]
fn no_query_text_ever_makes_a_match_fail_or_change_shape() {
    let db = index();
    insert(
        &db,
        1,
        &["T-HQ1", "near not and or", "", "foo bar baz 1", "col val"].map(str::to_owned),
    );
    let hostile = [
        "foo\" OR 1=1",
        "foo\" OR \"1\"=\"1",
        "NEAR/5",
        "NEAR(foo bar, 2)",
        "*",
        "---",
        "()",
        "col:val",
        "f_body:foo",
        "{f_title f_body}: foo",
        "foo AND",
        "NOT foo",
        "^foo",
        "foo*bar",
        "\"\"\"",
        "'; DROP TABLE t; --",
        "a\u{0}b",
        "🙂🙂",
        "x OR y OR z",
        "- - -",
        "foo + bar",
    ];
    for q in hostile {
        let terms = query_terms(q);
        if terms.is_empty() {
            continue;
        }
        let matched = fts5::match_all(&terms);
        let count: i64 = db
            .query_row("SELECT count(*) FROM t WHERE t MATCH ?1", [&matched], |r| {
                r.get(0)
            })
            .unwrap_or_else(|e| panic!("{q:?} -> {matched:?}: {e}"));
        assert!(count <= 1, "{q}");
    }
    // An operator is a plain word: `NOT foo` needs both words, unlike FTS5's NOT.
    let terms = query_terms("NOT foo");
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM t WHERE t MATCH ?1",
            [fts5::match_all(&terms)],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    let terms = query_terms("NOT missing");
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM t WHERE t MATCH ?1",
            [fts5::match_all(&terms)],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

const SYLLABLES: [&str; 16] = [
    "ka", "lo", "mi", "nu", "pe", "ra", "si", "to", "va", "xe", "zu", "bo", "de", "fi", "gu", "ha",
];

/// One of 4,096 three-syllable words, the low ones far more frequent (as in real text).
fn word(seed: usize) -> String {
    let skewed = (seed % 4096) * (seed % 4096) / 4096;
    (0..3)
        .map(|k| SYLLABLES[(skewed >> (4 * k)) % 16])
        .collect()
}

fn sentence(seed: usize, words: usize) -> String {
    (0..words)
        .map(|i| word(seed.wrapping_mul(2_654_435_761).wrapping_add(i * 40_503)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `n` documents of everyday text, every `every`-th of them also about `topic`.
fn seeded(
    n: usize,
    kind: SearchEntityType,
    conversation: Option<&str>,
    (topic, every): (&str, usize),
) -> Vec<SearchDoc> {
    (0..n)
        .map(|i| SearchDoc {
            entity_type: kind,
            entity_id: format!("0195a000-0000-7000-8000-{i:012x}"),
            meta: DocMeta {
                reference: Some(format!("T-HQ{i}")),
                title: Some(sentence(i, 5)),
                status: Some("todo".into()),
                conversation_id: conversation.map(str::to_owned),
                updated_at: format!("2026-11-{:02}T00:00:00.000Z", 1 + i % 28),
                ..DocMeta::default()
            },
            fields: [
                format!("T-HQ{i}"),
                sentence(i, 5),
                sentence(i + 3, 2),
                if i % every == 0 {
                    format!("{} {topic} {}", sentence(i + 11, 20), sentence(i + 13, 20))
                } else {
                    sentence(i + 11, 40)
                },
                sentence(i + 17, 12),
            ],
        })
        .collect()
}

fn plan(q: &str, types: &[SearchEntityType]) -> SearchPlan {
    SearchPlan {
        terms: query_terms(q),
        types: types.iter().copied().collect::<BTreeSet<_>>(),
        filters: Filters::default(),
        after: None,
        limit: 20,
        count_only: false,
    }
}

fn best(runs: Vec<Duration>) -> Duration {
    runs.into_iter().min().unwrap_or_default()
}

/// The regression bound of `TASK-91`: a search over hundreds of documents of everyday text
/// answers in under 5 ms, ranked by FTS5 (shared knowledge) and in Rust over the visible
/// corpus (messages), even in an unoptimized test build.
#[test]
fn a_search_over_hundreds_of_documents_answers_within_five_milliseconds() {
    let dir = tempfile::tempdir().unwrap();
    let db = Connection::open(dir.path().join("office.db")).unwrap();
    db.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
        .unwrap();
    Schema::current().create(&db).unwrap();
    let docs = seeded(
        600,
        SearchEntityType::Task,
        None,
        ("signing key rotation", 20),
    );
    db.execute_batch("BEGIN").unwrap();
    for d in &docs {
        shared::put(&db, ORG, "task", &d.entity_id, Some(d)).unwrap();
    }
    db.execute_batch("COMMIT").unwrap();
    let msg = Connection::open(dir.path().join("messaging.db")).unwrap();
    msg.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
        .unwrap();
    create_messaging_schema(&msg).unwrap();
    let conversation = "0195a000-0000-7000-8000-0000000000c1";
    let topic = ("cache latency regression", 15);
    msg.execute_batch("BEGIN").unwrap();
    for d in seeded(400, SearchEntityType::Message, Some(conversation), topic) {
        messages::put(&msg, ORG, &d.entity_id, Some(&d)).unwrap();
    }
    msg.execute_batch("COMMIT").unwrap();
    let scope = MessageScope {
        visible: vec![conversation.to_owned()],
        within: vec![conversation.to_owned()],
    };
    let shared_plan = plan("rotation signing", &[SearchEntityType::Task]);
    let message_plan = plan("latency cache", &[SearchEntityType::Message]);
    let first = shared::search(&db, ORG, &shared_plan).unwrap();
    assert_eq!((first.total, first.hits.len()), (30, 21));
    let first = messages::search(&msg, ORG, &message_plan, &scope).unwrap();
    assert_eq!((first.total, first.hits.len()), (27, 21));
    let mut shared_runs = Vec::new();
    let mut message_runs = Vec::new();
    for _ in 0..25 {
        let started = Instant::now();
        let page = shared::search(&db, ORG, &shared_plan).unwrap();
        shared_runs.push(started.elapsed());
        assert_eq!(page.hits.len(), 21);
        let started = Instant::now();
        let page = messages::search(&msg, ORG, &message_plan, &scope).unwrap();
        message_runs.push(started.elapsed());
        assert_eq!(page.hits.len(), 21);
    }
    // The best of 25 runs (matching `tasks::tests`): parallel SQLite tests on a busy runner
    // preempt some iterations, never the fastest of them.
    let (shared_best, message_best) = (best(shared_runs), best(message_runs));
    assert!(
        shared_best < Duration::from_millis(5),
        "shared search took {shared_best:?}"
    );
    assert!(
        message_best < Duration::from_millis(5),
        "message search took {message_best:?}"
    );
}

#[test]
fn the_index_ddl_lives_in_the_adapter_never_in_a_contract() {
    for contract in [
        include_str!("../../../../docs/design/organization-data/schema.sql"),
        include_str!("../../../../docs/design/organization-data/messaging-schema.sql"),
        include_str!("../../../../docs/design/organization-data/private-notes-schema.sql"),
    ] {
        assert!(!contract.to_ascii_uppercase().contains("VIRTUAL TABLE"));
        assert!(!contract.contains("fts5"));
    }
    assert!(shared::DDL.contains("CREATE VIRTUAL TABLE search_fts USING fts5"));
    assert!(messages::DDL.contains("CREATE VIRTUAL TABLE message_search_fts USING fts5"));
}
