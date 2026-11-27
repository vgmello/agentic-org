//! SQLite FTS5's Porter stemmer (`fts5_tokenize.c`), ported byte for byte, so the memory
//! engine and the scorer stem a word exactly as the SQLite index's `porter` tokenizer does.
//! Like FTS5 it works on the UTF-8 bytes of a folded word, leaves words shorter than 3 or
//! longer than 64 bytes as they are, and counts every byte but `a e i o u` (and `y` after a
//! consonant) as a consonant.

/// FTS5's longest word it stems (`FTS5_PORTER_MAX_TOKEN`).
const MAX_TOKEN: usize = 64;

/// The condition a suffix rule checks on the stem before the suffix.
#[derive(Clone, Copy)]
enum Cond {
    /// `m > 0`.
    MGt0,
    /// `m > 1`.
    MGt1,
    /// `m > 1` and the stem ends in `s` or `t`.
    MGt1ST,
}

/// One suffix rule: the suffix, its replacement and the condition on the stem.
type Rule = (&'static [u8], &'static [u8], Cond);

const STEP2: [Rule; 21] = [
    (b"ational", b"ate", Cond::MGt0),
    (b"tional", b"tion", Cond::MGt0),
    (b"enci", b"ence", Cond::MGt0),
    (b"anci", b"ance", Cond::MGt0),
    (b"izer", b"ize", Cond::MGt0),
    (b"logi", b"log", Cond::MGt0),
    (b"bli", b"ble", Cond::MGt0),
    (b"alli", b"al", Cond::MGt0),
    (b"entli", b"ent", Cond::MGt0),
    (b"eli", b"e", Cond::MGt0),
    (b"ousli", b"ous", Cond::MGt0),
    (b"ization", b"ize", Cond::MGt0),
    (b"ation", b"ate", Cond::MGt0),
    (b"ator", b"ate", Cond::MGt0),
    (b"alism", b"al", Cond::MGt0),
    (b"iveness", b"ive", Cond::MGt0),
    (b"fulness", b"ful", Cond::MGt0),
    (b"ousness", b"ous", Cond::MGt0),
    (b"aliti", b"al", Cond::MGt0),
    (b"iviti", b"ive", Cond::MGt0),
    (b"biliti", b"ble", Cond::MGt0),
];

const STEP3: [Rule; 7] = [
    (b"ical", b"ic", Cond::MGt0),
    (b"ness", b"", Cond::MGt0),
    (b"icate", b"ic", Cond::MGt0),
    (b"iciti", b"ic", Cond::MGt0),
    (b"ful", b"", Cond::MGt0),
    (b"ative", b"", Cond::MGt0),
    (b"alize", b"al", Cond::MGt0),
];

const STEP4: [Rule; 19] = [
    (b"al", b"", Cond::MGt1),
    (b"ance", b"", Cond::MGt1),
    (b"ence", b"", Cond::MGt1),
    (b"er", b"", Cond::MGt1),
    (b"ic", b"", Cond::MGt1),
    (b"able", b"", Cond::MGt1),
    (b"ible", b"", Cond::MGt1),
    (b"ant", b"", Cond::MGt1),
    (b"ement", b"", Cond::MGt1),
    (b"ment", b"", Cond::MGt1),
    (b"ent", b"", Cond::MGt1),
    (b"ion", b"", Cond::MGt1ST),
    (b"ou", b"", Cond::MGt1),
    (b"ism", b"", Cond::MGt1),
    (b"ate", b"", Cond::MGt1),
    (b"iti", b"", Cond::MGt1),
    (b"ous", b"", Cond::MGt1),
    (b"ive", b"", Cond::MGt1),
    (b"ize", b"", Cond::MGt1),
];

/// The stem FTS5's `porter` tokenizer makes of `word`, a folded word.
pub(super) fn stem(word: &str) -> String {
    let bytes = word.as_bytes();
    if bytes.len() < 3 || bytes.len() > MAX_TOKEN {
        return word.to_owned();
    }
    let mut buf = bytes.to_vec();
    step1(&mut buf);
    if buf.len() >= 2 {
        apply(&mut buf, &STEP2);
    }
    if buf.len() >= 2 {
        apply(&mut buf, &STEP3);
    }
    if buf.len() >= 2 {
        apply(&mut buf, &STEP4);
    }
    step5(&mut buf);
    // FTS5 may cut a multi-byte character in two in a pathological word; the stem is then
    // the folded word itself, which no index term can equal.
    String::from_utf8(buf).unwrap_or_else(|_| word.to_owned())
}

fn is_vowel(c: u8, y_is_vowel: bool) -> bool {
    matches!(c, b'a' | b'e' | b'i' | b'o' | b'u') || (y_is_vowel && c == b'y')
}

/// FTS5's `fts5PorterGobbleVC`: the length of the stem's first vowel-consonant run, or 0.
fn gobble_vc(stem: &[u8], prev_consonant: bool) -> usize {
    let mut consonant = prev_consonant;
    let mut i = 0;
    while i < stem.len() {
        consonant = !is_vowel(stem[i], consonant);
        if !consonant {
            break;
        }
        i += 1;
    }
    i += 1;
    while i < stem.len() {
        consonant = !is_vowel(stem[i], consonant);
        if consonant {
            return i + 1;
        }
        i += 1;
    }
    0
}

fn m_gt0(stem: &[u8]) -> bool {
    gobble_vc(stem, false) != 0
}

fn m_gt1(stem: &[u8]) -> bool {
    let n = gobble_vc(stem, false);
    n != 0 && gobble_vc(&stem[n..], true) != 0
}

fn m_eq1(stem: &[u8]) -> bool {
    let n = gobble_vc(stem, false);
    n != 0 && gobble_vc(&stem[n..], true) == 0
}

/// `*o`: the stem ends consonant-vowel-consonant, the last not `w`, `x` or `y`.
fn o_star(stem: &[u8]) -> bool {
    let Some(&last) = stem.last() else {
        return false;
    };
    if matches!(last, b'w' | b'x' | b'y') {
        return false;
    }
    let mut mask = 0u8;
    let mut consonant = false;
    for &c in stem {
        consonant = !is_vowel(c, consonant);
        mask = ((mask << 1) | u8::from(consonant)) & 0b111;
    }
    mask == 0b101
}

/// `*v*`: the stem holds a vowel.
fn has_vowel(stem: &[u8]) -> bool {
    stem.iter().enumerate().any(|(i, &c)| is_vowel(c, i > 0))
}

fn ends_with(buf: &[u8], suffix: &[u8]) -> bool {
    buf.len() > suffix.len() && buf.ends_with(suffix)
}

fn replace_tail(buf: &mut Vec<u8>, cut: usize, with: &[u8]) {
    buf.truncate(buf.len() - cut);
    buf.extend_from_slice(with);
}

/// The first rule whose suffix ends `buf` (leaving a stem) decides: it replaces the suffix
/// when its condition holds, and no later rule is tried either way.
fn apply(buf: &mut Vec<u8>, rules: &[Rule]) {
    for &(suffix, with, cond) in rules {
        if !ends_with(buf, suffix) {
            continue;
        }
        let stem = &buf[..buf.len() - suffix.len()];
        let holds = match cond {
            Cond::MGt0 => m_gt0(stem),
            Cond::MGt1 => m_gt1(stem),
            Cond::MGt1ST => matches!(stem.last(), Some(b's' | b't')) && m_gt1(stem),
        };
        if holds {
            replace_tail(buf, suffix.len(), with);
        }
        return;
    }
}

/// Steps 1a, 1b and 1c.
fn step1(buf: &mut Vec<u8>) {
    step1a(buf);
    if step1b(buf) && !step1b2(buf) {
        step1b_cleanup(buf);
    }
    step1c(buf);
}

/// Step 1a: sses -> ss, ies -> i, ss -> ss, s -> "".
fn step1a(buf: &mut Vec<u8>) {
    let n = buf.len();
    if buf[n - 1] != b's' {
        return;
    }
    if buf[n - 2] == b'e' {
        let sses = n > 4 && buf[n - 4] == b's' && buf[n - 3] == b's';
        let ies = n > 3 && buf[n - 3] == b'i';
        buf.truncate(if sses || ies { n - 2 } else { n - 1 });
    } else if buf[n - 2] != b's' {
        buf.truncate(n - 1);
    }
}

/// Post-1b stem cleanup: undouble trailing consonant (not l/s/z) or restore silent `e`.
fn step1b_cleanup(buf: &mut Vec<u8>) {
    let n = buf.len();
    let c = buf[n - 1];
    let doubled = n >= 2 && c == buf[n - 2];
    if !is_vowel(c, false) && !matches!(c, b'l' | b's' | b'z') && doubled {
        buf.truncate(n - 1);
    } else if m_eq1(buf) && o_star(buf) {
        buf.push(b'e');
    }
}

/// Step 1c: y -> i when the stem holds a vowel.
fn step1c(buf: &mut [u8]) {
    let n = buf.len();
    if n >= 1 && buf[n - 1] == b'y' && has_vowel(&buf[..n - 1]) {
        buf[n - 1] = b'i';
    }
}

/// Step 1b: eed -> ee (m > 0); ed and ing are removed when the stem holds a vowel. Whether
/// `ed` or `ing` went.
fn step1b(buf: &mut Vec<u8>) -> bool {
    if ends_with(buf, b"eed") {
        if m_gt0(&buf[..buf.len() - 3]) {
            buf.truncate(buf.len() - 1);
        }
        return false;
    }
    for suffix in [&b"ed"[..], b"ing"] {
        if ends_with(buf, suffix) {
            let stem = buf.len() - suffix.len();
            if has_vowel(&buf[..stem]) {
                buf.truncate(stem);
                return true;
            }
            return false;
        }
    }
    false
}

/// After step 1b: at -> ate, bl -> ble, iz -> ize. Whether one applied.
fn step1b2(buf: &mut Vec<u8>) -> bool {
    for (suffix, with) in [(&b"at"[..], &b"ate"[..]), (b"bl", b"ble"), (b"iz", b"ize")] {
        if ends_with(buf, suffix) {
            replace_tail(buf, suffix.len(), with);
            return true;
        }
    }
    false
}

/// Steps 5a and 5b.
fn step5(buf: &mut Vec<u8>) {
    let n = buf.len();
    if n >= 1 && buf[n - 1] == b'e' {
        let stem = &buf[..n - 1];
        if m_gt1(stem) || (m_eq1(stem) && !o_star(stem)) {
            buf.truncate(n - 1);
        }
    }
    let n = buf.len();
    if n > 1 && buf[n - 1] == b'l' && buf[n - 2] == b'l' && m_gt1(&buf[..n - 1]) {
        buf.truncate(n - 1);
    }
}
