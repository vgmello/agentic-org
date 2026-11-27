//! Wire contract tests for `org_proto::delivery`.

use serde_json::Value;

use super::*;
use crate::Call;

#[test]
fn delivery_rpc_method_names_match_wire_contract() {
    let pairs = [
        (FeaturesList::METHOD, "features.list"),
        (FeaturesInspect::METHOD, "features.inspect"),
        (FeaturesCreate::METHOD, "features.create"),
        (FeaturesUpdate::METHOD, "features.update"),
        (FeaturesRetire::METHOD, "features.retire"),
        (LinksList::METHOD, "links.list"),
        (LinksAdd::METHOD, "links.add"),
        (LinksRemove::METHOD, "links.remove"),
        (ReleasesList::METHOD, "releases.list"),
        (ReleasesInspect::METHOD, "releases.inspect"),
        (ReleasesCreate::METHOD, "releases.create"),
        (ReleasesUpdate::METHOD, "releases.update"),
        (ReleasesSetChangelog::METHOD, "releases.set_changelog"),
        (MilestonesCreate::METHOD, "milestones.create"),
        (MilestonesUpdate::METHOD, "milestones.update"),
        (DeploymentsList::METHOD, "deployments.list"),
        (DeploymentsInspect::METHOD, "deployments.inspect"),
        (DeploymentsCreate::METHOD, "deployments.create"),
        (DeploymentsUpdate::METHOD, "deployments.update"),
        (DeploymentsSetChangelog::METHOD, "deployments.set_changelog"),
        (FlagsList::METHOD, "flags.list"),
        (FlagsInspect::METHOD, "flags.inspect"),
        (FlagsCreate::METHOD, "flags.create"),
        (FlagsUpdate::METHOD, "flags.update"),
        (ReleaseFlagSettingsSet::METHOD, "releases.set_flag_settings"),
        (
            DeploymentFlagSettingsSet::METHOD,
            "deployments.set_flag_settings",
        ),
    ];
    for (actual, expected) in pairs {
        assert_eq!(actual, expected);
    }
}

#[test]
fn unknown_delivery_enum_variants_deserialize_as_unknown() {
    assert_eq!(
        serde_json::from_str::<FeatureStatusWire>("\"future\"").unwrap(),
        FeatureStatusWire::Unknown
    );
    assert_eq!(
        serde_json::from_str::<DeploymentStatusWire>("\"draining\"").unwrap(),
        DeploymentStatusWire::Unknown
    );
    assert_eq!(
        serde_json::from_str::<FlagValueTypeWire>("\"float\"").unwrap(),
        FlagValueTypeWire::Unknown
    );
    assert_eq!(
        serde_json::from_str::<LinkKindWire>("\"supersedes\"").unwrap(),
        LinkKindWire::Unknown
    );
}

#[test]
fn delivery_requests_refuse_unknown_fields() {
    fn refuses<T: serde::de::DeserializeOwned>(json: &str) {
        let known: Value = serde_json::from_str(json).unwrap();
        assert!(serde_json::from_value::<T>(known.clone()).is_ok(), "{json}");
        let mut extra = known;
        extra["unexpected"] = Value::Bool(true);
        assert!(serde_json::from_value::<T>(extra).is_err(), "{json}");
    }

    refuses::<FeaturesListParams>(r#"{"project":"p"}"#);
    refuses::<FeaturesCreateParams>(r#"{"command_id":"c","title":"Feature"}"#);
    refuses::<FeaturesUpdateParams>(r#"{"command_id":"c","expected_revision":1}"#);
    refuses::<FeaturesRetireParams>(r#"{"command_id":"c","expected_revision":1}"#);
    refuses::<LinksListParams>(r#"{"project":"p"}"#);
    refuses::<LinksAddParams>(
        r#"{"command_id":"c","source_kind":"task","source_id":"a","kind":"relates_to","target_kind":"feature","target_id":"b"}"#,
    );
    refuses::<LinksRemoveParams>(
        r#"{"command_id":"c","source_kind":"task","source_id":"a","link_id":"l"}"#,
    );
    refuses::<ReleasesCreateParams>(r#"{"command_id":"c","title":"v1"}"#);
    refuses::<ReleasesSetChangelogParams>(r#"{"command_id":"c","title":"Notes"}"#);
    refuses::<MilestonesCreateParams>(r#"{"command_id":"c","title":"M1"}"#);
    refuses::<DeploymentsCreateParams>(
        r#"{"command_id":"c","release_id":"r","environment":"prod"}"#,
    );
    refuses::<FlagsCreateParams>(
        r#"{"command_id":"c","key":"my-flag","value_type":"boolean","default_value":true}"#,
    );
    refuses::<ReleaseFlagSettingsSetParams>(r#"{"command_id":"c","expected_revision":1}"#);
    refuses::<DeploymentFlagSettingsSetParams>(r#"{"command_id":"c","expected_revision":1}"#);
}
