//! Engine-independent projector for `state/projects/<id>/{features,releases,deployments,flags,links}/**`.

use std::collections::BTreeSet;

use org_docs::delivery::codecs::{
    decode_changelog, decode_counter, decode_deployment, decode_feature, decode_flag, decode_link,
    decode_milestone, decode_release,
};
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
        match (kind, bytes) {
            (DeliveryPath::Foreign { .. }, Some(_)) => Err(ProjectionError::skippable(
                path,
                "unrecognized file inside project delivery directory",
            )),
            (DeliveryPath::Foreign { .. }, None) => Ok(()),
            (
                DeliveryPath::Feature {
                    project_id,
                    feature_id,
                },
                Some(b),
            ) => {
                let rec = decode_feature(b, &project_id, &feature_id)
                    .map_err(|e| refuse(path, e.to_string()))?;
                Ok(tx.put_feature(&rec)?)
            }
            (DeliveryPath::Feature { feature_id, .. }, None) => {
                Ok(tx.remove_feature(&parse_id(path, &feature_id)?)?)
            }
            (DeliveryPath::FeatureCounter { project_id }, Some(b)) => {
                let rec = decode_counter(b).map_err(|e| refuse(path, e.to_string()))?;
                Ok(tx.put_delivery_counter(
                    "project",
                    &parse_id(path, &project_id)?,
                    "feature",
                    &rec,
                )?)
            }
            (DeliveryPath::FeatureCounter { project_id }, None) => Ok(tx.remove_delivery_counter(
                "project",
                &parse_id(path, &project_id)?,
                "feature",
            )?),
            (
                DeliveryPath::Release {
                    project_id,
                    release_id,
                },
                Some(b),
            ) => {
                let rec = decode_release(b, &project_id, &release_id)
                    .map_err(|e| refuse(path, e.to_string()))?;
                Ok(tx.put_release(&rec)?)
            }
            (DeliveryPath::Release { release_id, .. }, None) => {
                Ok(tx.remove_release(&parse_id(path, &release_id)?)?)
            }
            (DeliveryPath::ReleaseCounter { project_id }, Some(b)) => {
                let rec = decode_counter(b).map_err(|e| refuse(path, e.to_string()))?;
                Ok(tx.put_delivery_counter(
                    "project",
                    &parse_id(path, &project_id)?,
                    "release",
                    &rec,
                )?)
            }
            (DeliveryPath::ReleaseCounter { project_id }, None) => Ok(tx.remove_delivery_counter(
                "project",
                &parse_id(path, &project_id)?,
                "release",
            )?),
            (
                DeliveryPath::ReleaseChangelog {
                    project_id,
                    release_id,
                },
                Some(b),
            ) => {
                let rec = decode_changelog(b, &project_id, Some(&release_id), None)
                    .map_err(|e| refuse(path, e.to_string()))?;
                Ok(tx.put_scoped_changelog(&rec)?)
            }
            (DeliveryPath::ReleaseChangelog { release_id, .. }, None) => {
                Ok(tx.remove_release_changelog(&parse_id(path, &release_id)?)?)
            }
            (
                DeliveryPath::Milestone {
                    project_id,
                    release_id,
                    milestone_id,
                },
                Some(b),
            ) => {
                let rec = decode_milestone(b, &project_id, &release_id, &milestone_id)
                    .map_err(|e| refuse(path, e.to_string()))?;
                Ok(tx.put_milestone(&rec)?)
            }
            (DeliveryPath::Milestone { milestone_id, .. }, None) => {
                Ok(tx.remove_milestone(&parse_id(path, &milestone_id)?)?)
            }
            (DeliveryPath::MilestoneCounter { release_id, .. }, Some(b)) => {
                let rec = decode_counter(b).map_err(|e| refuse(path, e.to_string()))?;
                Ok(tx.put_delivery_counter(
                    "release",
                    &parse_id(path, &release_id)?,
                    "milestone",
                    &rec,
                )?)
            }
            (DeliveryPath::MilestoneCounter { release_id, .. }, None) => Ok(
                tx.remove_delivery_counter("release", &parse_id(path, &release_id)?, "milestone")?
            ),
            (
                DeliveryPath::Deployment {
                    project_id,
                    deployment_id,
                },
                Some(b),
            ) => {
                let rec = decode_deployment(b, &project_id, &deployment_id)
                    .map_err(|e| refuse(path, e.to_string()))?;
                Ok(tx.put_deployment(&rec)?)
            }
            (DeliveryPath::Deployment { deployment_id, .. }, None) => {
                Ok(tx.remove_deployment(&parse_id(path, &deployment_id)?)?)
            }
            (
                DeliveryPath::DeploymentChangelog {
                    project_id,
                    deployment_id,
                },
                Some(b),
            ) => {
                let rec = decode_changelog(b, &project_id, None, Some(&deployment_id))
                    .map_err(|e| refuse(path, e.to_string()))?;
                Ok(tx.put_scoped_changelog(&rec)?)
            }
            (DeliveryPath::DeploymentChangelog { deployment_id, .. }, None) => {
                Ok(tx.remove_deployment_changelog(&parse_id(path, &deployment_id)?)?)
            }
            (
                DeliveryPath::Flag {
                    project_id,
                    flag_id,
                },
                Some(b),
            ) => {
                let rec = decode_flag(b, &project_id, &flag_id)
                    .map_err(|e| refuse(path, e.to_string()))?;
                Ok(tx.put_flag(&rec)?)
            }
            (DeliveryPath::Flag { flag_id, .. }, None) => {
                Ok(tx.remove_flag(&parse_id(path, &flag_id)?)?)
            }
            (
                DeliveryPath::Link {
                    project_id,
                    source_kind,
                    source_id,
                    link_id,
                },
                Some(b),
            ) => {
                let rec = decode_link(b, &project_id, source_kind, &source_id, &link_id)
                    .map_err(|e| refuse(path, e.to_string()))?;
                Ok(tx.put_entity_link(&rec)?)
            }
            (
                DeliveryPath::Link {
                    source_kind,
                    source_id,
                    link_id,
                    ..
                },
                None,
            ) => Ok(tx.remove_entity_link(
                source_kind,
                &parse_id(path, &source_id)?,
                &parse_id(path, &link_id)?,
            )?),
        }
    }

    fn validate(&self, tx: &E::Tx) -> Result<(), ProjectionError> {
        validate_all(tx)
    }
}

fn validate_all<T: DeliveryRead + CoreRead>(tx: &T) -> Result<(), ProjectionError> {
    let existing: BTreeSet<Id> = tx.projects()?.into_iter().map(|p| p.id).collect();
    let mut projects = existing.clone();
    projects.extend(tx.projects_with_delivery()?);
    let adapter = StoreViewAdapter(tx);
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
