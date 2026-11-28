//! Engine-independent projector for `state/projects/<id>/{features,releases,deployments,flags,links}/**`.

use std::collections::BTreeSet;

use org_docs::delivery::codecs::{
    decode_changelog, decode_counter, decode_deployment, decode_feature, decode_flag, decode_link,
    decode_milestone, decode_release,
};
use org_docs::delivery::ops::DeliveryStoreView;
use org_docs::delivery::paths::{parse_delivery_path, DeliveryPath};
use org_docs::delivery::validate::validate_project_delivery;
use org_store::{CoreRead, DurablePath, Engine, Id, ProjectionError, Projector};

use super::features::StoreViewAdapter;
use super::ports::{DeliveryRead, DeliveryWrite};

/// Projects canonical delivery files under `state/projects/<id>/`.
pub struct DeliveryFiles;

fn refuse(path: &DurablePath, reason: impl Into<String>) -> ProjectionError {
    ProjectionError::new(path, reason)
}

fn parse_id(path: &DurablePath, raw: &str) -> Result<Id, ProjectionError> {
    Id::parse(raw).map_err(|_| refuse(path, "invalid UUIDv7"))
}

fn put_ctr<Tx: DeliveryWrite>(
    tx: &mut Tx,
    path: &DurablePath,
    sk: &str,
    sid: &str,
    ek: &str,
    bytes: Option<&[u8]>,
) -> Result<(), ProjectionError> {
    let id = parse_id(path, sid)?;
    match bytes {
        Some(b) => {
            let rec = decode_counter(b).map_err(|e| refuse(path, e.to_string()))?;
            Ok(tx.put_delivery_counter(sk, &id, ek, &rec)?)
        }
        None => Ok(tx.remove_delivery_counter(sk, &id, ek)?),
    }
}

impl<E: Engine> Projector<E> for DeliveryFiles
where
    E::Tx: DeliveryWrite,
{
    fn owns(&self, path: &DurablePath) -> bool {
        parse_delivery_path(path.as_str()).is_some()
    }

    fn apply(
        &self,
        tx: &mut E::Tx,
        path: &DurablePath,
        bytes: Option<&[u8]>,
    ) -> Result<(), ProjectionError> {
        let Some(kind) = parse_delivery_path(path.as_str()) else {
            return Err(refuse(path, "not a delivery file"));
        };
        match bytes {
            Some(b) => apply_put(tx, path, kind, b),
            None => apply_remove(tx, path, kind),
        }
    }

    fn validate(&self, tx: &E::Tx) -> Result<(), ProjectionError> {
        validate_all(tx)
    }
}

fn apply_put<Tx: DeliveryWrite>(
    tx: &mut Tx,
    path: &DurablePath,
    kind: DeliveryPath,
    b: &[u8],
) -> Result<(), ProjectionError> {
    let dec = |e: org_docs::delivery::rules::RuleError| refuse(path, e.to_string());
    match kind {
        DeliveryPath::Foreign { .. } => Err(ProjectionError::skippable(
            path,
            "unrecognized file inside project delivery directory",
        )),
        DeliveryPath::Feature {
            project_id,
            feature_id,
        } => Ok(tx.put_feature(&decode_feature(b, &project_id, &feature_id).map_err(dec)?)?),
        DeliveryPath::FeatureCounter { project_id } => {
            put_ctr(tx, path, "project", &project_id, "feature", Some(b))
        }
        DeliveryPath::Release {
            project_id,
            release_id,
        } => Ok(tx.put_release(&decode_release(b, &project_id, &release_id).map_err(dec)?)?),
        DeliveryPath::ReleaseCounter { project_id } => {
            put_ctr(tx, path, "project", &project_id, "release", Some(b))
        }
        DeliveryPath::ReleaseChangelog {
            project_id,
            release_id,
        } => Ok(tx.put_scoped_changelog(
            &decode_changelog(b, &project_id, Some(&release_id), None).map_err(dec)?,
        )?),
        DeliveryPath::Milestone {
            project_id,
            release_id,
            milestone_id,
        } => Ok(tx.put_milestone(
            &decode_milestone(b, &project_id, &release_id, &milestone_id).map_err(dec)?,
        )?),
        DeliveryPath::MilestoneCounter { release_id, .. } => {
            put_ctr(tx, path, "release", &release_id, "milestone", Some(b))
        }
        DeliveryPath::Deployment {
            project_id,
            deployment_id,
        } => {
            Ok(tx
                .put_deployment(&decode_deployment(b, &project_id, &deployment_id).map_err(dec)?)?)
        }
        DeliveryPath::DeploymentChangelog {
            project_id,
            deployment_id,
        } => Ok(tx.put_scoped_changelog(
            &decode_changelog(b, &project_id, None, Some(&deployment_id)).map_err(dec)?,
        )?),
        DeliveryPath::Flag {
            project_id,
            flag_id,
        } => Ok(tx.put_flag(&decode_flag(b, &project_id, &flag_id).map_err(dec)?)?),
        DeliveryPath::Link {
            project_id,
            source_kind,
            source_id,
            link_id,
        } => Ok(tx.put_entity_link(
            &decode_link(b, &project_id, source_kind, &source_id, &link_id).map_err(dec)?,
        )?),
    }
}

fn apply_remove<Tx: DeliveryWrite>(
    tx: &mut Tx,
    path: &DurablePath,
    kind: DeliveryPath,
) -> Result<(), ProjectionError> {
    match kind {
        DeliveryPath::Foreign { .. } => Ok(()),
        DeliveryPath::Feature { feature_id, .. } => {
            Ok(tx.remove_feature(&parse_id(path, &feature_id)?)?)
        }
        DeliveryPath::FeatureCounter { project_id } => {
            put_ctr(tx, path, "project", &project_id, "feature", None)
        }
        DeliveryPath::Release { release_id, .. } => {
            Ok(tx.remove_release(&parse_id(path, &release_id)?)?)
        }
        DeliveryPath::ReleaseCounter { project_id } => {
            put_ctr(tx, path, "project", &project_id, "release", None)
        }
        DeliveryPath::ReleaseChangelog { release_id, .. } => {
            Ok(tx.remove_release_changelog(&parse_id(path, &release_id)?)?)
        }
        DeliveryPath::Milestone { milestone_id, .. } => {
            Ok(tx.remove_milestone(&parse_id(path, &milestone_id)?)?)
        }
        DeliveryPath::MilestoneCounter { release_id, .. } => {
            put_ctr(tx, path, "release", &release_id, "milestone", None)
        }
        DeliveryPath::Deployment { deployment_id, .. } => {
            Ok(tx.remove_deployment(&parse_id(path, &deployment_id)?)?)
        }
        DeliveryPath::DeploymentChangelog { deployment_id, .. } => {
            Ok(tx.remove_deployment_changelog(&parse_id(path, &deployment_id)?)?)
        }
        DeliveryPath::Flag { flag_id, .. } => Ok(tx.remove_flag(&parse_id(path, &flag_id)?)?),
        DeliveryPath::Link {
            source_kind,
            source_id,
            link_id,
            ..
        } => Ok(tx.remove_entity_link(
            source_kind,
            &parse_id(path, &source_id)?,
            &parse_id(path, &link_id)?,
        )?),
    }
}

fn validate_source_paths<T: DeliveryRead + CoreRead>(
    tx: &T,
    adapter: &StoreViewAdapter<'_, T>,
    existing: &BTreeSet<Id>,
    projects: &mut BTreeSet<Id>,
) -> Result<(), ProjectionError> {
    let root = DurablePath::parse("state/projects").expect("fixed projects root");
    let mut seen_links = BTreeSet::new();
    for path in tx.sources_under(&root)? {
        let Some(kind) = parse_delivery_path(path.as_str()) else {
            continue;
        };
        match kind {
            DeliveryPath::Link {
                project_id,
                source_kind,
                source_id,
                link_id,
            } => {
                let pid = parse_id(&path, &project_id)?;
                projects.insert(pid.clone());
                if !existing.contains(&pid) {
                    return Err(refuse(
                        &path,
                        "link belongs to a project that does not exist",
                    ));
                }
                if !seen_links.insert((source_kind, source_id.clone(), link_id)) {
                    return Err(refuse(&path, "duplicate link id across project paths"));
                }
                let ok = adapter
                    .entity_exists_in_project(&project_id, source_kind, &source_id)
                    .map_err(|e| refuse(&path, e))?;
                if !ok {
                    return Err(refuse(
                        &path,
                        "link source entity does not belong to file path project",
                    ));
                }
            }
            DeliveryPath::MilestoneCounter {
                project_id,
                release_id,
            } => {
                let pid = parse_id(&path, &project_id)?;
                projects.insert(pid.clone());
                let rel_ok = tx
                    .release(&parse_id(&path, &release_id)?)?
                    .is_some_and(|r| r.project_id == project_id);
                if !existing.contains(&pid) || !rel_ok {
                    return Err(refuse(
                        &path,
                        "milestone counter references unknown release",
                    ));
                }
            }
            DeliveryPath::FeatureCounter { project_id }
            | DeliveryPath::ReleaseCounter { project_id } => {
                let pid = parse_id(&path, &project_id)?;
                projects.insert(pid.clone());
                if !existing.contains(&pid) {
                    return Err(refuse(
                        &path,
                        "counter belongs to a project that does not exist",
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_all<T: DeliveryRead + CoreRead>(tx: &T) -> Result<(), ProjectionError> {
    let existing: BTreeSet<Id> = tx.projects()?.into_iter().map(|p| p.id).collect();
    let mut projects = existing.clone();
    projects.extend(tx.projects_with_delivery()?);
    let adapter = StoreViewAdapter(tx);
    validate_source_paths(tx, &adapter, &existing, &mut projects)?;
    for project in &projects {
        validate_project_delivery(&adapter, project.as_str(), existing.contains(project)).map_err(
            |e| {
                let dp = DurablePath::parse(&e.path).unwrap_or_else(|_| {
                    DurablePath::parse(&format!("state/projects/{}/features", project.as_str()))
                        .expect("valid project path")
                });
                refuse(&dp, e.reason)
            },
        )?;
    }
    Ok(())
}
