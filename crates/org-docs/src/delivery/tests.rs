//! Unit tests for pure delivery domain rules, paths, and YAML/Markdown codecs.

use serde_json::json;

use super::codecs::*;
use super::paths::*;
use super::records::*;
use super::rules::*;
use crate::project_tree::{render_project_index, DocPath, DocStatus, IndexDocRow, MediaFile};

const PID: &str = "0193a5c0-0000-7000-8000-000000000001";
const FID1: &str = "0193a5c0-0000-7000-8000-000000000011";
const FID2: &str = "0193a5c0-0000-7000-8000-000000000012";
const RID: &str = "0193a5c0-0000-7000-8000-000000000021";
const MID: &str = "0193a5c0-0000-7000-8000-000000000031";
const DID1: &str = "0193a5c0-0000-7000-8000-000000000041";
const DID2: &str = "0193a5c0-0000-7000-8000-000000000042";
const FLAG_ID: &str = "0193a5c0-0000-7000-8000-000000000051";
const EMP: &str = "0193a5c0-0000-7000-8000-000000000099";

fn meta() -> RecordMeta {
    RecordMeta {
        schema_version: 1,
        revision: 1,
        created_at: Some("2026-11-27T00:00:00Z".into()),
        created_by: ActorRef::Employee(EMP.into()),
        updated_at: Some("2026-11-27T00:00:00Z".into()),
        updated_by: ActorRef::Employee(EMP.into()),
    }
}

#[test]
fn feature_replacement_cycle_and_slug_validation() {
    assert!(is_valid_slug("auth-v2"));
    assert!(!is_valid_slug("-bad"));
    assert!(!is_valid_slug("bad--slug"));
    assert_eq!(slug_from_title("Hello, World! 2.0"), "hello-world-2-0");
    assert_eq!(feature_qualified_slug("core", "auth"), "core/auth");

    let mut f1 = FeatureRecord {
        id: FID1.into(),
        project_id: PID.into(),
        number: 1,
        local_slug: "old".into(),
        owner_employee_id: Some(EMP.into()),
        work_type: Some(FeatureWorkType::Build),
        title: "Old".into(),
        status: FeatureStatus::Retired,
        body_markdown: "old".into(),
        delivered_at: None,
        retired_reason: Some("superseded".into()),
        replaced_by_feature_id: Some(FID2.into()),
        meta: meta(),
    };
    let mut f2 = f1.clone();
    f2.id = FID2.into();
    f2.number = 2;
    f2.local_slug = "new".into();
    f2.replaced_by_feature_id = None;
    assert!(check_feature_replacement_acyclic(&[f1.clone(), f2.clone()]).is_ok());

    f2.replaced_by_feature_id = Some(FID1.into());
    assert!(check_feature_replacement_acyclic(&[f1.clone(), f2]).is_err());

    let encoded = encode_feature(&f1);
    let decoded = decode_feature(&encoded, PID, FID1).unwrap();
    assert_eq!(decoded, f1);

    f1.status = FeatureStatus::Live;
    f1.owner_employee_id = None;
    assert!(validate_feature_record(&f1).is_err());
}

#[test]
fn deployment_rollback_cycle_and_timestamps_guard() {
    let d1 = DeploymentRecord {
        id: DID1.into(),
        project_id: PID.into(),
        release_id: RID.into(),
        environment: "prod".into(),
        target: "primary".into(),
        operation: DeploymentOperation::Deploy,
        rollback_of_id: None,
        baseline_deployment_id: None,
        status: DeploymentStatus::Succeeded,
        started_at: Some("2026-11-27T01:00:00Z".into()),
        finished_at: Some("2026-11-27T01:05:00Z".into()),
        external_url: Some("https://ci.example/1".into()),
        body_markdown: "ok".into(),
        repository_commits: vec![],
        flag_settings: vec![],
        meta: meta(),
    };
    let mut d2 = d1.clone();
    d2.id = DID2.into();
    d2.operation = DeploymentOperation::Rollback;
    d2.rollback_of_id = Some(DID1.into());
    assert!(validate_deployment_record(&d1).is_ok());
    assert!(validate_deployment_record(&d2).is_ok());
    assert!(check_deployment_rollback_acyclic(&[d1.clone(), d2.clone()]).is_ok());

    let mut cycle = d1.clone();
    cycle.operation = DeploymentOperation::Rollback;
    cycle.rollback_of_id = Some(DID2.into());
    assert!(check_deployment_rollback_acyclic(&[cycle, d2]).is_err());

    let mut bad_time = d1;
    bad_time.finished_at = Some("2026-11-27T00:30:00Z".into());
    assert!(validate_deployment_record(&bad_time).is_err());
}

#[test]
fn flag_value_types_used_immutability_and_links() {
    assert!(validate_flag_value(FlagValueType::Boolean, &json!(true)).is_ok());
    assert!(validate_flag_value(FlagValueType::Boolean, &json!("true")).is_err());
    assert!(validate_flag_value(FlagValueType::Number, &json!(42)).is_ok());
    assert!(validate_flag_value(FlagValueType::Number, &json!("42")).is_err());
    assert!(validate_flag_value(FlagValueType::String, &json!("hi")).is_ok());
    assert!(validate_flag_value(FlagValueType::Json, &json!({"a": 1})).is_ok());
    assert!(validate_rollout_rules(Some(&json!({"pct": 50}))).is_ok());
    assert!(validate_rollout_rules(Some(&json!([1, 2]))).is_err());

    let flag = FeatureFlagRecord {
        id: FLAG_ID.into(),
        project_id: PID.into(),
        feature_id: Some(FID1.into()),
        key: "enable-cache".into(),
        owner_employee_id: Some(EMP.into()),
        status: FlagStatus::Active,
        value_type: FlagValueType::Boolean,
        default_value: json!(false),
        body_markdown: String::new(),
        meta: meta(),
    };
    let mut renamed = flag.clone();
    renamed.key = "enable-cache-v2".into();
    assert!(check_used_flag_identity_unchanged(&flag, &renamed, false).is_ok());
    assert!(check_used_flag_identity_unchanged(&flag, &renamed, true).is_err());

    let encoded = encode_flag(&flag);
    assert_eq!(decode_flag(&encoded, PID, FLAG_ID).unwrap(), flag);

    let ok_link = EntityLinkRecord {
        id: DID1.into(),
        project_id: PID.into(),
        source_kind: LinkEntityKind::Task,
        source_id: FID1.into(),
        kind: LinkKind::RelatesTo,
        target_kind: LinkEntityKind::Document,
        target_id: FID2.into(),
    };
    assert!(validate_entity_link(&ok_link).is_ok());
    let mut bad_link = ok_link.clone();
    bad_link.kind = LinkKind::Fixes;
    assert!(validate_entity_link(&bad_link).is_err());
}

#[test]
fn paths_progress_and_index_rendering_are_deterministic() {
    assert_eq!(
        parse_delivery_path(&milestone_path(PID, RID, MID)),
        Some(DeliveryPath::Milestone {
            project_id: PID.into(),
            release_id: RID.into(),
            milestone_id: MID.into(),
        })
    );
    assert!(matches!(
        parse_delivery_path(&format!("state/projects/{PID}/features/bad.txt")),
        Some(DeliveryPath::Foreign { .. })
    ));
    let m1 = compute_milestone_progress([true, true]);
    let m2 = compute_milestone_progress([true, false]);
    assert!(m1.is_done && !m2.is_done);
    let r = compute_release_progress(
        &[m1, m2],
        &[
            (FID1.into(), true),
            (FID1.into(), true),
            (FID2.into(), false),
        ],
    );
    assert_eq!(r.milestone_count, 2);
    assert_eq!(r.task_count, 2);
    assert_eq!(r.done_task_count, 1);
    assert!(!r.is_done);

    let p = DocPath::parse("guides/setup.md").unwrap();
    let idx = render_project_index(
        PID,
        &[IndexDocRow {
            id: FID1,
            path: &p,
            kind: "guide",
            title: "Setup",
            status: DocStatus::Draft,
            summary: None,
            owner_employee_id: None,
            revision: 1,
        }],
        &[],
    );
    let text = std::str::from_utf8(&idx).unwrap();
    assert!(text.contains("project_id:"));
    assert!(text.contains("guides/setup.md"));
    assert!(MediaFile::parse("../evil.png").is_err());
}
