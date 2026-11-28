//! Pass-2 dual-engine (`Memory` + `Sqlite`) and `Delete+Rebuild` hardening & parity tests
//! (`P0-2`, `P1-2`, `P1-3`, `P1-4`, `P2-3`, `P2-4`, `P2-7`).

use std::path::Path;

use org_docs::delivery::codecs::{encode_changelog, encode_counter, encode_link, encode_milestone};
use org_docs::delivery::paths::{
    deployment_changelog_path, feature_counter_path, link_path, milestone_path,
    release_changelog_path,
};
use org_docs::delivery::records::{
    ActorRef, EntityLinkRecord, LinkEntityKind, LinkKind, MilestoneRecord, NumberAllocation,
    NumberCounterRecord, RecordMeta, ScopedChangelogRecord,
};
use org_docs::project_tree::DocStatus;
use org_office::{Office, OfficeError};
use org_proto::delivery::{
    DeploymentFlagSettingInput, DeploymentFlagSettingsSetParams, DeploymentStatusWire,
    DeploymentsCreateParams, DeploymentsInspectParams, DeploymentsSetChangelogParams,
    DeploymentsUpdateParams, FeaturesCreateParams, FeaturesInspectParams, FlagValueTypeWire,
    FlagsCreateParams, LinkEntityKindWire, LinkKindWire, LinksAddParams, LinksRemoveParams,
    MilestonesCreateParams, MilestonesUpdateParams, ReleasesCreateParams,
    ReleasesSetChangelogParams,
};
use org_proto::docs::{DocStatusWire, DocsCreateParams, DocsDeleteParams, DocsIndexParams};
use org_proto::hiring::{AgentOffboard, AgentOffboardParams};
use org_proto::tasks::PatchField;
use org_proto::{ErrorCode, Outcome, Request};
use org_store::testkit::identity;
use serde_json::json;

use super::{
    blank_task_update, engines, err, next_uuid, ok, options, reopen, seed, task_create, Fx,
};

fn reopen_try(fx: Fx, tamper: impl FnOnce(&Path)) -> Result<Fx, OfficeError> {
    fx.office.sync_store().unwrap();
    let Fx {
        _tmp,
        folder,
        roots,
        custody,
        engine,
        office,
        owner,
        person,
        owning_team,
        project,
        other_project,
        repo,
        owning_agent,
        outsider_agent,
    } = fx;
    office.close().unwrap();
    tamper(&roots.durable);
    let runtime = roots.runtime.join(identity().office_id.as_str());
    if runtime.exists() {
        std::fs::remove_dir_all(&runtime).unwrap();
    }
    let office = Office::open_with(
        folder.clone(),
        None,
        identity().instance_id.as_str(),
        roots.clone(),
        custody.clone(),
        options(engine.clone()),
    )?;
    Ok(Fx {
        _tmp,
        folder,
        roots,
        custody,
        engine,
        office,
        owner,
        person,
        owning_team,
        project,
        other_project,
        repo,
        owning_agent,
        outsider_agent,
    })
}

fn sample_meta(emp: &str) -> RecordMeta {
    RecordMeta {
        schema_version: 1,
        revision: 1,
        created_at: Some("2026-11-27T10:00:00.000Z".into()),
        created_by: ActorRef::Employee(emp.to_owned()),
        updated_at: Some("2026-11-27T10:00:00.000Z".into()),
        updated_by: ActorRef::Employee(emp.to_owned()),
    }
}

fn write_durable(durable: &Path, rel: &str, bytes: &[u8]) {
    let full = durable.join(rel);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, bytes).unwrap();
}

fn offboard(fx: &Fx, agent_id: &str) {
    let version = fx
        .office
        .org_snapshot()
        .unwrap()
        .members
        .into_iter()
        .find(|m| m.id == agent_id)
        .unwrap()
        .version;
    let req = Request::new::<AgentOffboard>(
        1,
        &AgentOffboardParams {
            command_id: next_uuid(),
            member_id: agent_id.to_owned(),
            expected_version: version,
        },
    )
    .unwrap();
    let resp = fx
        .office
        .dispatch_hiring_rpc(Some(&fx.owner), &req)
        .unwrap();
    assert!(matches!(resp.outcome, Outcome::Result(_)), "{resp:?}");
}

#[test]
fn docs_delete_blocked_by_why_links_then_succeeds_after_unlink_on_both_engines() {
    for engine in engines() {
        let fx = seed(engine.clone());
        let p = &fx.project;
        let feat = ok(fx.office.features_create(
            &fx.owning_agent,
            FeaturesCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                title: "Linked Feature".into(),
                ..Default::default()
            },
        ))
        .feature;
        let doc_target = ok(fx.office.docs_create(
            &fx.owning_agent,
            DocsCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                kind: "adr".into(),
                title: "Target ADR".into(),
                path: Some("target-adr.md".into()),
                body_markdown: "Target doc.".into(),
                ..Default::default()
            },
        ))
        .document;
        let doc_source = ok(fx.office.docs_create(
            &fx.owning_agent,
            DocsCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                kind: "adr".into(),
                title: "Source ADR".into(),
                path: Some("source-adr.md".into()),
                body_markdown: "Source doc.".into(),
                ..Default::default()
            },
        ))
        .document;

        // Feature -> Document (target link) AND Document -> Feature (source link).
        let link_to_doc = ok(fx.office.links_add(
            &fx.owning_agent,
            LinksAddParams {
                command_id: next_uuid(),
                project: p.clone(),
                source_kind: LinkEntityKindWire::Feature,
                source_id: feat.id.clone(),
                kind: LinkKindWire::RelatesTo,
                target_kind: LinkEntityKindWire::Document,
                target_id: doc_target.id.clone(),
            },
        ))
        .link;
        let link_from_doc = ok(fx.office.links_add(
            &fx.owning_agent,
            LinksAddParams {
                command_id: next_uuid(),
                project: p.clone(),
                source_kind: LinkEntityKindWire::Document,
                source_id: doc_source.id.clone(),
                kind: LinkKindWire::RelatesTo,
                target_kind: LinkEntityKindWire::Feature,
                target_id: feat.id.clone(),
            },
        ))
        .link;

        // Deleting either linked document MUST refuse cleanly with Conflict naming the link ID.
        let blocked_target = err(fx.office.docs_delete(
            &fx.owning_agent,
            DocsDeleteParams {
                command_id: next_uuid(),
                project: p.clone(),
                document_id: doc_target.id.clone(),
                expected_revision: 1,
            },
        ));
        assert_eq!(blocked_target.code, ErrorCode::Conflict, "{engine:?}");
        assert!(
            blocked_target.message.contains(&link_to_doc.id),
            "{engine:?} missing link id in {}",
            blocked_target.message
        );

        let blocked_source = err(fx.office.docs_delete(
            &fx.owning_agent,
            DocsDeleteParams {
                command_id: next_uuid(),
                project: p.clone(),
                document_id: doc_source.id.clone(),
                expected_revision: 1,
            },
        ));
        assert_eq!(blocked_source.code, ErrorCode::Conflict, "{engine:?}");
        assert!(
            blocked_source.message.contains(&link_from_doc.id),
            "{engine:?} missing link id in {}",
            blocked_source.message
        );

        // Store stays healthy through Delete+Rebuild after refused deletes.
        let fx = reopen(fx);
        assert_eq!(
            ok(fx.office.docs_index(
                &fx.owner,
                DocsIndexParams {
                    project: fx.project.clone()
                }
            ))
            .documents
            .len(),
            2,
            "{engine:?}"
        );

        // Removing the links unblocks docs.delete cleanly on both engines + Delete+Rebuild.
        ok(fx.office.links_remove(
            &fx.owning_agent,
            LinksRemoveParams {
                command_id: next_uuid(),
                project: fx.project.clone(),
                source_kind: LinkEntityKindWire::Feature,
                source_id: feat.id,
                link_id: link_to_doc.id,
            },
        ));
        ok(fx.office.links_remove(
            &fx.owning_agent,
            LinksRemoveParams {
                command_id: next_uuid(),
                project: fx.project.clone(),
                source_kind: LinkEntityKindWire::Document,
                source_id: doc_source.id.clone(),
                link_id: link_from_doc.id,
            },
        ));
        ok(fx.office.docs_delete(
            &fx.owning_agent,
            DocsDeleteParams {
                command_id: next_uuid(),
                project: fx.project.clone(),
                document_id: doc_target.id,
                expected_revision: 1,
            },
        ));
        ok(fx.office.docs_delete(
            &fx.owning_agent,
            DocsDeleteParams {
                command_id: next_uuid(),
                project: fx.project.clone(),
                document_id: doc_source.id,
                expected_revision: 1,
            },
        ));
        let fx = reopen(fx);
        assert!(
            ok(fx.office.docs_index(
                &fx.owner,
                DocsIndexParams {
                    project: fx.project.clone()
                }
            ))
            .documents
            .is_empty(),
            "{engine:?}"
        );
        fx.office.close().unwrap();
    }
}

#[test]
fn timestamps_canonicalize_offsets_and_fractions_and_survive_rebuild_on_both_engines() {
    for engine in engines() {
        let fx = seed(engine.clone());
        let p = &fx.project;
        let feat = ok(fx.office.features_create(
            &fx.owning_agent,
            FeaturesCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                title: "Offset Feature".into(),
                delivered_at: Some("2026-11-27T12:00:00+02:00".into()),
                ..Default::default()
            },
        ))
        .feature;
        assert_eq!(
            feat.delivered_at.as_deref(),
            Some("2026-11-27T10:00:00.000Z"),
            "{engine:?}"
        );
        let rel = ok(fx.office.releases_create(
            &fx.owning_agent,
            ReleasesCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                title: "Offset Release".into(),
                ..Default::default()
            },
        ))
        .release;
        let flag = ok(fx.office.flags_create(
            &fx.owning_agent,
            FlagsCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                key: "offset-flag".into(),
                value_type: FlagValueTypeWire::Boolean,
                default_value: json!(false),
                feature_id: Some(feat.id.clone()),
                owner_employee_id: Some(fx.owning_agent.clone()),
                status: None,
                body_markdown: String::new(),
            },
        ))
        .flag;

        // Offset (+02:00) started_at is earlier in UTC than Z finished_at even though
        // lexicographically "10:05:00Z" < "12:00:00+02:00".
        let dep_offset = ok(fx.office.deployments_create(
            &fx.owning_agent,
            DeploymentsCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                release_id: rel.id.clone(),
                environment: "prod".into(),
                status: Some(DeploymentStatusWire::Succeeded),
                started_at: Some("2026-11-27T12:00:00+02:00".into()),
                finished_at: Some("2026-11-27T10:05:00Z".into()),
                ..Default::default()
            },
        ))
        .deployment;
        assert_eq!(
            dep_offset.started_at.as_deref(),
            Some("2026-11-27T10:00:00.000Z"),
            "{engine:?}"
        );
        assert_eq!(
            dep_offset.finished_at.as_deref(),
            Some("2026-11-27T10:05:00.000Z"),
            "{engine:?}"
        );

        // Fractional second (.500Z) > whole second (Z) even though '.' < 'Z' in ASCII.
        let dep_frac = ok(fx.office.deployments_create(
            &fx.owning_agent,
            DeploymentsCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                release_id: rel.id.clone(),
                environment: "staging".into(),
                status: Some(DeploymentStatusWire::Succeeded),
                started_at: Some("2026-11-27T10:00:00Z".into()),
                finished_at: Some("2026-11-27T10:00:00.500Z".into()),
                ..Default::default()
            },
        ))
        .deployment;
        assert_eq!(
            dep_frac.started_at.as_deref(),
            Some("2026-11-27T10:00:00.000Z"),
            "{engine:?}"
        );
        assert_eq!(
            dep_frac.finished_at.as_deref(),
            Some("2026-11-27T10:00:00.500Z"),
            "{engine:?}"
        );

        // Update with offset timestamp where finished_at is earlier in UTC must fail on both.
        let bad_order = err(fx.office.deployments_update(
            &fx.owning_agent,
            DeploymentsUpdateParams {
                command_id: next_uuid(),
                project: p.clone(),
                deployment_id: dep_offset.id.clone(),
                expected_revision: 1,
                started_at: Some("2026-11-27T10:05:00Z".into()),
                finished_at: Some("2026-11-27T12:00:00+02:00".into()),
                ..Default::default()
            },
        ));
        assert_eq!(bad_order.code, ErrorCode::InvalidRequest, "{engine:?}");

        // applied_at canonicalizes offsets & fractions identically.
        let applied = ok(fx.office.deployments_set_flag_settings(
            &fx.owning_agent,
            DeploymentFlagSettingsSetParams {
                command_id: next_uuid(),
                project: p.clone(),
                deployment_id: dep_offset.id.clone(),
                expected_revision: 1,
                settings: vec![DeploymentFlagSettingInput {
                    flag_id: flag.id,
                    value: json!(true),
                    rollout_rules: None,
                    applied_at: Some("2026-11-27T12:02:30.5+02:00".into()),
                }],
            },
        ));
        assert_eq!(
            applied.flag_settings[0].applied_at.as_deref(),
            Some("2026-11-27T10:02:30.500Z"),
            "{engine:?}"
        );

        let fx = reopen(fx);
        let inspected = ok(fx.office.deployments_inspect(
            &fx.owning_agent,
            DeploymentsInspectParams {
                project: fx.project.clone(),
                deployment_id: dep_offset.id,
            },
        ));
        assert_eq!(
            inspected.deployment.started_at.as_deref(),
            Some("2026-11-27T10:00:00.000Z")
        );
        assert_eq!(
            inspected.flag_settings[0].applied_at.as_deref(),
            Some("2026-11-27T10:02:30.500Z")
        );
        let feat_after = ok(fx.office.features_inspect(
            &fx.owning_agent,
            FeaturesInspectParams {
                project: fx.project.clone(),
                feature: feat.id,
            },
        ));
        assert_eq!(
            feat_after.feature.delivered_at.as_deref(),
            Some("2026-11-27T10:00:00.000Z")
        );
        fx.office.close().unwrap();
    }
}

#[test]
fn rebuild_parity_rejects_bad_task_refs_orphans_duplicates_and_misfiled_links_on_both_engines() {
    for engine in engines() {
        // 1. Task with dangling feature_id, milestone_id, and cross-project affects_feature_id.
        for tamper_kind in ["bad_feature", "bad_milestone", "bad_affects_feature"] {
            let fx = seed(engine.clone());
            let other_feat = ok(fx.office.features_create(
                &fx.owner,
                FeaturesCreateParams {
                    command_id: next_uuid(),
                    project: fx.other_project.clone(),
                    title: "Other Proj Feat".into(),
                    ..Default::default()
                },
            ))
            .feature;
            let good_feat = ok(fx.office.features_create(
                &fx.owner,
                FeaturesCreateParams {
                    command_id: next_uuid(),
                    project: fx.project.clone(),
                    title: "Good Feat".into(),
                    ..Default::default()
                },
            ))
            .feature;
            let rel = ok(fx.office.releases_create(
                &fx.owner,
                ReleasesCreateParams {
                    command_id: next_uuid(),
                    project: fx.project.clone(),
                    title: "Rel".into(),
                    ..Default::default()
                },
            ))
            .release;
            let ms = ok(fx.office.milestones_create(
                &fx.owner,
                MilestonesCreateParams {
                    command_id: next_uuid(),
                    project: fx.project.clone(),
                    release_id: rel.id,
                    title: "M1".into(),
                    body_markdown: String::new(),
                },
            ))
            .milestone;
            let tid = task_create(&fx, "Bug", "bug", &fx.project, Some(good_feat.id.clone()));
            let mut up = blank_task_update(&tid, 1);
            up.feature_id = PatchField::Set(Some(good_feat.id.clone()));
            up.milestone_id = PatchField::Set(Some(ms.id.clone()));
            ok(fx.office.task_update(&fx.owner, up));

            let task_rel = format!("state/projects/{}/work/tasks/{tid}/task.md", fx.project);
            let missing_id = next_uuid();
            let other_id = other_feat.id.clone();
            let good_id = good_feat.id.clone();
            let ms_id = ms.id.clone();
            let res = reopen_try(fx, move |durable| {
                let p = durable.join(&task_rel);
                let text = std::fs::read_to_string(&p).unwrap();
                let (key, old_val, new_val) = match tamper_kind {
                    "bad_feature" => ("feature_id:", good_id.as_str(), missing_id.as_str()),
                    "bad_milestone" => ("milestone_id:", ms_id.as_str(), missing_id.as_str()),
                    _ => ("details:", good_id.as_str(), other_id.as_str()),
                };
                let mut replaced = false;
                let edited: String = text
                    .split_inclusive('\n')
                    .map(|line| {
                        if !replaced && line.trim_start().starts_with(key) && line.contains(old_val)
                        {
                            replaced = true;
                            line.replacen(old_val, new_val, 1)
                        } else {
                            line.to_owned()
                        }
                    })
                    .collect();
                assert!(replaced, "{tamper_kind} tamper did not match in:\n{text}");
                std::fs::write(&p, edited).unwrap();
            });
            assert!(
                res.is_err(),
                "{engine:?} {tamper_kind}: rebuild must fail closed"
            );
        }

        // 2. Orphan milestone, orphan release changelog, and orphan deployment changelog.
        for orphan_kind in ["milestone", "rel_changelog", "dep_changelog"] {
            let fx = seed(engine.clone());
            let pid = fx.project.clone();
            let owner = fx.owner.clone();
            let missing_parent = next_uuid();
            let item_id = next_uuid();
            let res = reopen_try(fx, move |durable| match orphan_kind {
                "milestone" => {
                    let rec = MilestoneRecord {
                        id: item_id.clone(),
                        project_id: pid.clone(),
                        release_id: missing_parent.clone(),
                        number: 1,
                        title: "Orphan M".into(),
                        body_markdown: String::new(),
                        meta: sample_meta(&owner),
                    };
                    write_durable(
                        durable,
                        &milestone_path(&pid, &missing_parent, &item_id),
                        &encode_milestone(&rec),
                    );
                }
                "rel_changelog" => {
                    let rec = ScopedChangelogRecord {
                        id: item_id,
                        project_id: pid.clone(),
                        release_id: Some(missing_parent.clone()),
                        deployment_id: None,
                        title: "Orphan Rel CL".into(),
                        status: DocStatus::Draft,
                        owner_employee_id: None,
                        body_markdown: String::new(),
                        meta: sample_meta(&owner),
                    };
                    write_durable(
                        durable,
                        &release_changelog_path(&pid, &missing_parent),
                        &encode_changelog(&rec),
                    );
                }
                _ => {
                    let rec = ScopedChangelogRecord {
                        id: item_id,
                        project_id: pid.clone(),
                        release_id: None,
                        deployment_id: Some(missing_parent.clone()),
                        title: "Orphan Dep CL".into(),
                        status: DocStatus::Draft,
                        owner_employee_id: None,
                        body_markdown: String::new(),
                        meta: sample_meta(&owner),
                    };
                    write_durable(
                        durable,
                        &deployment_changelog_path(&pid, &missing_parent),
                        &encode_changelog(&rec),
                    );
                }
            });
            assert!(
                res.is_err(),
                "{engine:?} orphan {orphan_kind}: rebuild must fail closed"
            );
        }

        // 3. Duplicate counter allocations fail closed on rebuild.
        {
            let fx = seed(engine.clone());
            let pid = fx.project.clone();
            let f1 = ok(fx.office.features_create(
                &fx.owner,
                FeaturesCreateParams {
                    command_id: next_uuid(),
                    project: pid.clone(),
                    title: "F1".into(),
                    ..Default::default()
                },
            ))
            .feature;
            let dup_ctr = NumberCounterRecord {
                next_number: 3,
                allocations: vec![
                    NumberAllocation {
                        number: 1,
                        entity_id: f1.id.clone(),
                        allocated_at: "2026-11-27T10:00:00.000Z".into(),
                    },
                    NumberAllocation {
                        number: 2,
                        entity_id: f1.id,
                        allocated_at: "2026-11-27T10:00:00.000Z".into(),
                    },
                ],
            };
            let res = reopen_try(fx, move |durable| {
                write_durable(
                    durable,
                    &feature_counter_path(&pid),
                    &encode_counter(&dup_ctr),
                );
            });
            assert!(
                res.is_err(),
                "{engine:?} duplicate counter entity_id must fail closed on rebuild"
            );
        }

        // 4. Misfiled link under project A referencing task & doc that belong to project B.
        {
            let fx = seed(engine.clone());
            let other_team = fx
                .office
                .org_snapshot()
                .unwrap()
                .teams
                .into_iter()
                .find(|t| t.name == "Finance")
                .unwrap()
                .id;
            let task_b = next_uuid();
            ok(fx.office.task_create(
                &fx.owner,
                org_proto::tasks::TasksCreateParams {
                    command_id: next_uuid(),
                    task_id: task_b.clone(),
                    team_id: Some(other_team),
                    assigned_team_id: None,
                    project_id: Some(fx.other_project.clone()),
                    task_type: Some("task".into()),
                    work_type: None,
                    title: "Billing Task".into(),
                    body_markdown: String::new(),
                    assignee_id: None,
                    priority: None,
                    effort: None,
                    severity: None,
                    rank_order: None,
                    labels: vec![],
                    reference_files: vec![],
                    modified_files: vec![],
                    depends_on: vec![],
                    details: None,
                    dod: None,
                    attachment_ids: vec![],
                },
            ));
            let doc_b = ok(fx.office.docs_create(
                &fx.owner,
                DocsCreateParams {
                    command_id: next_uuid(),
                    project: fx.other_project.clone(),
                    kind: "guide".into(),
                    title: "Billing Doc".into(),
                    ..Default::default()
                },
            ))
            .document;
            let project_a = fx.project.clone();
            let misfiled = EntityLinkRecord {
                id: next_uuid(),
                project_id: project_a.clone(),
                source_kind: LinkEntityKind::Task,
                source_id: task_b,
                kind: LinkKind::RelatesTo,
                target_kind: LinkEntityKind::Document,
                target_id: doc_b.id,
            };
            let res = reopen_try(fx, move |durable| {
                write_durable(
                    durable,
                    &link_path(
                        &project_a,
                        misfiled.source_kind,
                        &misfiled.source_id,
                        &misfiled.id,
                    ),
                    &encode_link(&misfiled),
                );
            });
            assert!(
                res.is_err(),
                "{engine:?} misfiled link under project A for project B task must fail closed"
            );
        }
    }
}

#[test]
fn milestone_release_guard_changelog_owner_active_owner_and_counter_timestamps_on_both_engines() {
    for engine in engines() {
        let fx = seed(engine.clone());
        let p = &fx.project;

        // P1-4: milestones.update rejects mismatched release_id with NotFound on both engines.
        let r1 = ok(fx.office.releases_create(
            &fx.owning_agent,
            ReleasesCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                title: "Release One".into(),
                ..Default::default()
            },
        ))
        .release;
        let r2 = ok(fx.office.releases_create(
            &fx.owning_agent,
            ReleasesCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                title: "Release Two".into(),
                ..Default::default()
            },
        ))
        .release;
        let m1 = ok(fx.office.milestones_create(
            &fx.owning_agent,
            MilestonesCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                release_id: r1.id.clone(),
                title: "Milestone 1".into(),
                body_markdown: String::new(),
            },
        ))
        .milestone;
        let wrong_rel = err(fx.office.milestones_update(
            &fx.owning_agent,
            MilestonesUpdateParams {
                command_id: next_uuid(),
                project: p.clone(),
                release_id: Some(r2.id),
                milestone_id: m1.id.clone(),
                expected_revision: 1,
                title: Some("Hijacked".into()),
                body_markdown: None,
            },
        ));
        assert_eq!(wrong_rel.code, ErrorCode::NotFound, "{engine:?}");
        ok(fx.office.milestones_update(
            &fx.owning_agent,
            MilestonesUpdateParams {
                command_id: next_uuid(),
                project: p.clone(),
                release_id: Some(r1.id.clone()),
                milestone_id: m1.id,
                expected_revision: 1,
                title: Some("Valid Rename".into()),
                body_markdown: None,
            },
        ));

        // P2-3: clearing owner_employee_id ("") on Current release/deployment changelog is refused.
        ok(fx.office.releases_set_changelog(
            &fx.owning_agent,
            ReleasesSetChangelogParams {
                command_id: next_uuid(),
                project: p.clone(),
                release_id: r1.id.clone(),
                title: "Rel CL".into(),
                body_markdown: "Notes".into(),
                status: Some(DocStatusWire::Current),
                owner_employee_id: Some(fx.owning_agent.clone()),
                expected_revision: None,
            },
        ));
        let clear_rel_cl = err(fx.office.releases_set_changelog(
            &fx.owner,
            ReleasesSetChangelogParams {
                command_id: next_uuid(),
                project: p.clone(),
                release_id: r1.id.clone(),
                title: "Rel CL".into(),
                body_markdown: "Notes".into(),
                status: Some(DocStatusWire::Current),
                owner_employee_id: Some(String::new()),
                expected_revision: Some(1),
            },
        ));
        assert_eq!(clear_rel_cl.code, ErrorCode::InvalidRequest, "{engine:?}");

        let dep = ok(fx.office.deployments_create(
            &fx.owning_agent,
            DeploymentsCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                release_id: r1.id.clone(),
                environment: "prod".into(),
                ..Default::default()
            },
        ))
        .deployment;
        ok(fx.office.deployments_set_changelog(
            &fx.owning_agent,
            DeploymentsSetChangelogParams {
                command_id: next_uuid(),
                project: p.clone(),
                deployment_id: dep.id.clone(),
                title: "Dep CL".into(),
                body_markdown: "Notes".into(),
                status: Some(DocStatusWire::Current),
                owner_employee_id: Some(fx.owning_agent.clone()),
                expected_revision: None,
            },
        ));
        let clear_dep_cl = err(fx.office.deployments_set_changelog(
            &fx.owner,
            DeploymentsSetChangelogParams {
                command_id: next_uuid(),
                project: p.clone(),
                deployment_id: dep.id,
                title: "Dep CL".into(),
                body_markdown: "Notes".into(),
                status: Some(DocStatusWire::Current),
                owner_employee_id: Some("   ".into()),
                expected_revision: Some(1),
            },
        ));
        assert_eq!(clear_dep_cl.code, ErrorCode::InvalidRequest, "{engine:?}");

        // P2-4: inactive (offboarded) employee cannot be assigned as owner_employee_id.
        offboard(&fx, &fx.outsider_agent);
        let inactive_feat = err(fx.office.features_create(
            &fx.owner,
            FeaturesCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                title: "Inactive Owner".into(),
                owner_employee_id: Some(fx.outsider_agent.clone()),
                ..Default::default()
            },
        ));
        assert_eq!(inactive_feat.code, ErrorCode::InvalidRequest, "{engine:?}");
        let inactive_flag = err(fx.office.flags_create(
            &fx.owner,
            FlagsCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                key: "inactive-owner-flag".into(),
                value_type: FlagValueTypeWire::Boolean,
                default_value: json!(false),
                feature_id: None,
                owner_employee_id: Some(fx.outsider_agent.clone()),
                status: None,
                body_markdown: String::new(),
            },
        ));
        assert_eq!(inactive_flag.code, ErrorCode::InvalidRequest, "{engine:?}");

        // P2-7: real allocated_at persisted in _counter.yaml (never 1970 epoch placeholder).
        ok(fx.office.features_create(
            &fx.owning_agent,
            FeaturesCreateParams {
                command_id: next_uuid(),
                project: p.clone(),
                title: "Counter Check".into(),
                ..Default::default()
            },
        ));
        fx.office.sync_store().unwrap();
        let ctr_text =
            std::fs::read_to_string(fx.roots.durable.join(feature_counter_path(p))).unwrap();
        assert!(ctr_text.contains("\"allocated_at\":"), "{ctr_text}");
        assert!(
            !ctr_text.contains("1970-01-01T00:00:00"),
            "counter must persist real allocated_at, got: {ctr_text}"
        );
        let fx = reopen(fx);
        fx.office.close().unwrap();
    }
}
