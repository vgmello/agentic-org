//! Wire tests of the documentation tree's calls.

use super::*;

#[test]
fn methods_are_the_docs_family_and_unknown_enums_decode() {
    assert_eq!(DocsIndex::METHOD, "docs.index");
    assert_eq!(DocsList::METHOD, "docs.list");
    assert_eq!(DocsInspect::METHOD, "docs.inspect");
    assert_eq!(DocsCreate::METHOD, "docs.create");
    assert_eq!(DocsUpdate::METHOD, "docs.update");
    assert_eq!(DocsMove::METHOD, "docs.move");
    assert_eq!(DocsPutMedia::METHOD, "docs.put_media");
    let status: DocStatusWire = serde_json::from_str("\"archived\"").unwrap();
    assert_eq!(status, DocStatusWire::Unknown);
    let kind: DocMediaKindWire = serde_json::from_str("\"video\"").unwrap();
    assert_eq!(kind, DocMediaKindWire::Unknown);
    assert_eq!(
        DOC_MEDIA_MAX_BASE64_CHARS,
        4 * DOC_MEDIA_MAX_BYTES.div_ceil(3)
    );
}

#[test]
fn put_media_debug_never_prints_bytes_and_bodies_take_route_fields_by_default() {
    let params = DocsPutMediaParams {
        content_base64: "c2VjcmV0LWJ5dGVz".into(),
        ..Default::default()
    };
    let debug = format!("{params:?}");
    assert!(!debug.contains("c2VjcmV0"), "{debug}");
    assert!(debug.contains("<16 chars>"));
    let update: DocsUpdateParams =
        serde_json::from_str(r#"{"command_id":"c","expected_revision":2,"title":"New"}"#).unwrap();
    assert!(update.project.is_empty() && update.document_id.is_empty());
    assert_eq!(update.title.as_deref(), Some("New"));
    let media = serde_json::to_value(DocMediaWire {
        id: "i".into(),
        project_id: "p".into(),
        path: "media/a.png".into(),
        kind: DocMediaKindWire::Diagram,
        media_type: "image/png".into(),
        bytes: 3,
        sha256: "0".repeat(64),
        title: "A".into(),
        summary: String::new(),
        revision: 1,
        created_at: None,
        updated_at: None,
    })
    .unwrap();
    assert_eq!(media["type"], "diagram");
}
