//! Unit tests for delivery BFF query parsing, path precedence, and command-id guards.

use super::parse::{
    parse_deployments_query, parse_features_query, parse_flags_query, parse_links_query, valid_ref,
    valid_uuid,
};
use super::*;
use org_proto::delivery::{
    DeploymentStatusWire, FeatureStatusWire, FlagStatusWire, LinkEntityKindWire, LinkKindWire,
};

const CMD: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607109";
const ID_A: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607101";

#[test]
fn query_parsers_validate_known_filters_and_reject_unknown_or_duplicate_keys() {
    let f = parse_features_query("core".into(), Some("status=live"))
        .ok()
        .unwrap();
    assert_eq!(f.status, Some(FeatureStatusWire::Live));
    assert!(parse_features_query("core".into(), Some("status=bogus")).is_err());
    assert!(parse_features_query("core".into(), Some("status=live&status=retired")).is_err());
    assert!(parse_features_query("core".into(), Some("extra=1")).is_err());

    let l = parse_links_query(
        "core".into(),
        Some(&format!(
            "entity_kind=feature&entity_id={ID_A}&kind=relates_to"
        )),
    )
    .ok()
    .unwrap();
    assert_eq!(l.entity_kind, Some(LinkEntityKindWire::Feature));
    assert_eq!(l.entity_id.as_deref(), Some(ID_A));
    assert_eq!(l.kind, Some(LinkKindWire::RelatesTo));
    // entity_kind without entity_id (or vice versa) is refused fail-closed.
    assert!(parse_links_query("core".into(), Some("entity_kind=feature")).is_err());
    assert!(parse_links_query("core".into(), Some(&format!("entity_id={ID_A}"))).is_err());

    let d = parse_deployments_query(
        "core".into(),
        Some(&format!(
            "release_id={ID_A}&environment=prod&status=succeeded"
        )),
    )
    .ok()
    .unwrap();
    assert_eq!(d.release_id.as_deref(), Some(ID_A));
    assert_eq!(d.environment.as_deref(), Some("prod"));
    assert_eq!(d.status, Some(DeploymentStatusWire::Succeeded));
    assert!(parse_deployments_query("core".into(), Some("release_id=not-uuid")).is_err());

    let fl = parse_flags_query("core".into(), Some("status=active&used_only=true"))
        .ok()
        .unwrap();
    assert_eq!(fl.status, Some(FlagStatusWire::Active));
    assert!(fl.used_only);
    assert!(parse_flags_query("core".into(), Some("used_only=yes")).is_err());
}

#[test]
fn path_ids_win_over_body_and_command_id_is_required_uuid_v7() {
    let pair = CallerAuth::LocalAuthless;

    // Path wins on feature update, release changelog, deployment flag-settings, flag update.
    let call = to_call(
        DeliveryRoute::FeaturesUpdate(pair, "org".into(), "core".into(), ID_A.into()),
        format!(
            r#"{{"command_id":"{CMD}","project":"evil","feature_id":"other","expected_revision":2,"title":"New"}}"#
        )
        .as_bytes(),
    )
    .ok()
    .unwrap();
    let DeliveryCall::FeaturesUpdate(_, _, params) = call else {
        panic!("expected FeaturesUpdate");
    };
    assert_eq!(params.project, "core");
    assert_eq!(params.feature_id, ID_A);

    let call = to_call(
        DeliveryRoute::ReleasesSetFlagSettings(pair, "org".into(), "core".into(), ID_A.into()),
        format!(
            r#"{{"command_id":"{CMD}","project":"evil","release_id":"other","expected_revision":1,"settings":[]}}"#
        )
        .as_bytes(),
    )
    .ok()
    .unwrap();
    let DeliveryCall::ReleasesSetFlagSettings(_, _, params) = call else {
        panic!("expected ReleasesSetFlagSettings");
    };
    assert_eq!(params.project, "core");
    assert_eq!(params.release_id, ID_A);

    // Invalid command_id and unknown fields are rejected before any office call.
    assert!(to_call(
        DeliveryRoute::FeaturesCreate(pair, "org".into(), "core".into()),
        br#"{"command_id":"not-uuid","title":"T"}"#,
    )
    .is_err());
    assert!(to_call(
        DeliveryRoute::FlagsCreate(pair, "org".into(), "core".into()),
        format!(
            r#"{{"command_id":"{CMD}","key":"f","value_type":"boolean","default_value":true,"extra":1}}"#
        )
        .as_bytes(),
    )
    .is_err());

    // Reads refuse non-empty bodies.
    assert!(to_call(
        DeliveryRoute::ReleasesList(pair, "org".into(), "core".into()),
        b"{}",
    )
    .is_err());
    assert!(prepare(
        DeliveryRoute::ReleasesList(pair, "org".into(), "core".into()),
        b"",
    )
    .is_ok());
}

#[test]
fn segment_validators_block_traversal_and_non_uuid_ids() {
    for ok in ["F-1", "R-12", "checkout_v2", ID_A] {
        assert!(valid_ref(ok).is_ok(), "{ok}");
    }
    for bad in ["", "a/b", "a b", "é", &"x".repeat(129)] {
        assert!(valid_ref(bad).is_err(), "{bad:?}");
    }
    assert!(valid_uuid(ID_A).is_ok());
    assert!(valid_uuid("F-1").is_err());
}
