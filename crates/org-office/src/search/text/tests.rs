use org_proto::search::SEARCH_MAX_TERMS;

use super::*;

fn terms(text: &str) -> Vec<String> {
    tokens(text).map(|t| t.term).collect()
}

fn words_of(q: &str) -> Vec<String> {
    query_terms(q).into_iter().map(|t| t.word).collect()
}

#[test]
fn words_keep_the_token_chars_fold_case_and_latin_diacritics_and_stem() {
    assert_eq!(
        terms("Deploying the T-HQ12 fix: src/main.rs, Café @alpha"),
        [
            "deploi",
            "the",
            "t-hq12",
            "fix",
            "src/main.r",
            "cafe",
            "@alpha"
        ]
    );
    assert_eq!(
        terms("naïve RÉSUMÉ Ångström"),
        ["naiv", "resum", "angstrom"]
    );
    assert_eq!(terms("  \t\n "), Vec::<String>::new());
    assert_eq!(token_count("one, two; three"), 3);
}

#[test]
fn every_word_starts_on_a_character_boundary_of_multibyte_text() {
    let text = "日本語 émoji 🙂 x-y ação";
    for token in tokens(text) {
        assert!(text.is_char_boundary(token.start), "{token:?}");
        let rest = &text[token.start..];
        assert!(rest.chars().next().is_some_and(is_word_char), "{token:?}");
    }
    assert_eq!(terms("ação"), ["acao"]);
}

#[test]
fn hostile_query_text_is_plain_words_never_syntax() {
    assert_eq!(words_of("foo\" OR 1=1"), ["foo", "or", "1"]);
    assert_eq!(words_of("NEAR/5 deploy"), ["near/5", "deploy"]);
    assert_eq!(words_of("col:val ^start"), ["col", "val", "start"]);
    assert_eq!(words_of("---"), ["---"]);
    for nothing in ["*", "()", "\"\"", "^", ":", "  ", "'\";", "\u{0}"] {
        assert!(query_terms(nothing).is_empty(), "{nothing:?}");
    }
    // The same stem counts once; the word as written goes to the index's tokenizer.
    let deploy = query_terms("deploy deploying Deploys");
    assert_eq!(deploy.len(), 1);
    assert_eq!(deploy[0].word, "deploy");
    assert_eq!(deploy[0].stem, "deploi");
    assert!(deploy[0].matches("deploi"));
    assert!(!deploy[0].matches("deploy"));
}

#[test]
fn a_query_keeps_at_most_the_term_limit() {
    let q: Vec<String> = (0..40).map(|n| format!("w{n}")).collect();
    assert_eq!(query_terms(&q.join(" ")).len(), SEARCH_MAX_TERMS);
}

/// The Porter rules FTS5 applies, word by word (the SQLite adapter's tests also compare the
/// stems with FTS5's own index).
#[test]
fn the_porter_port_stems_like_fts5() {
    for (word, stem) in [
        ("caresses", "caress"),
        ("ponies", "poni"),
        ("ties", "ti"),
        ("caress", "caress"),
        ("cats", "cat"),
        ("feed", "feed"),
        ("agreed", "agre"),
        ("plastered", "plaster"),
        ("bled", "bled"),
        ("motoring", "motor"),
        ("sing", "sing"),
        ("conflated", "conflat"),
        ("troubled", "troubl"),
        ("sized", "size"),
        ("hopping", "hop"),
        ("tanned", "tan"),
        ("falling", "fall"),
        ("hissing", "hiss"),
        ("fizzed", "fizz"),
        ("failing", "fail"),
        ("filing", "file"),
        ("happy", "happi"),
        ("sky", "sky"),
        ("relational", "relat"),
        ("conditional", "condit"),
        ("valenci", "valenc"),
        ("digitizer", "digit"),
        ("generalization", "gener"),
        ("operator", "oper"),
        ("hopefulness", "hope"),
        ("formality", "formal"),
        ("sensitivity", "sensit"),
        ("sensibility", "sensibl"),
        ("electrical", "electr"),
        ("goodness", "good"),
        ("adjustable", "adjust"),
        ("replacement", "replac"),
        ("adoption", "adopt"),
        ("communism", "commun"),
        ("activate", "activ"),
        ("effective", "effect"),
        ("probate", "probat"),
        ("controll", "control"),
        ("roll", "roll"),
        ("deployment", "deploy"),
        ("deploying", "deploi"),
        ("at", "at"),
    ] {
        assert_eq!(porter::stem(word), stem, "{word}");
    }
}
