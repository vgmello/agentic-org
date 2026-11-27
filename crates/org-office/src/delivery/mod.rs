//! Project delivery & knowledge-graph family (`TASK-55`, `L7`/`L11`, `FC-058..070`):
//! canonical features, Why links, releases, milestones, changelogs, deployments, feature flags,
//! and release/deployment flag snapshots under `state/projects/<id>/{features,releases,deployments,flags,links}/`.

pub mod features;
pub mod ports;
pub mod projector;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
