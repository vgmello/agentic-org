//! The shell of the operator command line `agentic-org`: everything a command needs around
//! its domain call, and nothing of the domain.
//!
//! - [`terminal`]: escaping untrusted text for a terminal ([`terminal::sanitize`]) and inside
//!   JSON ([`terminal::escape_json_controls`]), closed-pipe detection, what is a terminal, and
//!   the Ctrl+C handler.
//! - [`error`]: the [`error::CliDiagnostic`] trait every command error implements, the exit
//!   codes, the shell's own [`error::ShellError`] and the one diagnostic renderer (a JSON
//!   envelope under `--json`, `× [code] message` otherwise).
//! - [`context`]: the global modes (`--json`, `-q`, `--dry-run`, `--no-input`, `--yes`,
//!   `--color`), the color decision, a raw-argument pre-scan, and [`context::ExecutionContext`]
//!   that owns standard output and standard error.
//! - [`output`]: the four output modes of a list or a detail (TTY table, piped TSV, `--json`,
//!   `-q` IDs) and the dry-run preview.
//! - [`prompt`]: confirmation and input on the terminal (the only user of `cliclack`).
//! - [`paths`]: the configuration and cache directories.
//! - [`config`]: the five-tier non-secret instance/office selection with the strict
//!   `agentic-org.toml` manifest.
//! - [`cache`]: the completion cache.
//!
//! The crate knows no socket, office, host or operating system: the composition root passes
//! the platform directories and the environment in. Module graph (an arrow points at what a
//! module uses): `terminal -> error -> escape`; `context -> terminal, error`;
//! `output, prompt -> context`; `paths -> error`; `config, cache -> error, escape`.

#![forbid(unsafe_code)]

pub mod cache;
pub mod config;
pub mod context;
pub mod error;
mod escape;
pub mod output;
pub mod paths;
pub mod prompt;
pub mod terminal;
