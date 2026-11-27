//! Pure domain rules of private notes (`TASK-111` ADEC-1/2): content validation, title
//! derivation, the tag grammar, patches, digests, cursors, visibility and redaction.

use org_proto::private_notes::{
    PrivateNoteAddParams, PrivateNoteField, PrivateNoteKind, PrivateNoteUpdateParams,
};
use org_proto::ErrorCode;

use super::*;

const AGENT: &str = "0193a5c0-0000-7000-8000-000000000001";
const OTHER_AGENT: &str = "0193a5c0-0000-7000-8000-000000000002";
const NOTE: &str = "0193a5c0-0000-7000-8000-000000000010";

fn add(body: &str) -> PrivateNoteAddParams {
    PrivateNoteAddParams {
        command_id: "c".into(),
        note_id: NOTE.into(),
        task: None,
        project_id: None,
        kind: PrivateNoteKind::Decision,
        title: None,
        why: None,
        alternatives: None,
        tags: Vec::new(),
        body_markdown: body.into(),
        checkpoint: false,
    }
}

fn update() -> PrivateNoteUpdateParams {
    PrivateNoteUpdateParams {
        command_id: "c".into(),
        note_id: NOTE.into(),
        expected_revision: 1,
        kind: None,
        title: None,
        why: None,
        alternatives: None,
        tags: None,
        body_markdown: None,
    }
}

fn record(content: &NoteContent) -> PrivateNoteRecord {
    let cmd = CreateNoteCmd {
        org_id: "org",
        author_id: AGENT,
        task_id: None,
        project_id: None,
        note_id: NOTE,
        content,
        operation_id: "op",
        request_sha256: "sha",
        now: "2026-11-26T10:00:00.000Z",
    };
    PrivateNoteRecord::new_active(&cmd)
}

#[test]
fn a_title_is_derived_from_the_first_line_with_text() {
    assert_eq!(derive_title("\n\n  # Use SQLite  \nbecause"), "Use SQLite");
    assert_eq!(derive_title("###\nreal line"), "real line");
    assert_eq!(derive_title("###"), "###");
    assert_eq!(derive_title("tab\tinside"), "tab inside");
    // Control characters read as spaces before a line is trimmed and chosen.
    assert_eq!(derive_title("\u{0007}"), "Untitled");
    assert_eq!(derive_title("\u{0001}\u{0007} \u{007f}"), "Untitled");
    assert_eq!(derive_title("# \x01\nReal heading"), "Real heading");
    assert_eq!(
        derive_title("\x01 # \x02\n\x03Next\x04line\x05"),
        "Next line"
    );
    assert_eq!(derive_title("\u{0085}Lead\u{009f}"), "Lead");
    let bell = NoteContent::from_add(&add("\u{0007}")).unwrap();
    assert_eq!(bell.title.as_deref(), Some("Untitled"));
    assert_eq!(bell.body_markdown, "\u{0007}");
    let long = "é".repeat(300);
    let derived = derive_title(&long);
    assert_eq!(derived.chars().count(), 120);
    assert!(derived.ends_with('…'));

    let content = NoteContent::from_add(&add("  Ship the cache\n\nDetails")).unwrap();
    assert_eq!(content.title.as_deref(), Some("Ship the cache"));
    assert_eq!(content.body_markdown, "Ship the cache\n\nDetails");
    let mut explicit = add("body");
    explicit.title = Some("  Explicit  ".into());
    assert_eq!(
        NoteContent::from_add(&explicit).unwrap().title.as_deref(),
        Some("Explicit")
    );
    explicit.title = Some("   ".into());
    assert_eq!(
        NoteContent::from_add(&explicit).unwrap().title.as_deref(),
        Some("body")
    );
}

#[test]
fn content_limits_are_enforced_in_characters() {
    let mut params = add("body");
    params.title = Some("t".repeat(200));
    params.why = Some("é".repeat(2_000));
    params.alternatives = Some("a".repeat(2_000));
    let ok = NoteContent::from_add(&params).unwrap();
    assert_eq!(ok.why.as_ref().map(|w| w.chars().count()), Some(2_000));

    let refused = |params: PrivateNoteAddParams| NoteContent::from_add(&params).unwrap_err().code;
    let mut long_title = add("body");
    long_title.title = Some("t".repeat(201));
    assert_eq!(refused(long_title), ErrorCode::TooLarge);
    let mut two_lines = add("body");
    two_lines.title = Some("one\ntwo".into());
    assert_eq!(refused(two_lines), ErrorCode::InvalidRequest);
    let mut long_why = add("body");
    long_why.why = Some("w".repeat(2_001));
    assert_eq!(refused(long_why), ErrorCode::TooLarge);
    let mut long_alternatives = add("body");
    long_alternatives.alternatives = Some("é".repeat(2_001));
    assert_eq!(refused(long_alternatives), ErrorCode::TooLarge);
    assert_eq!(refused(add("  \n ")), ErrorCode::InvalidRequest);

    let mut blank = add("body");
    blank.why = Some("   ".into());
    assert_eq!(NoteContent::from_add(&blank).unwrap().why, None);
}

#[test]
fn tags_follow_the_task_label_grammar_and_cap_at_eight() {
    let tags: Vec<String> = [" Arch ", "arch", "db:sqlite", "a/b-c_d.e", "z9"]
        .map(String::from)
        .to_vec();
    assert_eq!(
        normalize_tags(&tags).unwrap(),
        ["a/b-c_d.e", "arch", "db:sqlite", "z9"]
    );
    for bad in [
        "",
        "-lead",
        "trail.",
        "two words",
        "ümlaut",
        "semi;colon",
        &"x".repeat(65),
    ] {
        assert_eq!(
            normalize_tag(bad).unwrap_err().code,
            ErrorCode::InvalidRequest,
            "{bad:?}"
        );
    }
    let eight: Vec<String> = (0..8).map(|n| format!("t{n}")).collect();
    assert_eq!(normalize_tags(&eight).unwrap().len(), 8);
    let nine: Vec<String> = (0..9).map(|n| format!("t{n}")).collect();
    assert_eq!(
        normalize_tags(&nine).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    // Duplicates after normalization count once.
    let mut dupes = eight.clone();
    dupes.push("T0".into());
    assert_eq!(normalize_tags(&dupes).unwrap().len(), 8);
}

#[test]
fn a_patch_keeps_unchanged_fields_clears_blank_ones_and_rederives_a_cleared_title() {
    assert_eq!(
        NotePatch::from_update(&update()).unwrap_err().code,
        ErrorCode::InvalidRequest,
        "an update that changes nothing is refused"
    );
    let mut params = add("Decision line\nmore");
    params.why = Some("because".into());
    params.alternatives = Some("other".into());
    params.tags = vec!["arch".into()];
    let base = record(&NoteContent::from_add(&params).unwrap());

    let mut change = update();
    change.kind = Some(PrivateNoteKind::Plan);
    change.why = Some(String::new());
    change.tags = Some(Vec::new());
    change.body_markdown = Some("New first line\nrest".into());
    let patched = NotePatch::from_update(&change)
        .unwrap()
        .apply(&base)
        .unwrap();
    assert_eq!(patched.kind, PrivateNoteKind::Plan);
    assert_eq!(patched.why, None, "a blank why clears it");
    assert_eq!(patched.alternatives.as_deref(), Some("other"));
    assert!(patched.tags.is_empty());
    assert_eq!(
        patched.title.as_deref(),
        Some("Decision line"),
        "the title stays"
    );

    let mut retitle = update();
    retitle.title = Some(String::new());
    retitle.body_markdown = Some("Fresh\nbody".into());
    let patched = NotePatch::from_update(&retitle)
        .unwrap()
        .apply(&base)
        .unwrap();
    assert_eq!(patched.title.as_deref(), Some("Fresh"));

    // A carried-over untitled note gains a title on its next edit.
    let mut untitled = base.clone();
    untitled.title = None;
    let mut tag_only = update();
    tag_only.tags = Some(vec!["Later".into()]);
    let patched = NotePatch::from_update(&tag_only)
        .unwrap()
        .apply(&untitled)
        .unwrap();
    assert_eq!(patched.title.as_deref(), Some("Decision line"));
    assert_eq!(patched.tags, ["later"]);

    // A tombstone has no body to patch onto.
    let tombstone = base.tombstone(2, "2026-11-26T11:00:00.000Z");
    assert!(NotePatch::from_update(&tag_only)
        .unwrap()
        .apply(&tombstone)
        .is_none());
}

#[test]
fn digests_separate_every_value_and_absence() {
    let digest = |patch: &NotePatch| {
        let mut input = DigestInput::new("edit");
        patch.digest_into(&mut input);
        input.finish()
    };
    let cleared = NotePatch {
        why: Some(None),
        ..NotePatch::default()
    };
    let kept = NotePatch::default();
    let shifted_a = NotePatch {
        why: Some(Some("ab".into())),
        alternatives: Some(Some("c".into())),
        ..NotePatch::default()
    };
    let shifted_b = NotePatch {
        why: Some(Some("a".into())),
        alternatives: Some(Some("bc".into())),
        ..NotePatch::default()
    };
    let all = [&cleared, &kept, &shifted_a, &shifted_b].map(digest);
    for (i, a) in all.iter().enumerate() {
        assert_eq!(a.len(), 64);
        for b in &all[i + 1..] {
            assert_ne!(a, b);
        }
    }
    assert_eq!(digest(&shifted_a), digest(&shifted_a.clone()));
}

#[test]
fn cursors_round_trip_and_refuse_anything_else() {
    let base = record(&NoteContent::from_add(&add("x")).unwrap());
    let cursor = NoteCursor::of(&base);
    assert_eq!(NoteCursor::parse(&cursor.encode()).unwrap(), cursor);
    for bad in [
        "",
        "garbage",
        "2026-11-26T10:00:00Z~0193a5c0-0000-7000-8000-000000000010",
        "2026-11-26T10:00:00.000Z~not-a-uuid",
        "2026-11-26T10:00:00.000Z",
    ] {
        assert_eq!(
            NoteCursor::parse(bad).unwrap_err().code,
            ErrorCode::InvalidRequest,
            "{bad}"
        );
    }
    assert_eq!(
        query::normalize_since("2026-11-26T12:00:00+02:00").unwrap(),
        "2026-11-26T10:00:00.000Z"
    );
    assert!(query::normalize_since("yesterday").is_err());
    assert_eq!(query::page_limit(None).unwrap(), 100);
    assert!(query::page_limit(Some(0)).is_err());
    assert!(query::page_limit(Some(501)).is_err());
}

fn query<'a>(caller: &'a str, person: bool, author: Option<&'a str>) -> NoteQuery<'a> {
    NoteQuery {
        org_id: "org",
        caller_id: caller,
        caller_is_person: person,
        task_id: None,
        project_id: None,
        kind: None,
        tag: None,
        since: None,
        author_id: author,
        after: None,
        limit: 10,
    }
}

#[test]
fn an_agent_sees_only_its_own_notes_and_a_person_every_authors() {
    let mine = record(&NoteContent::from_add(&add("x")).unwrap());
    assert_eq!(
        query(AGENT, false, None).visible_author(),
        Some(Some(AGENT))
    );
    assert_eq!(
        query(AGENT, false, Some(AGENT)).visible_author(),
        Some(Some(AGENT))
    );
    assert_eq!(
        query(OTHER_AGENT, false, Some(AGENT)).visible_author(),
        None
    );
    assert_eq!(query("person", true, None).visible_author(), Some(None));
    assert_eq!(
        query("person", true, Some(AGENT)).visible_author(),
        Some(Some(AGENT))
    );

    assert!(query(AGENT, false, None).matches(&mine));
    assert!(!query(OTHER_AGENT, false, None).matches(&mine));
    assert!(!query(OTHER_AGENT, false, Some(AGENT)).matches(&mine));
    assert!(query("person", true, None).matches(&mine));
    let tombstone = mine.tombstone(2, "2026-11-26T11:00:00.000Z");
    assert!(!query("person", true, None).matches(&tombstone));

    let mut tagged = query(AGENT, false, None);
    tagged.tag = Some("arch");
    assert!(!tagged.matches(&mine));
    tagged.tag = None;
    tagged.since = Some("2026-11-26T10:00:00.001Z");
    assert!(!tagged.matches(&mine));
    tagged.since = Some("2026-11-26T10:00:00.000Z");
    assert!(tagged.matches(&mine));
}

#[test]
fn field_sets_prune_only_content() {
    let mut params = add("SECRET_BODY");
    params.why = Some("SECRET_WHY".into());
    params.tags = vec!["arch".into()];
    let note = record(&NoteContent::from_add(&params).unwrap());
    let compact = note.to_wire_with(
        AGENT,
        FieldSet::requested(&[PrivateNoteField::Title, PrivateNoteField::Tags]),
    );
    assert_eq!(compact.body_markdown, None);
    assert_eq!(compact.why, None);
    assert_eq!(compact.title.as_deref(), Some("SECRET_BODY"));
    assert_eq!(compact.tags, ["arch"]);
    assert_eq!(compact.id, NOTE);
    assert!(compact.editable);
    let full = note.to_wire_with(OTHER_AGENT, FieldSet::requested(&[]));
    assert_eq!(full.body_markdown.as_deref(), Some("SECRET_BODY"));
    assert!(!full.editable);
}

#[test]
fn debug_output_of_domain_values_redacts_private_text() {
    let mut params = add("SECRET_BODY");
    params.title = Some("SECRET_TITLE".into());
    params.why = Some("SECRET_WHY".into());
    params.alternatives = Some("SECRET_ALTERNATIVES".into());
    params.tags = vec!["secret-tag".into()];
    let content = NoteContent::from_add(&params).unwrap();
    let note = record(&content);
    let patch = NotePatch {
        title: Some(Some("SECRET_TITLE".into())),
        tags: Some(vec!["secret-tag".into()]),
        body_markdown: Some("SECRET_BODY".into()),
        ..NotePatch::default()
    };
    let mut tagged = query(AGENT, false, None);
    tagged.tag = Some("secret-tag");
    let cmd = CreateNoteCmd {
        org_id: "org",
        author_id: AGENT,
        task_id: None,
        project_id: None,
        note_id: NOTE,
        content: &content,
        operation_id: "op",
        request_sha256: "sha",
        now: "2026-11-26T10:00:00.000Z",
    };
    for rendered in [
        format!("{content:?}"),
        format!("{note:?}"),
        format!("{patch:?}"),
        format!("{tagged:?}"),
        format!("{cmd:?}"),
        format!(
            "{:?}",
            replayed_note(&note, 1, "t", Accepted::Edited(&patch))
        ),
    ] {
        for secret in [
            "SECRET_BODY",
            "SECRET_TITLE",
            "SECRET_WHY",
            "SECRET_ALTERNATIVES",
            "secret-tag",
        ] {
            assert!(!rendered.contains(secret), "{secret} leaked: {rendered}");
        }
    }
}

#[test]
fn a_task_less_note_is_not_publishable_and_a_satellite_refuses_everything() {
    let note = record(&NoteContent::from_add(&add("x")).unwrap());
    assert_eq!(
        require_publishable(&note).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    let mut on_task = note.clone();
    on_task.task_id = Some("task".into());
    assert!(require_publishable(&on_task).is_ok());

    let satellite = SatelliteRefusedPrivateNotesStore;
    let content = NoteContent::from_add(&add("x")).unwrap();
    let cmd = CreateNoteCmd {
        org_id: "org",
        author_id: AGENT,
        task_id: None,
        project_id: None,
        note_id: NOTE,
        content: &content,
        operation_id: "op",
        request_sha256: "sha",
        now: "t",
    };
    assert!(!satellite.available());
    assert!(memory::MemoryPrivateNotesStore::new().available());
    assert_eq!(
        satellite.create_note(cmd).unwrap().unwrap_err().code,
        ErrorCode::Unavailable
    );
    assert_eq!(
        SatelliteRefusedPrivateNotesStore::refused().code,
        ErrorCode::Unavailable
    );
    let page = satellite.list_notes(&query("person", true, None)).unwrap();
    assert!(page.notes.is_empty() && page.total == 0 && page.next_cursor.is_none());
    assert!(satellite
        .get_note("org", AGENT, false, NOTE)
        .unwrap()
        .is_none());
}
