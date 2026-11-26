use super::LinkKind;

#[test]
fn typed_link_kind_inventory_is_exhaustive() {
    // A new known variant must update org-office assembly::LINK_KINDS too.
    // This match is inside the defining crate: a new enum variant must get an index.
    fn index(kind: LinkKind) -> usize {
        match kind {
            LinkKind::Fixes => 0,
            LinkKind::PartOf => 1,
            LinkKind::FromRequest => 2,
            LinkKind::ReportedFrom => 3,
            LinkKind::RelatesTo => 4,
            LinkKind::PaysBack => 5,
            LinkKind::Unknown => 6,
        }
    }
    let kinds = [
        LinkKind::Fixes,
        LinkKind::PartOf,
        LinkKind::FromRequest,
        LinkKind::ReportedFrom,
        LinkKind::RelatesTo,
        LinkKind::PaysBack,
        LinkKind::Unknown,
    ];
    let names: Vec<_> = kinds
        .iter()
        .filter(|kind| **kind != LinkKind::Unknown)
        .map(|kind| serde_json::to_value(kind).unwrap())
        .collect();
    assert_eq!(
        serde_json::Value::Array(names),
        serde_json::json!([
            "fixes",
            "part_of",
            "from_request",
            "reported_from",
            "relates_to",
            "pays_back"
        ])
    );
    for (i, kind) in kinds.into_iter().enumerate() {
        assert_eq!(index(kind), i);
    }
}
