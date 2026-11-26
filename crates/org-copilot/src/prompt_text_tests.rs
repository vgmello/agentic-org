//! The copilot prompt files: hygiene, the file-to-manifest bijection, and what they render.

use std::collections::BTreeSet;
use std::path::Path;

use super::PROMPT_FILES;

#[test]
fn every_prompt_file_is_in_the_manifest_exactly_once_and_clean() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("prompts");
    let on_disk: BTreeSet<String> = std::fs::read_dir(&dir)
        .expect("prompts directory")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .into_string()
                .expect("UTF-8")
        })
        .collect();
    let listed: BTreeSet<String> = PROMPT_FILES.iter().map(|(f, _)| (*f).to_owned()).collect();
    assert_eq!(listed.len(), PROMPT_FILES.len(), "a prompt is listed twice");
    assert_eq!(
        on_disk, listed,
        "prompts/ and PROMPT_FILES must match one to one"
    );
    for (file, text) in PROMPT_FILES {
        assert_eq!(
            std::fs::read(dir.join(file)).expect("prompt file"),
            text.as_bytes(),
            "{file} is embedded as written"
        );
        assert!(!text.trim().is_empty(), "{file} is empty");
        assert!(!text.contains('\r'), "{file} has a carriage return");
        assert!(!text.contains('\u{feff}'), "{file} has a byte order mark");
        assert!(
            !text.contains(['\u{2028}', '\u{2029}']),
            "{file} has a Unicode line or paragraph separator"
        );
        assert!(!text.starts_with("---"), "{file} has YAML front matter");
        assert!(
            text.ends_with('\n') && !text.ends_with("\n\n"),
            "{file} must end with exactly one newline"
        );
    }
}

#[test]
fn templates_keep_one_blank_line_between_rules_and_task_and_no_trailing_newline_in_tasks() {
    for registry in [crate::builtin(), crate::watcher::watcher_registry()] {
        for feature in registry.iter() {
            let head = &feature.template().head;
            assert!(head.contains(".\n\nTask ("), "{}: {head}", feature.id());
            assert!(
                head.contains(".\n\nSchema of your answer:\n"),
                "{}: {head}",
                feature.id()
            );
        }
    }
}
