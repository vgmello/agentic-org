use std::collections::BTreeSet;

use org_proto::search::{SearchEntityType, SEARCH_EXCERPT_MAX_CHARS};
use org_proto::ErrorCode;

use super::*;
use crate::search::plan::{Filters, SearchPlan};
use crate::search::records::DocMeta;
use crate::search::text::query_terms;

fn doc(id: &str, updated_at: &str, fields: [&str; 5]) -> SearchDoc {
    SearchDoc {
        entity_type: SearchEntityType::Task,
        entity_id: id.into(),
        meta: DocMeta {
            updated_at: updated_at.into(),
            ..DocMeta::default()
        },
        fields: fields.map(str::to_owned),
    }
}

fn plan(q: &str, limit: usize) -> SearchPlan {
    SearchPlan {
        terms: query_terms(q),
        types: BTreeSet::from([SearchEntityType::Task]),
        filters: Filters::default(),
        after: None,
        limit,
        count_only: false,
        visible_projects: None,
    }
}

/// The corpus of a probe run against SQLite FTS5's own `bm25(f, 10, 6, 4, 2, 1)`: FTS5 gave
/// the first document 1.5310515752376577 for `"deploy"*` and 0.9968210122202397 for
/// `"deployment"*`.
fn probe_corpus() -> Vec<SearchDoc> {
    vec![
        doc(
            "a",
            "",
            [
                "T-HQ1",
                "deploying the app",
                "ops",
                "we deploy every day; deployment is fun",
                "",
            ],
        ),
        doc("b", "", ["T-HQ2", "Café menu", "food", "naïve résumé", ""]),
        doc(
            "c",
            "",
            ["T-HQ3", "runner", "", "running runs", "src/main.rs"],
        ),
        doc("d", "", ["T-HQ4", "near not and", "", "foo bar baz", ""]),
    ]
}

#[test]
fn bm25_is_fts5s_weighted_bm25_to_the_last_bit() {
    let corpus = probe_corpus();
    for (q, expected) in [
        ("deploy", 1.531_051_575_237_657_7),
        ("deploying", 1.531_051_575_237_657_7),
        ("deployment", 0.996_821_012_220_239_7),
    ] {
        let terms = query_terms(q);
        let (stats, matches) = analyze_corpus(&corpus, &terms);
        assert_eq!(stats.docs, 4);
        assert_eq!(stats.tokens, 30);
        assert!(matches[0].matches_all());
        assert!(!matches[1].matches_all());
        let score = bm25(&stats, &matches[0]);
        assert!(
            (score - expected).abs() < 1e-12,
            "{q}: {score} != {expected}"
        );
    }
    // A prefix every document matches has an IDF below zero: FTS5's floor applies.
    let terms = query_terms("t-hq");
    let (stats, matches) = analyze_corpus(&corpus, &terms);
    assert_eq!(stats.term_docs, [4]);
    let tiny = bm25(&stats, &matches[2]);
    assert!(tiny > 0.0 && tiny < 1e-5, "{tiny}");
}

#[test]
fn a_document_the_caller_may_not_see_never_moves_a_score() {
    let mut corpus = probe_corpus();
    let terms = query_terms("deploy");
    let (stats, matches) = analyze_corpus(&corpus, &terms);
    let alone = bm25(&stats, &matches[0]);
    // Another caller's documents are not in this caller's corpus: the same score.
    corpus.push(doc("z", "", ["", "deploy deploy", "", "deploy", ""]));
    let (wider, wider_matches) = analyze_corpus(&corpus, &terms);
    assert_ne!(bm25(&wider, &wider_matches[0]), alone);
    let (narrow, narrow_matches) = analyze_corpus(&corpus[..4], &terms);
    assert_eq!(bm25(&narrow, &narrow_matches[0]), alone);
}

#[test]
fn ties_break_by_later_update_then_smaller_id_and_pages_never_repeat_or_skip() {
    let docs: Vec<SearchDoc> = [
        ("01", "2026-11-01T00:00:00.000Z"),
        ("02", "2026-11-03T00:00:00.000Z"),
        ("03", "2026-11-03T00:00:00.000Z"),
        ("04", "2026-11-02T00:00:00.000Z"),
        ("05", ""),
    ]
    .map(|(id, at)| doc(id, at, ["", "same words", "", "", ""]))
    .into();
    let mut plan = plan("same", 2);
    let stats = analyze_corpus(&docs, &plan.terms).0;
    let mut seen = Vec::new();
    loop {
        let page = scored_page(docs.clone(), &stats, &plan);
        assert_eq!(page.total, 5);
        let (hits, total, next) = merge(vec![page], plan.limit);
        assert_eq!(total, 5);
        seen.extend(hits.iter().map(|h| h.doc.entity_id.clone()));
        let Some(next) = next else { break };
        let parsed = SearchCursor::parse(&next.encode()).unwrap();
        assert_eq!(parsed, next);
        plan.after = Some(parsed);
    }
    assert_eq!(seen, ["02", "03", "04", "01", "05"]);
}

#[test]
fn counting_answers_the_total_without_hits() {
    let docs = probe_corpus();
    let mut plan = plan("deploy", 10);
    plan.count_only = true;
    let stats = analyze_corpus(&docs, &plan.terms).0;
    let page = scored_page(docs.into_iter().take(1), &stats, &plan);
    assert_eq!(page.total, 1);
    assert!(page.hits.is_empty());
}

#[test]
fn a_cursor_that_is_not_a_next_cursor_is_refused() {
    let long = format!("{:016x}~task~~{}", 1.0f64.to_bits(), "a".repeat(300));
    for raw in [
        "",
        "abc",
        "zzzzzzzzzzzzzzzz~task~2026~id",
        &format!("{:016x}~task~2026~id", f64::NAN.to_bits()),
        &format!("{:016x}~task~2026~id", (-1.0f64).to_bits()),
        &format!("{:016x}~unknown_kind~2026~id", 1.0f64.to_bits()),
        &format!("{:016x}~task~2026'--~id", 1.0f64.to_bits()),
        &format!("{:016x}~task~2026~", 1.0f64.to_bits()),
        &format!("{:016x}~task~2026~a b", 1.0f64.to_bits()),
        &long,
    ] {
        let refused = SearchCursor::parse(raw).unwrap_err();
        assert_eq!(refused.code, ErrorCode::InvalidRequest, "{raw:?}");
    }
}

#[test]
fn an_excerpt_is_bounded_in_characters_around_the_match_and_never_splits_one() {
    let terms = query_terms("needle");
    let lead = "日本語の文章🙂 ".repeat(40);
    let tail = " été ação 🙂".repeat(60);
    let body = format!("{lead}the NEEDLE is here{tail}");
    let found = doc("x", "", ["", "Title", "", &body, ""]);
    let cut = excerpt(&found, &terms).unwrap();
    assert!(cut.chars().count() <= SEARCH_EXCERPT_MAX_CHARS, "{cut}");
    assert!(cut.starts_with('…') && cut.ends_with('…'));
    assert!(cut.contains("NEEDLE"));
    // Whitespace runs collapse.
    let spaced = doc("y", "", ["", "", "", "a\n\n\t needle   b", ""]);
    assert_eq!(excerpt(&spaced, &terms).unwrap(), "a needle b");
    // No match in a text field (a key hit): the body's start; no text at all: nothing.
    let keyed = doc("z", "", ["T-HQ1", "", "", "Body text", ""]);
    assert_eq!(excerpt(&keyed, &query_terms("t-hq1")).unwrap(), "Body text");
    assert_eq!(
        excerpt(&doc("e", "", ["T-HQ1", "", "", "", ""]), &terms),
        None
    );
    // Every cut lands on a character boundary, whatever the budget meets.
    for n in 0..300 {
        let text = format!("{}needle{}", "é".repeat(n), "🙂".repeat(n));
        let cut = excerpt(&doc("m", "", ["", "", "", &text, ""]), &terms).unwrap();
        assert!(cut.chars().count() <= SEARCH_EXCERPT_MAX_CHARS);
    }
}
