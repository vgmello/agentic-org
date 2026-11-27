//! Ranking, the same for every engine: field-weighted BM25 exactly as SQLite FTS5's `bm25()`
//! computes it (`k1 = 1.2`, `b = 0.75`, an IDF of at most `1e-6` below zero), the best-first
//! order with its deterministic tie-breakers (`score DESC, updated_at DESC, entity_id ASC`),
//! the cursor of a page, the merge of the separate sources, and a hit's excerpt.
//!
//! BM25's corpus statistics are always those of the documents the caller may see in that
//! source: a document the caller may not see never moves another's score, so a score never
//! tells an outsider that a message or a note exists.

use std::cmp::Ordering;

use org_proto::search::{SearchEntityType, SEARCH_EXCERPT_MAX_CHARS, SEARCH_MAX_CURSOR_BYTES};
use org_proto::{ApiError, ErrorCode};

use super::plan::SearchPlan;
use super::records::{Field, SearchDoc, FIELD_WEIGHTS};
use super::text::{for_each_word, stem, token_count, tokens, QueryTerm};

const K1: f64 = 1.2;
const B: f64 = 0.75;
/// FTS5's floor for an IDF at or below zero.
const MIN_IDF: f64 = 1e-6;
/// How many characters of context an excerpt keeps before its match.
const EXCERPT_LEAD: usize = 60;

/// The statistics of the documents a caller may see in one source.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CorpusStats {
    pub docs: u64,
    /// Their words, all fields together.
    pub tokens: u64,
    /// For each query word in order: how many of those documents hold a word it matches.
    pub term_docs: Vec<u64>,
}

/// How one document matches the query words.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DocMatch {
    /// For each query word: its field-weighted count of matching words.
    pub freq: Vec<f64>,
    /// The document's words, all fields together.
    pub len: u64,
}

impl DocMatch {
    /// Whether every query word matches (the query is an AND of its words).
    pub fn matches_all(&self) -> bool {
        !self.freq.is_empty() && self.freq.iter().all(|f| *f > 0.0)
    }
}

/// How `doc` matches `terms`.
pub fn analyze(doc: &SearchDoc, terms: &[QueryTerm]) -> DocMatch {
    let mut freq = vec![0.0; terms.len()];
    let mut len = 0;
    for (text, weight) in doc.fields.iter().zip(FIELD_WEIGHTS) {
        for_each_word(text, |_, word| {
            len += 1;
            if !terms.iter().any(|term| term.may_match(word)) {
                return;
            }
            let stemmed = stem(word);
            for (count, term) in freq.iter_mut().zip(terms) {
                if term.matches(&stemmed) {
                    *count += weight;
                }
            }
        });
    }
    DocMatch { freq, len }
}

/// How many words a document holds, all fields together (what the SQLite adapters store).
pub fn doc_len(doc: &SearchDoc) -> u64 {
    doc.fields.iter().map(|text| token_count(text)).sum()
}

/// The statistics of `visible` for `terms`, and each document's match.
pub fn analyze_corpus(visible: &[SearchDoc], terms: &[QueryTerm]) -> (CorpusStats, Vec<DocMatch>) {
    let mut stats = CorpusStats {
        term_docs: vec![0; terms.len()],
        ..CorpusStats::default()
    };
    let matches: Vec<DocMatch> = visible.iter().map(|doc| analyze(doc, terms)).collect();
    for m in &matches {
        stats.docs += 1;
        stats.tokens += m.len;
        for (count, f) in stats.term_docs.iter_mut().zip(&m.freq) {
            *count += u64::from(*f > 0.0);
        }
    }
    (stats, matches)
}

/// FTS5's `bm25()` of a document with match `m` in a corpus with `stats`, negated: higher is
/// better.
pub fn bm25(stats: &CorpusStats, m: &DocMatch) -> f64 {
    if stats.docs == 0 {
        return 0.0;
    }
    let rows = stats.docs as f64;
    let avgdl = if stats.tokens == 0 {
        1.0
    } else {
        stats.tokens as f64 / rows
    };
    let mut score = 0.0;
    for (i, f) in m.freq.iter().enumerate() {
        let hits = stats.term_docs.get(i).copied().unwrap_or(0) as f64;
        let mut idf = ((rows - hits + 0.5) / (hits + 0.5)).ln();
        if idf <= 0.0 {
            idf = MIN_IDF;
        }
        score += idf * ((f * (K1 + 1.0)) / (f + K1 * (1.0 - B + B * m.len as f64 / avgdl)));
    }
    score
}

/// One ranked document.
#[derive(Clone, Debug)]
pub struct Scored {
    pub doc: SearchDoc,
    pub score: f64,
}

/// What one source answers: its best hits after the cursor (at most a page and one), and how
/// many visible documents match in all.
#[derive(Clone, Debug, Default)]
pub struct SourcePage {
    pub total: u64,
    pub hits: Vec<Scored>,
}

/// The ranking key of a hit.
fn key<'a>(
    score: f64,
    updated_at: &'a str,
    entity_id: &'a str,
    kind: SearchEntityType,
) -> RankKey<'a> {
    RankKey {
        score,
        updated_at,
        entity_id,
        kind,
    }
}

struct RankKey<'a> {
    score: f64,
    updated_at: &'a str,
    entity_id: &'a str,
    kind: SearchEntityType,
}

fn order(a: &RankKey<'_>, b: &RankKey<'_>) -> Ordering {
    b.score
        .total_cmp(&a.score)
        .then_with(|| b.updated_at.cmp(a.updated_at))
        .then_with(|| a.entity_id.cmp(b.entity_id))
        .then_with(|| a.kind.cmp(&b.kind))
}

fn key_of(hit: &Scored) -> RankKey<'_> {
    key(
        hit.score,
        &hit.doc.meta.updated_at,
        &hit.doc.entity_id,
        hit.doc.entity_type,
    )
}

/// Best first: higher score, then later `updated_at`, then smaller `entity_id`, then type.
pub fn best_first(a: &Scored, b: &Scored) -> Ordering {
    order(&key_of(a), &key_of(b))
}

fn invalid_cursor() -> ApiError {
    ApiError::new(
        ErrorCode::InvalidRequest,
        "cursor is the next_cursor of a previous page",
    )
}

/// Where a page ended: its last hit's ranking key.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchCursor {
    pub score: f64,
    pub updated_at: String,
    pub entity_type: SearchEntityType,
    pub entity_id: String,
}

impl SearchCursor {
    pub fn of(hit: &Scored) -> Self {
        Self {
            score: hit.score,
            updated_at: hit.doc.meta.updated_at.clone(),
            entity_type: hit.doc.entity_type,
            entity_id: hit.doc.entity_id.clone(),
        }
    }

    /// The score's exact bits, then the rest, `~`-separated.
    pub fn encode(&self) -> String {
        format!(
            "{:016x}~{}~{}~{}",
            self.score.to_bits(),
            self.entity_type.as_str(),
            self.updated_at,
            self.entity_id
        )
    }

    /// Parses a `next_cursor`.
    pub fn parse(raw: &str) -> Result<Self, ApiError> {
        if raw.len() > SEARCH_MAX_CURSOR_BYTES {
            return Err(invalid_cursor());
        }
        let mut parts = raw.splitn(4, '~');
        let (Some(bits), Some(kind), Some(updated_at), Some(entity_id)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(invalid_cursor());
        };
        let plain = |text: &str, extra: &[char]| {
            text.chars()
                .all(|c| c.is_ascii_alphanumeric() || extra.contains(&c))
        };
        let score = (bits.len() == 16)
            .then(|| u64::from_str_radix(bits, 16).ok())
            .flatten()
            .map(f64::from_bits)
            .filter(|s| s.is_finite() && *s >= 0.0)
            .ok_or_else(invalid_cursor)?;
        let entity_type = SearchEntityType::parse(kind).ok_or_else(invalid_cursor)?;
        if !plain(updated_at, &['-', ':', '.', '+'])
            || entity_id.is_empty()
            || !plain(entity_id, &['-'])
        {
            return Err(invalid_cursor());
        }
        Ok(Self {
            score,
            updated_at: updated_at.to_owned(),
            entity_type,
            entity_id: entity_id.to_owned(),
        })
    }

    /// Whether `hit` comes after this cursor, best first.
    pub fn precedes(&self, hit: &Scored) -> bool {
        let here = key(
            self.score,
            &self.updated_at,
            &self.entity_id,
            self.entity_type,
        );
        order(&here, &key_of(hit)) == Ordering::Less
    }
}

/// The page `plan` asks of one source's ranked matches: all of them counted, those after the
/// cursor sorted best first, a page and one kept (none when only counting).
pub fn page_of(mut scored: Vec<Scored>, plan: &SearchPlan) -> SourcePage {
    let total = scored.len() as u64;
    if plan.count_only {
        return SourcePage {
            total,
            hits: Vec::new(),
        };
    }
    if let Some(after) = &plan.after {
        scored.retain(|hit| after.precedes(hit));
    }
    scored.sort_by(best_first);
    scored.truncate(plan.fetch());
    SourcePage {
        total,
        hits: scored,
    }
}

/// Scores `candidates` (visible documents that match every word and filter) in a corpus with
/// `stats`, and pages them.
pub fn scored_page(
    candidates: impl IntoIterator<Item = SearchDoc>,
    stats: &CorpusStats,
    plan: &SearchPlan,
) -> SourcePage {
    let scored = candidates
        .into_iter()
        .map(|doc| {
            let score = bm25(stats, &analyze(&doc, &plan.terms));
            Scored { doc, score }
        })
        .collect();
    page_of(scored, plan)
}

/// One page across the sources: every source's total summed, their hits merged best first and
/// cut to `limit`, with the cursor of the last when more follow.
pub fn merge(pages: Vec<SourcePage>, limit: usize) -> (Vec<Scored>, u64, Option<SearchCursor>) {
    let total = pages.iter().map(|page| page.total).sum();
    let mut hits: Vec<Scored> = pages.into_iter().flat_map(|page| page.hits).collect();
    hits.sort_by(best_first);
    let next = (hits.len() > limit).then(|| {
        hits.truncate(limit);
        hits.last().map(SearchCursor::of)
    });
    (hits, total, next.flatten())
}

/// At most [`SEARCH_EXCERPT_MAX_CHARS`] characters of `doc` around its first matching word
/// (the body first, then comments, tags and title), or the start of its body or title.
pub fn excerpt(doc: &SearchDoc, terms: &[QueryTerm]) -> Option<String> {
    for field in [Field::Body, Field::Extra, Field::Tags, Field::Title] {
        let text = doc.field(field);
        let found = tokens(text).find(|t| terms.iter().any(|q| q.matches(&t.term)));
        if let Some(token) = found {
            return Some(window(text, token.start));
        }
    }
    [Field::Body, Field::Title]
        .into_iter()
        .map(|field| doc.field(field))
        .find(|text| !text.trim().is_empty())
        .map(|text| window(text, 0))
}

/// The excerpt of `text` from a little before byte offset `at`: whitespace collapsed, `…`
/// where it is cut, at most [`SEARCH_EXCERPT_MAX_CHARS`] characters in all. Works in whole
/// characters, so it never splits one.
fn window(text: &str, at: usize) -> String {
    let before = text.get(..at).map_or(0, |head| head.chars().count());
    let lead = before.saturating_sub(EXCERPT_LEAD);
    let mut kept: Vec<char> = Vec::new();
    let mut gap = false;
    for c in text.chars().skip(lead) {
        if c.is_whitespace() {
            gap = !kept.is_empty();
            continue;
        }
        if gap {
            kept.push(' ');
            gap = false;
        }
        kept.push(c);
        if kept.len() > SEARCH_EXCERPT_MAX_CHARS {
            break;
        }
    }
    let cut_front = lead > 0;
    let room = SEARCH_EXCERPT_MAX_CHARS - usize::from(cut_front);
    let cut_back = kept.len() > room;
    let keep = if cut_back { room - 1 } else { kept.len() };
    let mut out = String::with_capacity(keep + 8);
    if cut_front {
        out.push('…');
    }
    out.extend(&kept[..keep]);
    if cut_back {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests;
