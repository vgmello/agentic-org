//! Pure delivery domain layer (`TASK-55`, `L7`/`L11`, `FC-058..070`):
//! durable file paths, canonical records, schema-aligned invariants, and YAML/Markdown codecs
//! for features, Why links, releases, milestones, changelogs, deployments, feature flags, and
//! release/deployment flag snapshots.

/// Strict deterministic YAML/Markdown codecs for delivery records.
pub mod codecs;
/// Engine-independent delivery read/write decision contracts and helpers.
pub mod ops;
/// Deployment, deployment changelog, feature flag, and deployment snapshot decisions.
pub mod ops_deployments;
/// Feature and Why-link write decisions.
pub mod ops_features;
/// Read and inspect handlers for all delivery entities.
pub mod ops_read;
/// Release, milestone, release changelog, and release flag snapshot decisions.
pub mod ops_releases;
/// Durable file paths under `state/projects/<id>/{features,releases,deployments,flags,links}/`.
pub mod paths;
/// Pure domain records for delivery entities and progress projections.
pub mod records;
/// Pure validation rules, DAG cycle checks, flag type checks, and progress calculations.
pub mod rules;
/// Cross-file rebuild validation for a project's delivery subtree.
pub mod validate;
/// Wire <-> pure domain record conversions.
pub mod wire;

#[cfg(test)]
mod tests;
