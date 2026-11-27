//! Unit tests of the documentation tree's path validator, codecs, index rendering and ID
//! derivation. The office-level lifecycle, dual-engine rebuild and security tests are in
//! `crate::office::project_docs_tests`.

use org_store::{DurablePath, Id, Timestamp};

use super::features::derive_id;
use super::index;
use super::paths::{
    file_stem_from_title, parse_durable, DocPath, DocsFile, MediaFile, Section, MAX_DEPTH,
};
use super::records::{
    decode_doc, decode_media, encode_doc, encode_media, is_doc_kind, is_summary, DocMedia,
    DocStatus, MediaKind, ProjectDoc,
};
use crate::organization::records::{By, Meta};

const PROJECT: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607101";
const OTHER: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607102";
const DOC: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607103";
const PERSON: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607104";

fn id(text: &str) -> Id {
    Id::parse(text).unwrap()
}

fn doc(path: &str) -> ProjectDoc {
    ProjectDoc {
        id: id(DOC),
        project_id: id(PROJECT),
        path: DocPath::parse(path).unwrap(),
        kind: "guide".into(),
        title: "Setup guide".into(),
        status: DocStatus::Current,
        summary: Some("How to set up.\nTwo lines.".into()),
        owner_employee_id: Some(id(PERSON)),
        body_markdown: "# Setup\n\n---\nNot front matter: a rule in the body.\n".into(),
        meta: Meta::created(&By::Employee(id(PERSON)), Timestamp(1_730_000_000_000)),
    }
}

fn media(file: &str) -> DocMedia {
    DocMedia {
        id: id(OTHER),
        project_id: id(PROJECT),
        file: MediaFile::parse(file).unwrap(),
        kind: MediaKind::Diagram,
        media_type: "image/png".into(),
        bytes: 42,
        sha256: "ab".repeat(32),
        title: "Architecture".into(),
        summary: String::new(),
        meta: Meta::created(&By::Employee(id(PERSON)), Timestamp(1_730_000_000_000)),
    }
}

#[test]
fn document_paths_accept_lowercase_relative_markdown_only() {
    for ok in [
        "readme.md",
        "guides/setup.md",
        "a/b/c/setup.v2.md",
        "x_y-z.md",
        "0.md",
    ] {
        let parsed = DocPath::parse(ok).unwrap_or_else(|e| panic!("{ok}: {e}"));
        assert_eq!(parsed.as_str(), ok);
    }
    let deep = format!("{}x.md", "d/".repeat(MAX_DEPTH));
    let long = format!("{}.md", "a".repeat(512));
    for bad in [
        "",
        "../escape.md",
        "guides/../../escape.md",
        "a/..md",
        "/abs.md",
        "index.yaml",
        "media/inside.md",
        "a//b.md",
        "a/.hidden.md",
        ".md",
        "a.md/",
        "Readme.md",
        "a\\b.md",
        "a\0b.md",
        "a\nb.md",
        "notes.txt",
        "dir.v2/x.md",
        "é.md",
        "a/ b.md",
        deep.as_str(),
        long.as_str(),
    ] {
        assert!(DocPath::parse(bad).is_err(), "accepted {bad:?}");
    }
    let path = DocPath::parse("guides/ops/setup.md").unwrap();
    assert_eq!(path.section().as_str(), "guides/ops");
    assert_eq!(path.file_name(), "setup.md");
    assert_eq!(
        DocPath::parse("top.md").unwrap().section(),
        Section::default()
    );
}

#[test]
fn sections_and_media_files_refuse_traversal_and_foreign_types() {
    for ok in ["", "guides", "a/b_c/d-e"] {
        assert!(Section::parse(ok).is_ok(), "{ok}");
    }
    for bad in [
        "/x",
        "x/",
        "media",
        "media/sub",
        "a//b",
        "a.b",
        "../x",
        "A",
        "a\0",
    ] {
        assert!(Section::parse(bad).is_err(), "accepted section {bad:?}");
    }
    let guides = Section::parse("guides").unwrap();
    assert!(guides.contains(&Section::parse("guides/ops").unwrap()));
    assert!(!guides.contains(&Section::parse("guidesx").unwrap()));
    assert!(Section::default().contains(&guides));

    for ok in ["arch.png", "a-b_c.jpeg", "x.v2.webp", "y.gif", "z.jpg"] {
        assert!(MediaFile::parse(ok).is_ok(), "{ok}");
    }
    for bad in [
        "../a.png",
        "a/b.png",
        "a.svg",
        "a.html",
        "a.png.yaml",
        ".png",
        "A.png",
        "a..png",
        "png",
        "a\0.png",
    ] {
        assert!(MediaFile::parse(bad).is_err(), "accepted media {bad:?}");
    }
    let file = MediaFile::parse("arch.png").unwrap();
    assert_eq!(file.rel_path(), "media/arch.png");
    assert_eq!(MediaFile::from_rel_path("media/arch.png"), Some(file));
    assert_eq!(MediaFile::from_rel_path("other/arch.png"), None);
}

#[test]
fn the_family_owns_every_file_under_a_projects_docs_and_nothing_else() {
    let at =
        |rest: &str| DurablePath::parse(&format!("state/projects/{PROJECT}/docs/{rest}")).unwrap();
    let project = id(PROJECT);
    assert_eq!(
        parse_durable(&at("index.yaml")),
        Some(DocsFile::Index(project.clone()))
    );
    assert!(
        matches!(parse_durable(&at("guides/a.md")), Some(DocsFile::Doc(p, d)) if p == project && d.as_str() == "guides/a.md")
    );
    assert!(matches!(
        parse_durable(&at("media/a.png")),
        Some(DocsFile::MediaBlob(..))
    ));
    assert!(matches!(
        parse_durable(&at("media/a.png.yaml")),
        Some(DocsFile::MediaMeta(..))
    ));
    for foreign in [
        "notes.txt",
        "media/a.svg",
        "media/sub/a.png",
        "-lead.md",
        "a.md.bak",
    ] {
        assert_eq!(
            parse_durable(&at(foreign)),
            Some(DocsFile::Foreign(project.clone())),
            "{foreign}"
        );
    }
    for other in [
        format!("state/projects/{PROJECT}/project.yaml"),
        format!("state/projects/{PROJECT}/processes/{DOC}.md"),
        "state/projects/not-a-uuid/docs/a.md".to_owned(),
    ] {
        assert_eq!(
            parse_durable(&DurablePath::parse(&other).unwrap()),
            None,
            "{other}"
        );
    }
}

#[test]
fn titles_become_safe_file_stems() {
    assert_eq!(file_stem_from_title("Setup Guide: v2!"), "setup-guide-v2");
    assert_eq!(file_stem_from_title("日本語"), "document");
    assert_eq!(file_stem_from_title("../../etc/passwd"), "etc-passwd");
    assert!(file_stem_from_title(&"x".repeat(300)).len() <= 64);
}

#[test]
fn documents_round_trip_and_bad_front_matter_is_refused() {
    let original = doc("guides/setup.md");
    let bytes = encode_doc(&original);
    let decoded = decode_doc(&bytes, &id(PROJECT), original.path.clone()).unwrap();
    assert_eq!(decoded, original);
    // A file in another project's tree is refused.
    assert!(decode_doc(&bytes, &id(OTHER), original.path.clone()).is_err());
    let text = String::from_utf8(bytes).unwrap();
    for (from, to) in [
        ("kind: \"guide\"", "kind: \"brief\""),
        ("kind: \"guide\"", "kind: \"Bad Kind\""),
        ("status: \"current\"", "status: \"archived\""),
        ("title: \"Setup guide\"", "title: \"\""),
        ("revision: 1", "revision: 1\nextra: 1"),
    ] {
        let tampered = text.replacen(from, to, 1);
        assert_ne!(tampered, text, "{from}");
        assert!(
            decode_doc(tampered.as_bytes(), &id(PROJECT), original.path.clone()).is_err(),
            "accepted {to}"
        );
    }
    let ownerless = text.replacen(
        &format!("owner_employee_id: \"{PERSON}\""),
        "owner_employee_id: null",
        1,
    );
    assert!(decode_doc(ownerless.as_bytes(), &id(PROJECT), original.path).is_err());
    assert!(is_doc_kind("runbook") && !is_doc_kind("changelog") && !is_doc_kind(""));
    assert!(is_summary("two\nlines") && !is_summary(" edge") && !is_summary("bidi\u{202e}"));
}

#[test]
fn media_metadata_round_trips_and_must_agree_with_its_file() {
    let original = media("arch.png");
    let bytes = encode_media(&original);
    let decoded = decode_media(&bytes, &id(PROJECT), original.file.clone()).unwrap();
    assert_eq!(decoded, original);
    assert!(decode_media(&bytes, &id(PROJECT), MediaFile::parse("other.png").unwrap()).is_err());
    let text = String::from_utf8(bytes).unwrap();
    for (from, to) in [
        ("media_type: \"image/png\"", "media_type: \"image/svg+xml\""),
        ("bytes: 42", "bytes: 0"),
        ("type: \"diagram\"", "type: \"video\""),
    ] {
        let tampered = text.replacen(from, to, 1);
        assert!(
            decode_media(tampered.as_bytes(), &id(PROJECT), original.file.clone()).is_err(),
            "accepted {to}"
        );
    }
    let bad_sha = text.replacen(&"ab".repeat(32), &"AB".repeat(32), 1);
    assert!(decode_media(bad_sha.as_bytes(), &id(PROJECT), original.file).is_err());
}

#[test]
fn the_index_is_deterministic_and_lists_sections_documents_and_media() {
    let mut second = doc("a/z.md");
    second.id = id("0190a5d2-7c3e-7a01-8b2c-3d4e5f607105");
    let first = doc("guides/setup.md");
    let assets = [media("arch.png")];
    let one = index::render(&id(PROJECT), &[first.entry(), second.entry()], &assets);
    let two = index::render(&id(PROJECT), &[second.entry(), first.entry()], &assets);
    assert_eq!(one, two);
    let text = String::from_utf8(one).unwrap();
    assert!(text.starts_with("# Maintained by the office"));
    assert!(text.find("a/z.md").unwrap() < text.find("guides/setup.md").unwrap());
    assert!(text.contains("\"path\":\"media/arch.png\""));
    assert!(text.contains("\"document_count\":1"));
    // The rendered index is one bare YAML mapping the strict reader accepts.
    let parsed = org_docs::parse_mapping(&text).unwrap();
    assert_eq!(parsed["project_id"], PROJECT);
    assert_eq!(parsed["documents"].as_array().unwrap().len(), 2);
}

#[test]
fn derived_ids_are_stable_uuid_v7s_per_command() {
    let a = derive_id(
        "project-doc",
        "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071ff",
        1_730_000_000_000,
    );
    let b = derive_id(
        "project-doc",
        "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071ff",
        1_730_000_000_000,
    );
    let c = derive_id(
        "project-doc-media",
        "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071ff",
        1_730_000_000_000,
    );
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert!(org_proto::is_uuid_v7(a.as_str()), "{a}");
}
