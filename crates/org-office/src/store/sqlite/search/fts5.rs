//! The FTS5 side of a search: the tokenizer every index is declared with, and the safe
//! `MATCH` expression made of a query's words.
//!
//! A query's words come from [`crate::search::text::query_terms`]: runs of letters, digits
//! and `- _ . / @`, so no word holds a quote, `*`, `^`, `:`, a parenthesis or a space. Each
//! is written as a double-quoted FTS5 string (a quote doubled, should one ever appear) with a
//! trailing `*`: a prefix phrase. FTS5 reads everything inside the quotes as text for its
//! tokenizer, never as syntax, so `NOT`, `NEAR/5`, `OR`, `col:val` and SQL are plain words
//! and no user text can change the query's shape or make it fail to parse.

use crate::search::text::QueryTerm;

/// The tokenizer of every search index: the one [`crate::search::text`] mirrors.
pub(super) const TOKENIZE: &str = "porter unicode61 tokenchars '-_./@'";

/// The five indexed columns, in field order.
pub(super) const COLUMNS: &str = "f_ref, f_title, f_tags, f_body, f_extra";

/// `"word"*`: one query word as a quoted prefix phrase.
pub(super) fn phrase(term: &QueryTerm) -> String {
    format!("\"{}\"*", term.word.replace('"', "\"\""))
}

/// Every query word as a prefix phrase, all of them required.
pub(super) fn match_all(terms: &[QueryTerm]) -> String {
    terms.iter().map(phrase).collect::<Vec<_>>().join(" AND ")
}

/// The `CREATE VIRTUAL TABLE` of an index named `table`.
pub(super) fn create_index(table: &str) -> String {
    format!("CREATE VIRTUAL TABLE {table} USING fts5({COLUMNS}, tokenize = \"{TOKENIZE}\");\n")
}
