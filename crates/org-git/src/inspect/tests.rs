//! Tests of read-only inspection: real `git` against a bare cache and a task worktree in a
//! temporary directory, plus the pure parsers, path validator, digest and line slicer.

mod blob_lines;
mod diff;
mod fixture;
mod units;
mod worktree_head;
