//! Durable path layout and parser for project delivery files under
//! `state/projects/<project_id>/{features,releases,deployments,flags,links}/`.

use org_proto::is_uuid_v7;

use super::records::LinkEntityKind;

/// Owned durable file classification under a project's delivery directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryPath {
    /// `state/projects/<pid>/features/<fid>.md`.
    Feature {
        /// Project UUIDv7.
        project_id: String,
        /// Feature UUIDv7.
        feature_id: String,
    },
    /// `state/projects/<pid>/features/_counter.yaml`.
    FeatureCounter {
        /// Project UUIDv7.
        project_id: String,
    },
    /// `state/projects/<pid>/releases/<rid>.md`.
    Release {
        /// Project UUIDv7.
        project_id: String,
        /// Release UUIDv7.
        release_id: String,
    },
    /// `state/projects/<pid>/releases/_counter.yaml`.
    ReleaseCounter {
        /// Project UUIDv7.
        project_id: String,
    },
    /// `state/projects/<pid>/releases/<rid>/changelog.md`.
    ReleaseChangelog {
        /// Project UUIDv7.
        project_id: String,
        /// Release UUIDv7.
        release_id: String,
    },
    /// `state/projects/<pid>/releases/<rid>/milestones/<mid>.md`.
    Milestone {
        /// Project UUIDv7.
        project_id: String,
        /// Release UUIDv7.
        release_id: String,
        /// Milestone UUIDv7.
        milestone_id: String,
    },
    /// `state/projects/<pid>/releases/<rid>/milestones/_counter.yaml`.
    MilestoneCounter {
        /// Project UUIDv7.
        project_id: String,
        /// Release UUIDv7.
        release_id: String,
    },
    /// `state/projects/<pid>/deployments/<did>.md`.
    Deployment {
        /// Project UUIDv7.
        project_id: String,
        /// Deployment UUIDv7.
        deployment_id: String,
    },
    /// `state/projects/<pid>/deployments/<did>/changelog.md`.
    DeploymentChangelog {
        /// Project UUIDv7.
        project_id: String,
        /// Deployment UUIDv7.
        deployment_id: String,
    },
    /// `state/projects/<pid>/flags/<flag_id>.md`.
    Flag {
        /// Project UUIDv7.
        project_id: String,
        /// Feature flag UUIDv7.
        flag_id: String,
    },
    /// `state/projects/<pid>/links/<source_kind>/<source_id>/<link_id>.yaml`.
    Link {
        /// Project UUIDv7.
        project_id: String,
        /// Source entity kind.
        source_kind: LinkEntityKind,
        /// Source entity UUIDv7.
        source_id: String,
        /// Link UUIDv7.
        link_id: String,
    },
    /// Any unexpected file inside one of the owned delivery subtrees (fail-closed on rebuild).
    Foreign {
        /// Project UUIDv7.
        project_id: String,
    },
}

/// Top-level project delivery directories owned by the delivery projector.
pub const DELIVERY_SUBDIRS: [&str; 5] = ["features", "releases", "deployments", "flags", "links"];

/// Builds `state/projects/<pid>/features/<fid>.md`.
pub fn feature_path(project_id: &str, feature_id: &str) -> String {
    format!("state/projects/{project_id}/features/{feature_id}.md")
}

/// Builds `state/projects/<pid>/features/_counter.yaml`.
pub fn feature_counter_path(project_id: &str) -> String {
    format!("state/projects/{project_id}/features/_counter.yaml")
}

/// Builds `state/projects/<pid>/releases/<rid>.md`.
pub fn release_path(project_id: &str, release_id: &str) -> String {
    format!("state/projects/{project_id}/releases/{release_id}.md")
}

/// Builds `state/projects/<pid>/releases/_counter.yaml`.
pub fn release_counter_path(project_id: &str) -> String {
    format!("state/projects/{project_id}/releases/_counter.yaml")
}

/// Builds `state/projects/<pid>/releases/<rid>/changelog.md`.
pub fn release_changelog_path(project_id: &str, release_id: &str) -> String {
    format!("state/projects/{project_id}/releases/{release_id}/changelog.md")
}

/// Builds `state/projects/<pid>/releases/<rid>/milestones/<mid>.md`.
pub fn milestone_path(project_id: &str, release_id: &str, milestone_id: &str) -> String {
    format!("state/projects/{project_id}/releases/{release_id}/milestones/{milestone_id}.md")
}

/// Builds `state/projects/<pid>/releases/<rid>/milestones/_counter.yaml`.
pub fn milestone_counter_path(project_id: &str, release_id: &str) -> String {
    format!("state/projects/{project_id}/releases/{release_id}/milestones/_counter.yaml")
}

/// Builds `state/projects/<pid>/deployments/<did>.md`.
pub fn deployment_path(project_id: &str, deployment_id: &str) -> String {
    format!("state/projects/{project_id}/deployments/{deployment_id}.md")
}

/// Builds `state/projects/<pid>/deployments/<did>/changelog.md`.
pub fn deployment_changelog_path(project_id: &str, deployment_id: &str) -> String {
    format!("state/projects/{project_id}/deployments/{deployment_id}/changelog.md")
}

/// Builds `state/projects/<pid>/flags/<flag_id>.md`.
pub fn flag_path(project_id: &str, flag_id: &str) -> String {
    format!("state/projects/{project_id}/flags/{flag_id}.md")
}

/// Builds `state/projects/<pid>/links/<source_kind>/<source_id>/<link_id>.yaml`.
pub fn link_path(
    project_id: &str,
    source_kind: LinkEntityKind,
    source_id: &str,
    link_id: &str,
) -> String {
    format!(
        "state/projects/{project_id}/links/{}/{source_id}/{link_id}.yaml",
        source_kind.as_str()
    )
}

fn uuid_stem(file: &str, suffix: &str) -> Option<String> {
    let stem = file.strip_suffix(suffix)?;
    is_uuid_v7(stem).then(|| stem.to_owned())
}

/// Parses a durable path into a [`DeliveryPath`]; returns `None` outside delivery subtrees.
pub fn parse_delivery_path(raw: &str) -> Option<DeliveryPath> {
    let parts: Vec<&str> = raw.split('/').collect();
    let ["state", "projects", project, subdir, rest @ ..] = parts.as_slice() else {
        return None;
    };
    if !is_uuid_v7(project) || !DELIVERY_SUBDIRS.contains(subdir) {
        return None;
    }
    let project_id = (*project).to_owned();
    let foreign = || DeliveryPath::Foreign {
        project_id: project_id.clone(),
    };
    Some(match (*subdir, rest) {
        ("features", ["_counter.yaml"]) => DeliveryPath::FeatureCounter { project_id },
        ("features", [file]) => match uuid_stem(file, ".md") {
            Some(feature_id) => DeliveryPath::Feature {
                project_id,
                feature_id,
            },
            None => foreign(),
        },
        ("releases", ["_counter.yaml"]) => DeliveryPath::ReleaseCounter { project_id },
        ("releases", [file]) => match uuid_stem(file, ".md") {
            Some(release_id) => DeliveryPath::Release {
                project_id,
                release_id,
            },
            None => foreign(),
        },
        ("releases", [rid, "changelog.md"]) if is_uuid_v7(rid) => DeliveryPath::ReleaseChangelog {
            project_id,
            release_id: (*rid).to_owned(),
        },
        ("releases", [rid, "milestones", "_counter.yaml"]) if is_uuid_v7(rid) => {
            DeliveryPath::MilestoneCounter {
                project_id,
                release_id: (*rid).to_owned(),
            }
        }
        ("releases", [rid, "milestones", file]) if is_uuid_v7(rid) => {
            match uuid_stem(file, ".md") {
                Some(milestone_id) => DeliveryPath::Milestone {
                    project_id,
                    release_id: (*rid).to_owned(),
                    milestone_id,
                },
                None => foreign(),
            }
        }
        ("deployments", [file]) => match uuid_stem(file, ".md") {
            Some(deployment_id) => DeliveryPath::Deployment {
                project_id,
                deployment_id,
            },
            None => foreign(),
        },
        ("deployments", [did, "changelog.md"]) if is_uuid_v7(did) => {
            DeliveryPath::DeploymentChangelog {
                project_id,
                deployment_id: (*did).to_owned(),
            }
        }
        ("flags", [file]) => match uuid_stem(file, ".md") {
            Some(flag_id) => DeliveryPath::Flag {
                project_id,
                flag_id,
            },
            None => foreign(),
        },
        ("links", [kind_str, source_id, file]) if is_uuid_v7(source_id) => {
            match (LinkEntityKind::parse(kind_str), uuid_stem(file, ".yaml")) {
                (Some(source_kind), Some(link_id)) if source_kind.is_valid_source() => {
                    DeliveryPath::Link {
                        project_id,
                        source_kind,
                        source_id: (*source_id).to_owned(),
                        link_id,
                    }
                }
                _ => foreign(),
            }
        }
        _ => foreign(),
    })
}
