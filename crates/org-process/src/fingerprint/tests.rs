//! The fingerprint (processes.md §3.3, Appendix C), table-driven: each case fails without its
//! guard.

use super::*;

/// A work for `project` with these chains (outermost team first).
fn work(project: &str, chains: &[&[&str]]) -> Work {
    let chain = |c: &&[&str]| c.iter().map(|id| TeamId::new(id).unwrap()).collect();
    let chains = chains.iter().map(chain).collect();
    Work::new(ProjectKey::new(project).unwrap(), chains).unwrap()
}

/// A fingerprint of (handbook commit, integration commit, work), or with no work.
fn key(handbook: &str, integration: Option<&str>, work: Option<Work>) -> Fingerprint {
    match work {
        Some(work) => fingerprint(handbook, integration, &work),
        None => fingerprint_without_work(handbook),
    }
}

type Case = (&'static str, Fingerprint, Fingerprint);

fn base() -> Fingerprint {
    key(
        "h1",
        Some("i1"),
        Some(work("atlas", &[&["platform", "web"], &["mobile"]])),
    )
}

#[test]
fn equal_inputs_give_equal_keys_whatever_the_chain_order() {
    let cases: Vec<Case> = vec![
        ("the same inputs", base(), base()),
        (
            "chains in another order",
            base(),
            key(
                "h1",
                Some("i1"),
                Some(work("atlas", &[&["mobile"], &["platform", "web"]])),
            ),
        ),
        (
            "three chains, rotated",
            key(
                "h1",
                None,
                Some(work("atlas", &[&["a"], &["b"], &["c", "d"]])),
            ),
            key(
                "h1",
                None,
                Some(work("atlas", &[&["c", "d"], &["a"], &["b"]])),
            ),
        ),
        (
            "two chains of one length, swapped",
            key("h1", None, Some(work("atlas", &[&["a"], &["b"]]))),
            key("h1", None, Some(work("atlas", &[&["b"], &["a"]]))),
        ),
        (
            "no work, the same handbook commit",
            key("h1", None, None),
            key("h1", None, None),
        ),
    ];
    for (name, a, b) in cases {
        assert_eq!(a, b, "{name}");
    }
}

#[test]
fn a_change_to_any_one_input_changes_the_key() {
    let at = |handbook, integration, chains: &[&[&str]]| {
        key(handbook, integration, Some(work("atlas", chains)))
    };
    let chains: &[&[&str]] = &[&["platform", "web"], &["mobile"]];
    let cases: Vec<Case> = vec![
        ("the handbook commit", base(), at("h2", Some("i1"), chains)),
        (
            "the integration commit",
            base(),
            at("h1", Some("i2"), chains),
        ),
        ("no integration commit", base(), at("h1", None, chains)),
        (
            "an empty integration commit isn't none",
            at("h1", Some(""), chains),
            at("h1", None, chains),
        ),
        (
            "the project",
            base(),
            key("h1", Some("i1"), Some(work("orion", chains))),
        ),
        (
            "the order inside a chain",
            at("h1", None, &[&["a", "b"]]),
            at("h1", None, &[&["b", "a"]]),
        ),
        (
            "a team in a chain",
            base(),
            at("h1", Some("i1"), &[&["platform", "api"], &["mobile"]]),
        ),
        (
            "a chain added",
            base(),
            at(
                "h1",
                Some("i1"),
                &[&["platform", "web"], &["mobile"], &["ops"]],
            ),
        ),
        (
            "a chain removed",
            base(),
            at("h1", Some("i1"), &[&["platform", "web"]]),
        ),
        (
            "a prefix chain, which resolving drops, still counts",
            base(),
            at(
                "h1",
                Some("i1"),
                &[&["platform", "web"], &["mobile"], &["platform"]],
            ),
        ),
        (
            "a copy of a chain, which resolving drops, still counts",
            at("h1", None, &[&["web"]]),
            at("h1", None, &[&["web"], &["web"]]),
        ),
        ("no chains", base(), at("h1", Some("i1"), &[])),
    ];
    for (name, a, b) in cases {
        assert_ne!(a, b, "{name}");
    }
}

#[test]
fn no_work_has_its_own_shape() {
    let cases: Vec<Case> = vec![
        (
            "no work against a project with no repository and no teams",
            key("h1", None, None),
            key("h1", None, Some(work("atlas", &[]))),
        ),
        (
            "no work against a work, whatever its commits",
            key("h1", None, None),
            key("h1", Some("h1"), Some(work("atlas", &[&["web"]]))),
        ),
        (
            "no work, another handbook commit",
            key("h1", None, None),
            key("h2", None, None),
        ),
    ];
    for (name, a, b) in cases {
        assert_ne!(a, b, "{name}");
    }
}

#[test]
fn parts_never_run_together() {
    let at = |handbook, integration, chains: &[&[&str]]| {
        key(handbook, integration, Some(work("atlas", chains)))
    };
    let cases: Vec<Case> = vec![
        (
            "commits split differently",
            at("ab", Some("c"), &[]),
            at("a", Some("bc"), &[]),
        ),
        (
            "team ids split differently inside a chain",
            at("h1", None, &[&["ab", "c"]]),
            at("h1", None, &[&["a", "bc"]]),
        ),
        (
            "one chain against two",
            at("h1", None, &[&["a", "b"]]),
            at("h1", None, &[&["a"], &["b"]]),
        ),
        (
            "teams moved between chains",
            at("h1", None, &[&["a", "b"], &["c"]]),
            at("h1", None, &[&["a"], &["b", "c"]]),
        ),
        (
            "a separator-like team id",
            at("h1", None, &[&["a_b"]]),
            at("h1", None, &[&["a"], &["b"]]),
        ),
    ];
    for (name, a, b) in cases {
        assert_ne!(a, b, "{name}");
    }
}
