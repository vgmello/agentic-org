use serde_json::json;

use super::*;

#[test]
fn params_refuse_unknown_keys_and_keep_their_wire_spelling() {
    let parsed: SearchQueryParams = serde_json::from_value(json!({
        "q": "token rotation",
        "types": ["task", "private_decision"],
        "count_only": true,
        "limit": 5,
        "fields": ["ref", "updated_at"],
    }))
    .unwrap();
    assert_eq!(parsed.q, "token rotation");
    assert_eq!(
        parsed.types,
        [SearchEntityType::Task, SearchEntityType::PrivateDecision]
    );
    assert!(parsed.count_only);
    assert_eq!(
        parsed.fields,
        [SearchHitField::Ref, SearchHitField::UpdatedAt]
    );
    let back = serde_json::to_value(&parsed).unwrap();
    assert_eq!(back["types"], json!(["task", "private_decision"]));
    assert!(back.get("project").is_none());

    for refused in [
        json!({"q": "x", "sql": "1=1"}),
        json!({"q": "x", "types": ["unknown_type"]}),
        json!({"q": "x", "fields": ["body"]}),
        json!({"types": ["task"]}),
    ] {
        assert!(
            serde_json::from_value::<SearchQueryParams>(refused.clone()).is_err(),
            "{refused}"
        );
    }
}

#[test]
fn every_type_and_field_round_trips_through_its_spelling() {
    for kind in SearchEntityType::ALL {
        assert_eq!(SearchEntityType::parse(kind.as_str()), Some(kind));
        assert_eq!(serde_json::to_value(kind).unwrap(), json!(kind.as_str()));
    }
    for field in SearchHitField::ALL {
        assert_eq!(SearchHitField::parse(field.as_str()), Some(field));
        assert_eq!(serde_json::to_value(field).unwrap(), json!(field.as_str()));
    }
    assert!(SearchEntityType::SHARED
        .iter()
        .all(|kind| !kind.is_private()));
    assert!(SearchEntityType::PrivateNote.is_private());
    assert!(SearchEntityType::PrivateDecision.is_private());
    assert_eq!(SearchEntityType::parse("Task"), None);
    assert_eq!(SearchQuery::METHOD, "search.query");
}

#[test]
fn debug_never_shows_the_query_or_what_was_found() {
    let params = SearchQueryParams {
        q: "SECRET_QUERY_TEXT".into(),
        ..SearchQueryParams::default()
    };
    assert!(!format!("{params:?}").contains("SECRET_QUERY_TEXT"));
    let hit = SearchHit {
        entity_type: SearchEntityType::PrivateDecision,
        entity_id: "0193a5c0-0000-7000-8000-000000000001".into(),
        reference: None,
        title: Some("SECRET_TITLE".into()),
        excerpt: Some("SECRET_EXCERPT".into()),
        score: Some(1.5),
        updated_at: None,
        status: None,
        project_id: None,
        conversation_id: None,
        task_id: None,
        author_id: None,
    };
    let shown = format!("{hit:?}");
    assert!(!shown.contains("SECRET_TITLE") && !shown.contains("SECRET_EXCERPT"));
    let wire = serde_json::to_value(&hit).unwrap();
    assert_eq!(wire["entity_type"], "private_decision");
    assert!(wire.get("ref").is_none());
    assert_eq!(wire["score"], 1.5);
}

#[test]
fn coverage_reports_an_omitted_source_with_its_reason() {
    let result = SearchQueryResult {
        coverage: SearchCoverage {
            searched: vec![],
            omitted: vec![SearchOmission {
                source: SearchSource::PrivateNotes,
                reason: SearchOmissionReason::HqOnly,
            }],
        },
        ..SearchQueryResult::default()
    };
    let wire = serde_json::to_value(&result).unwrap();
    assert_eq!(
        wire["coverage"]["omitted"],
        json!([{"source": "private_notes", "reason": "hq_only"}])
    );
    assert_eq!(wire["total"], 0);
    let back: SearchQueryResult = serde_json::from_value(wire).unwrap();
    assert_eq!(back, result);
}
