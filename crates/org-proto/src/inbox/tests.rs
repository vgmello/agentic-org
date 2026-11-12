use super::*;
use serde_json::{json, Value};

fn item() -> Value {
    json!({
        "ref":"hq:org:member:unplaced", "source":"org", "kind":"problem",
        "member":"member", "summary":"Place this agent", "state":"open",
        "clears":"placed", "dismissable":false,
        "actions":[{"type":"open","label":"Open","open":{"target":"member","id":"member"}}]
    })
}

#[test]
fn future_values_and_actions_do_not_lose_the_page_or_source_identity() {
    let mut value = item();
    value["source"] = json!("future_source");
    value["kind"] = json!("future_kind");
    value["clears"] = json!("future_clearing");
    value["actions"][0]["open"]["target"] = json!("future_target");
    value["actions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"future", "payload":42}));
    let page: InboxListResult = serde_json::from_value(json!({
        "items":[value], "counts":{"approvals":0,"questions":0,"decisions":0,"problems":0,"total":1,"future":1},
        "as_of":{"epoch":"instance","seq":1}, "degraded":[]
    })).unwrap();
    let row = &page.items[0];
    assert_eq!(row.source, "future_source");
    assert_eq!(row.kind, InboxKind::Unknown);
    assert_eq!(row.clears, InboxClears::Unknown);
    assert!(
        matches!(&row.actions[0], Action::Open { open, .. } if open.target == OpenTarget::Unknown)
    );
    assert_eq!(row.actions[1], Action::Unknown);
    assert_eq!(page.counts.total, 1);
}

#[test]
fn every_frozen_kind_and_clearing_value_round_trips() {
    for (wire, kind) in [
        ("approval", InboxKind::Approval),
        ("question", InboxKind::Question),
        ("decision", InboxKind::Decision),
        ("problem", InboxKind::Problem),
    ] {
        assert_eq!(
            serde_json::from_value::<InboxKind>(json!(wire)).unwrap(),
            kind
        );
        assert_eq!(serde_json::to_value(kind).unwrap(), json!(wire));
    }
    for (wire, clears) in [
        ("placed", InboxClears::Placed),
        ("model_set", InboxClears::ModelSet),
        ("answered", InboxClears::Answered),
        ("accepted", InboxClears::Accepted),
        ("leaves_fixed", InboxClears::LeavesFixed),
        ("assigned_or_closed", InboxClears::AssignedOrClosed),
        ("unblocked", InboxClears::Unblocked),
    ] {
        assert_eq!(
            serde_json::from_value::<InboxClears>(json!(wire)).unwrap(),
            clears
        );
        assert_eq!(serde_json::to_value(clears).unwrap(), json!(wire));
    }
    for (wire, target) in [
        ("member", OpenTarget::Member),
        ("work_item", OpenTarget::WorkItem),
        ("bug", OpenTarget::Bug),
        ("message", OpenTarget::Message),
    ] {
        assert_eq!(
            serde_json::from_value::<OpenTarget>(json!(wire)).unwrap(),
            target
        );
        assert_eq!(serde_json::to_value(target).unwrap(), json!(wire));
    }
}

#[test]
fn actions_keep_fixed_inputs_and_fresh_ids_separate() {
    let value = json!({"type":"call", "label":"Reopen", "op":"bug.verify",
        "params":{"outcome":"still_happens","item":"bug"}, "fresh":["change_id"],
        "input":{"field":"note","required":true,"max_chars":1000}});
    let action: Action = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(action).unwrap(), value);
    let mut malformed = value;
    malformed["params"] = json!("not an object");
    assert!(serde_json::from_value::<Action>(malformed).is_err());
    let row: InboxItem = serde_json::from_value(item()).unwrap();
    assert_eq!(
        serde_json::to_value(row.actions[0].clone()).unwrap(),
        item()["actions"][0]
    );
}

#[test]
fn list_and_hint_share_counts_position_and_degraded_without_a_version_bump() {
    assert_eq!(InboxList::METHOD, "inbox.list");
    assert_eq!(InboxChanged::NAME, "inbox.changed");
    assert_eq!(crate::API_VERSION, 1);
    let page: InboxListResult = serde_json::from_value(json!({
        "items":[item()], "counts":{"approvals":2,"questions":3,"decisions":4,"problems":5,"total":16},
        "as_of":{"epoch":"opaque-instance","seq":99}, "degraded":["messages"]
    })).unwrap();
    let hint = InboxChangedData {
        counts: page.counts.clone(),
        as_of: page.as_of.clone(),
        degraded: page.degraded.clone(),
    };
    let hint: InboxChangedData =
        serde_json::from_value(serde_json::to_value(hint).unwrap()).unwrap();
    assert_eq!(page.counts, hint.counts);
    assert_eq!(page.as_of, hint.as_of);
    assert_eq!(page.degraded, hint.degraded);
    for state in [InboxState::Open, InboxState::Handled] {
        let params = InboxListParams {
            state,
            kinds: Some(vec![InboxKind::Approval]),
            project: Some("project".into()),
            team: Some("team".into()),
            cursor: Some("opaque".into()),
            limit: Some(100),
        };
        assert_eq!(
            serde_json::from_value::<InboxListParams>(serde_json::to_value(&params).unwrap())
                .unwrap(),
            params
        );
    }
}

#[test]
fn every_encoded_bound_accepts_max_and_cuts_max_plus_one_at_a_char_boundary() {
    for cap in [512, 2048, 1024, 128] {
        for c in ["x", "\u{1}", "\"", "\\", "é", "🧭"] {
            let cost = crate::msg::json_string_len(c) - 2;
            let mut exact = c.repeat((cap - 2) / cost);
            exact.push_str(&"x".repeat((cap - 2) % cost));
            assert_eq!(crate::msg::json_string_len(&exact), cap);
            assert_eq!(bounded(&exact, cap), exact);
            let over = exact + "x";
            assert_eq!(crate::msg::json_string_len(&over), cap + 1);
            let cut = bounded(&over, cap);
            assert!(crate::msg::json_string_len(&cut) <= cap, "{cap}: {c:?}");
            let prefix = cut.strip_suffix('…').unwrap();
            assert!(over.starts_with(prefix));
            let next = over[prefix.len()..].chars().next().unwrap();
            let cost = crate::msg::json_string_len(next.encode_utf8(&mut [0; 4])) - 2;
            assert!(crate::msg::json_string_len(prefix) + cost + 3 > cap);
        }
    }
}

#[test]
fn a_maximal_page_of_escaped_control_characters_fits_the_frame_with_its_envelope() {
    let mut row: InboxItem = serde_json::from_value(item()).unwrap();
    row.summary = "\u{1}".repeat(512);
    row.context = Some("\u{2}".repeat(2048));
    row.recommendation = Some("\u{3}".repeat(1024));
    row.options = Some(vec!["\u{4}".repeat(128); 9]);
    row.bound_text();
    for (text, cap) in [
        (&row.summary, 512),
        (row.context.as_ref().unwrap(), 2048),
        (row.recommendation.as_ref().unwrap(), 1024),
    ] {
        assert!(crate::msg::json_string_len(text) <= cap);
        assert!(text.ends_with('…'));
    }
    assert_eq!(row.options.as_ref().unwrap().len(), 8);
    for text in row.options.as_ref().unwrap() {
        assert!(crate::msg::json_string_len(text) <= 128);
        assert!(text.ends_with('…'));
    }
    let page = InboxListResult {
        items: (0..100)
            .map(|i| {
                let mut item = row.clone();
                item.reference = format!("hq:org:member-{i}:unplaced");
                item
            })
            .collect(),
        next_cursor: Some("opaque".into()),
        counts: InboxCounts {
            problems: 100,
            total: 100,
            ..InboxCounts::default()
        },
        as_of: InboxPosition {
            epoch: "instance".into(),
            seq: 100,
        },
        degraded: vec!["messages".into(), "work".into()],
    };
    let frame = crate::Response::ok::<InboxList>(9_007_199_254_740_991, &page).unwrap();
    assert!(serde_json::to_vec(&frame).unwrap().len() < crate::MAX_FRAME_BYTES);
}

/// The sibling wire convention: `field?: T` means absent for None, never an emitted null.
/// Explicit null on input is equivalent to absence, as in msg.rs's optional cursors.
fn optional_fields<T: serde::Serialize + serde::de::DeserializeOwned>(
    base: Value,
    fields: &[(&str, Value)],
) {
    let absent = serde_json::to_value(serde_json::from_value::<T>(base.clone()).unwrap()).unwrap();
    for (field, present) in fields {
        assert!(
            absent.get(field).is_none(),
            "{field}: absent input must omit the wire key"
        );
        let mut value = base.clone();
        value[field] = present.clone();
        let value = serde_json::to_value(serde_json::from_value::<T>(value).unwrap()).unwrap();
        assert_eq!(
            value.get(field),
            Some(present),
            "{field}: retain the present value"
        );
        let mut value = base.clone();
        value[field] = Value::Null;
        let value = serde_json::to_value(serde_json::from_value::<T>(value).unwrap()).unwrap();
        assert_eq!(value, absent, "{field}: null input equals absence");
    }
    assert_eq!(absent, base, "only the declared required keys are emitted");
}

#[test]
fn every_optional_wire_field_omits_none_preserves_some_and_normalizes_null() {
    let mut row = item();
    row.as_object_mut().unwrap().remove("member");
    optional_fields::<InboxItem>(
        row,
        &[
            ("member", json!("member")),
            ("project", json!("project")),
            ("team", json!("team")),
            ("context", json!("why")),
            ("recommendation", json!("recommendation")),
            ("options", json!(["one", "two"])),
            ("created_at_ms", json!(1)),
            ("expires_at_ms", json!(2)),
            ("handled_at_ms", json!(3)),
            ("outcome", json!("placed")),
        ],
    );
    optional_fields::<InboxListParams>(
        json!({"state":"open"}),
        &[
            ("kinds", json!(["problem"])),
            ("project", json!("project")),
            ("team", json!("team")),
            ("cursor", json!("opaque")),
            ("limit", json!(50)),
        ],
    );
    optional_fields::<InboxListResult>(
        json!({"items":[],
        "counts":{"approvals":0,"questions":0,"decisions":0,"problems":0,"total":0},
        "as_of":{"epoch":"instance","seq":0},"degraded":[]}),
        &[("next_cursor", json!("opaque"))],
    );
    optional_fields::<Action>(
        json!({"type":"call","label":"Reply","op":"msg.reply","params":{},"fresh":["id"]}),
        &[(
            "input",
            json!({"field":"body","required":true,"max_chars":10000}),
        )],
    );
}

#[test]
fn last_page_wire_has_no_cursor_key_for_optional_string_clients() {
    let page = InboxListResult {
        items: vec![],
        next_cursor: None,
        counts: InboxCounts::default(),
        as_of: InboxPosition {
            epoch: "instance".into(),
            seq: 0,
        },
        degraded: vec![],
    };
    let last = serde_json::to_value(&page).unwrap();
    assert!(!last.as_object().unwrap().contains_key("next_cursor"));
    let next = serde_json::to_value(InboxListResult {
        next_cursor: Some("opaque".into()),
        ..page
    })
    .unwrap();
    assert_eq!(
        next.get("next_cursor").and_then(Value::as_str),
        Some("opaque")
    );
    println!("INBOX_CURSOR_SHAPE={}", json!({"last":last,"next":next}));
}
