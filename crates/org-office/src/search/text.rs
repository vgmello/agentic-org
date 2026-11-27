//! How search reads text, the same for every engine: SQLite FTS5's
//! `porter unicode61 tokenchars '-_./@'` tokenizer. Letters and digits, with `- _ . / @`,
//! make a word; case and Latin diacritics are folded; FTS5's Porter stemmer stems the word.
//! The SQLite adapter's FTS5 tables are declared with the same tokenizer, so the memory engine,
//! the scorer and the index agree on what each word is.
//!
//! One known difference: `unicode61` classifies characters by Unicode 6.1, so a symbol
//! assigned since (some newer emoji) is a word character to FTS5 and a separator here. It only
//! ever changes a document's word count by such a symbol; it never changes what a query of
//! letters and digits matches, and never lets anything through a visibility filter.

mod porter;

use org_proto::search::SEARCH_MAX_TERMS;

/// The characters besides letters and digits a word may hold (`tokenchars`).
pub const TOKEN_CHARS: [char; 5] = ['-', '_', '.', '/', '@'];

/// One word of a text, as the index keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    /// The folded, stemmed word.
    pub term: String,
    /// The byte offset in the text where the word starts (a character boundary).
    pub start: usize,
}

/// Whether `c` belongs to a word.
pub fn is_word_char(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphanumeric() || TOKEN_CHARS.contains(&c)
    } else {
        c.is_alphanumeric()
    }
}

/// `c` as `unicode61` folds it: lower case, without a Latin diacritic.
fn fold(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_lowercase();
    }
    let lower = c.to_lowercase().next().unwrap_or(c);
    strip_diacritic(lower)
}

/// The base letter of a lower-case Latin-1 or Latin Extended-A letter with a diacritic.
fn strip_diacritic(c: char) -> char {
    match c {
        'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'ď' => 'd',
        'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ĥ' => 'h',
        'ì'..='ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' => 'i',
        'ĵ' => 'j',
        'ķ' => 'k',
        'ĺ' | 'ļ' | 'ľ' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò'..='ö' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' => 's',
        'ţ' | 'ť' => 't',
        'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ŵ' => 'w',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        other => other,
    }
}

/// Calls `each` with every folded, unstemmed word of `text` and the byte offset where it
/// starts, in order, reusing one buffer: the scorer reads every word of every candidate.
pub fn for_each_word(text: &str, mut each: impl FnMut(usize, &str)) {
    let mut word = String::new();
    let mut start = 0;
    for (at, c) in text.char_indices() {
        if is_word_char(c) {
            if word.is_empty() {
                start = at;
            }
            word.push(fold(c));
        } else if !word.is_empty() {
            each(start, &word);
            word.clear();
        }
    }
    if !word.is_empty() {
        each(start, &word);
    }
}

/// The stem FTS5's Porter stemmer makes of a folded word.
pub fn stem(word: &str) -> String {
    porter::stem(word)
}

/// Every word of `text`, folded and stemmed, in order.
pub fn tokens(text: &str) -> impl Iterator<Item = Token> {
    let mut out = Vec::new();
    for_each_word(text, |start, word| {
        out.push(Token {
            term: porter::stem(word),
            start,
        });
    });
    out.into_iter()
}

/// How many words `text` holds.
pub fn token_count(text: &str) -> u64 {
    let mut count = 0;
    for_each_word(text, |_, _| count += 1);
    count
}

/// One word of a query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryTerm {
    /// The folded word as written: what the index's own tokenizer is handed.
    pub word: String,
    /// Its stem: an indexed word matches when its stem starts with this one (a prefix query).
    pub stem: String,
}

impl QueryTerm {
    /// Whether an indexed (stemmed) word matches this query word.
    pub fn matches(&self, term: &str) -> bool {
        term.starts_with(self.stem.as_str())
    }

    /// Whether the folded, unstemmed `word` may match: Porter never changes a word's first
    /// character, so a word that starts with another one cannot. Spares the stemmer nearly
    /// every word of a document.
    pub fn may_match(&self, word: &str) -> bool {
        self.stem.as_bytes().first() == word.as_bytes().first()
    }
}

/// The words of a query: every run of word characters, whatever surrounds it, so quotes,
/// operators (`AND`, `NEAR/5`, `*`, `^`, `:`, parentheses) and SQL are plain words or
/// separators and never syntax. Words with the same stem count once; at most
/// [`SEARCH_MAX_TERMS`] are kept. Empty when the text holds no word at all.
pub fn query_terms(q: &str) -> Vec<QueryTerm> {
    let mut terms: Vec<QueryTerm> = Vec::new();
    for_each_word(q, |_, word| {
        let stem = porter::stem(word);
        if terms.len() < SEARCH_MAX_TERMS && !terms.iter().any(|t| t.stem == stem) {
            terms.push(QueryTerm {
                word: word.to_owned(),
                stem,
            });
        }
    });
    terms
}

#[cfg(test)]
mod tests;
