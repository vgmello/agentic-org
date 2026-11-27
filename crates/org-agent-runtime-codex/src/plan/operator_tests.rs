//! The operator execpolicy's coverage (`TASK-80.1` pass 2): every forbidden program is
//! forbidden by name, by path and by version, both in the emitted Starlark and as a prefix.

use super::*;

#[test]
fn the_rules_file_is_the_rules_folder_and_its_one_file() {
    assert_eq!(
        RULES_FILE,
        format!("{OPERATOR_RULES_DIR}/{OPERATOR_RULES_FILE_NAME}")
    );
    assert!(LAYER_DIRS.contains(&OPERATOR_RULES_DIR));
}

#[test]
fn paths_and_versions_of_a_forbidden_program_are_forbidden_words() {
    let words = forbidden_words();
    for word in [
        "sh",
        "/bin/sh",
        "/bin/bash",
        "/usr/bin/python3",
        "/usr/bin/env",
        "/usr/local/bin/node",
        "/opt/homebrew/bin/python3",
        "/opt/homebrew/bin/bash",
        "python3.9",
        "python3.12",
        "python3.14",
        "/usr/bin/python3.12",
        "python2.7",
        "perl5",
        "lua5.4",
        "/usr/sbin/chroot",
        "busybox",
        "/usr/bin/git",
        "/usr/bin/curl",
        "/bin/nc",
        "/usr/bin/xargs",
        "/usr/bin/awk",
        "/bin/sed",
        "/usr/bin/find",
        "/usr/bin/wget",
        "/bin/dash",
        "/bin/zsh",
        "/usr/bin/perl",
        "/usr/bin/ruby",
    ] {
        assert!(words.iter().any(|w| w == word), "{word} is not forbidden");
    }
    let mut sorted = words.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), words.len(), "each word once");
    assert!(!words.iter().any(|w| w.contains("agent-os")));
}

#[test]
fn a_forbidden_program_is_recognised_by_any_path_or_version() {
    for word in [
        "bash",
        "/bin/bash",
        "/bin/sh",
        "/usr/bin/python3",
        "/usr/bin/env",
        "/opt/homebrew/bin/python3",
        "python3.12",
        "python3.15",
        "/nix/store/abc-python3/bin/python3.11",
        "perl5.40",
        "node22",
        "./sh",
        "../../usr/bin/env",
    ] {
        assert!(is_forbidden_program(word), "{word}");
    }
    for word in [
        "agent-os",
        "/usr/local/bin/agent-os",
        "pythonic",
        "nodeless",
        "ls",
    ] {
        assert!(!is_forbidden_program(word), "{word}");
    }
}

#[test]
fn the_emitted_starlark_names_every_word_and_an_example_for_each() {
    let spec = org_agent_runtime::testkit::operator_spec("codex");
    let rules = rules(&spec);
    let start = rules.find("pattern = [[").expect("a pattern");
    let end = start + rules[start..].find("]]").expect("the pattern's end");
    let pattern = &rules[start..end];
    for word in forbidden_words() {
        let quoted = serde_json::to_string(&word).unwrap();
        assert!(
            pattern.contains(&format!("{quoted},")),
            "{word} in the pattern"
        );
        let example = serde_json::to_string(&format!("{word} x")).unwrap();
        assert!(rules.contains(&example), "{word}'s match example");
    }
    for word in [
        "\"/bin/sh\"",
        "\"/usr/bin/python3\"",
        "\"/usr/bin/env\"",
        "\"/opt/homebrew/bin/python3\"",
        "\"python3.12\"",
    ] {
        assert!(pattern.contains(word), "{word}");
    }
    assert!(!pattern.contains("agent-os"));
    assert!(!rules.contains("\"allow\""));
    assert!(rules.contains("not_match = [\"agent-os --help\"]"));
}
