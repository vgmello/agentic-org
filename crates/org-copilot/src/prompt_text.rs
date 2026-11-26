//! The copilot's fixed prompt text, kept as body-only Markdown files under `prompts/` and
//! embedded at compile time. Each file ends with one newline; a task body is used without it
//! ([`body`]), so every rendered template is byte for byte what it was when the text was
//! inline. Changing any of this text changes a template: bump the feature's version.

/// The drafting copilot's rules (`text.improve`, `bug.from_sentence`).
pub(crate) const DRAFT_PREAMBLE: &str = include_str!("../prompts/draft-preamble.md");
/// The `text.improve` task.
pub(crate) const DRAFT_TEXT_IMPROVE: &str = include_str!("../prompts/draft-text-improve.md");
/// The `bug.from_sentence` task.
pub(crate) const DRAFT_BUG_FROM_SENTENCE: &str =
    include_str!("../prompts/draft-bug-from-sentence.md");
/// The read-only channel judgment rules (`channel.watch`, `channel.summarize`).
pub(crate) const CHANNEL_PREAMBLE: &str = include_str!("../prompts/channel-preamble.md");
/// The `channel.watch` task.
pub(crate) const CHANNEL_WATCH: &str = include_str!("../prompts/channel-watch.md");
/// The `channel.summarize` task.
pub(crate) const CHANNEL_SUMMARIZE: &str = include_str!("../prompts/channel-summarize.md");

/// Every file under `prompts/`, exactly once: `(file name, text)`.
#[cfg(test)]
pub(crate) const PROMPT_FILES: &[(&str, &str)] = &[
    ("draft-preamble.md", DRAFT_PREAMBLE),
    ("draft-text-improve.md", DRAFT_TEXT_IMPROVE),
    ("draft-bug-from-sentence.md", DRAFT_BUG_FROM_SENTENCE),
    ("channel-preamble.md", CHANNEL_PREAMBLE),
    ("channel-watch.md", CHANNEL_WATCH),
    ("channel-summarize.md", CHANNEL_SUMMARIZE),
];

/// `text` without its final newline: a task body sits in the middle of a template.
pub(crate) fn body(text: &str) -> &str {
    text.strip_suffix('\n').unwrap_or(text)
}

#[cfg(test)]
#[path = "prompt_text_tests.rs"]
mod tests;
