//! The project documentation tree family (`TASK-55`, `L11`, `FC-180`).
//!
//! Owns everything under `state/projects/<project-id>/docs/`:
//! - `<path>.md`: one document, Markdown with YAML front matter ([`records`]), projected into
//!   `documents` (the rows with a `path`);
//! - `media/<file>` and `media/<file>.yaml`: one image asset and its metadata, projected into
//!   `documentation_media`;
//! - `index.yaml`: the office-maintained index ([`index`]), regenerated in the same journal
//!   entry as every write of the tree ([`features::change`]) and checked on rebuild
//!   ([`projector`]).
//!
//! [`paths`] is the tree's one path validator and [`access`] its one authorization choke-point:
//! agents read and write only the trees of projects their home team owns or contributes to.

pub mod access;
pub mod features;
pub mod index;
pub mod paths;
pub mod ports;
pub mod projector;
pub mod records;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
#[cfg(test)]
mod tests;
