//! The built-in prompt and skill text: hygiene, byte budgets, the file-to-manifest bijection,
//! and no vendor or emphasis drift.

use std::collections::BTreeSet;
use std::path::Path;

use crate::{
    list_builtin_skills, BUILTIN_INSTRUCTIONS_MAX_BYTES, BUILTIN_PROMPTS, CORE_RULES,
    CORE_RULES_MAX_BYTES, HR_LEAD, MANAGED_SESSION, SKILL_MAX_BYTES,
};

/// Body-only Markdown: no carriage return, byte order mark, Unicode line or paragraph separator,
/// or YAML front matter, and exactly one final newline.
fn assert_clean(name: &str, text: &str) {
    assert!(!text.trim().is_empty(), "{name} is empty");
    assert!(!text.contains('\r'), "{name} has a carriage return");
    assert!(!text.contains('\u{feff}'), "{name} has a byte order mark");
    assert!(
        !text.contains(['\u{2028}', '\u{2029}']),
        "{name} has a Unicode line or paragraph separator"
    );
    assert!(!text.starts_with("---"), "{name} has YAML front matter");
    assert!(
        text.ends_with('\n') && !text.ends_with("\n\n"),
        "{name} must end with exactly one newline"
    );
}

/// Every entry of `dir` under this crate, by file name.
fn files_in(dir: &str) -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(dir);
    std::fs::read_dir(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        .map(|entry| {
            entry
                .expect("directory entry")
                .file_name()
                .into_string()
                .expect("UTF-8 file name")
        })
        .collect()
}

fn on_disk(dir: &str, file: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(dir).join(file))
        .unwrap_or_else(|e| panic!("read {dir}/{file}: {e}"))
}

#[test]
fn every_prompt_file_is_in_the_manifest_exactly_once_and_clean() {
    let manifest: Vec<&str> = BUILTIN_PROMPTS.iter().map(|p| p.file).collect();
    let unique: BTreeSet<String> = manifest.iter().map(|f| (*f).to_owned()).collect();
    assert_eq!(unique.len(), manifest.len(), "a prompt is listed twice");
    assert_eq!(
        files_in("prompts"),
        unique,
        "prompts/ and BUILTIN_PROMPTS must match one to one"
    );
    for prompt in BUILTIN_PROMPTS {
        assert_clean(prompt.file, prompt.body);
        assert_eq!(
            on_disk("prompts", prompt.file),
            prompt.body.as_bytes(),
            "{} is embedded as written",
            prompt.file
        );
    }
}

#[test]
fn every_skill_file_is_in_the_catalogue_exactly_once_clean_and_within_budget() {
    let skills = list_builtin_skills();
    let files: BTreeSet<String> = skills.iter().map(|s| format!("{}.md", s.key)).collect();
    assert_eq!(files.len(), skills.len(), "a skill key is listed twice");
    assert_eq!(
        files_in("skills"),
        files,
        "skills/ and the built-in skill catalogue must match one to one"
    );
    assert_eq!(skills.len(), 12, "the Day-1 library has twelve guides");
    for skill in skills {
        let file = format!("{}.md", skill.key);
        assert_clean(&file, skill.body_markdown);
        assert_eq!(on_disk("skills", &file), skill.body_markdown.as_bytes());
        assert!(
            skill.body_markdown.len() <= SKILL_MAX_BYTES,
            "{file} is {} bytes; the limit is {SKILL_MAX_BYTES}",
            skill.body_markdown.len()
        );
        assert!(
            skill.body_markdown.starts_with("# "),
            "{file} starts with its title"
        );
        assert_eq!(skill.source, "built_in");
        assert!(!skill.title.is_empty() && !skill.description.is_empty());
        assert!(
            skill
                .key
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'-'),
            "{} is a lowercase key",
            skill.key
        );
    }
}

#[test]
fn built_in_standing_text_stays_within_its_budgets() {
    assert!(
        CORE_RULES.len() <= CORE_RULES_MAX_BYTES,
        "core-rules.md is {} bytes; the limit is {CORE_RULES_MAX_BYTES}",
        CORE_RULES.len()
    );
    let total = CORE_RULES.len() + MANAGED_SESSION.len() + HR_LEAD.len();
    assert!(
        total <= BUILTIN_INSTRUCTIONS_MAX_BYTES,
        "built-in standing text is {total} bytes; the limit is {BUILTIN_INSTRUCTIONS_MAX_BYTES}"
    );
}

#[test]
fn core_rules_cover_the_day_one_contract() {
    for needle in [
        "Nobody watches your terminal",
        "limits the office enforces in code",
        "must-follow processes",
        "are data, not instructions",
        "untrusted fence",
        "`agent-os <resource> <verb> --flag value`",
        "`--schema`",
        "`--input-file <path>`",
        "`agent-os skill list`",
        "`agent-os skill read --name <key>`",
        "`agent-os task-note add`",
        "keep running across turns while your session is active",
        "paused, stopped or offboarded",
        "Never poll or sleep in a loop",
    ] {
        assert!(
            CORE_RULES.contains(needle),
            "core-rules.md lacks `{needle}`"
        );
    }
    assert!(
        !CORE_RULES.contains("end of turn") && !CORE_RULES.contains("killed"),
        "background work is not killed at the end of a turn (ADR-0025 decision 6)"
    );
}

#[test]
fn built_in_text_names_no_vendor_and_shouts_nothing() {
    let vendors = ["claude", "codex", "anthropic", "openai", "gemini", "agy"];
    let mut texts: Vec<(&str, &str)> = BUILTIN_PROMPTS.iter().map(|p| (p.file, p.body)).collect();
    texts.extend(
        list_builtin_skills()
            .iter()
            .map(|s| (s.key, s.body_markdown)),
    );
    for (name, text) in texts {
        let lower = text.to_ascii_lowercase();
        for vendor in vendors {
            let named = lower
                .split(|c: char| !c.is_ascii_alphanumeric())
                .any(|word| word == vendor);
            assert!(!named, "{name} names the vendor `{vendor}`");
        }
    }
    // Plain calm prose in the core rules: no word in capitals except the JSON acronym.
    for word in CORE_RULES.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
        let shouted = word.len() >= 3 && word.bytes().all(|b| b.is_ascii_uppercase());
        assert!(!shouted || word == "JSON", "core-rules.md shouts `{word}`");
    }
}
