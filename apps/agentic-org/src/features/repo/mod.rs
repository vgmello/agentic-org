//! Operator-owned `repo` credential subcommands handled directly by `agentic-org`:
//! - `repo set-credential`: store an encrypted per-repo HTTPS token in the office vault
//! - `repo credential-status`: inspect non-secret per-repo credential status on an office
//! - `repo revoke-credential`: revoke a stored per-repo HTTPS token from the office vault
//!
//! All other `repo <verb>` commands (`register`, `list`, `attach`, `status`, `fetch`, `push`)
//! run over the shared office command table via `sign_in::office_proxy`.

mod common;
mod credential_status;
mod revoke_credential;
mod set_credential;
#[cfg(test)]
mod tests;

use org_cli_shell::context::ExecutionContext;

use crate::error::CliError;

/// Whether `TopCommand::Repo(pass)` names one of the operator-local clap verbs
/// (`repo set-credential|credential-status|revoke-credential`).
pub(crate) fn is_local_verb(args: &[String]) -> bool {
    matches!(
        args.first().map(String::as_str),
        Some("set-credential" | "credential-status" | "revoke-credential")
    )
}

/// Whether `repo <args...>` may prompt for a secret on the controlling terminal.
pub(crate) fn reads_a_secret(args: &[String]) -> bool {
    args.first().map(String::as_str) == Some("set-credential")
}

/// Runs a directly handled `repo <verb>`, returning `None` for any other verb so the caller
/// forwards it through `sign_in::office_proxy`.
pub(crate) fn try_run(args: &[String], ctx: &mut ExecutionContext) -> Option<Result<(), CliError>> {
    let [verb, rest @ ..] = args else {
        return None;
    };
    match verb.as_str() {
        "set-credential" => Some(set_credential::run(rest, ctx)),
        "credential-status" => Some(credential_status::run(rest, ctx)),
        "revoke-credential" => Some(revoke_credential::run(rest, ctx)),
        _ => None,
    }
}

/// Runs `agentic-org sign-in [--name ...] [--office ...] -- repo <set-credential|credential-status|revoke-credential> ...`.
pub(crate) fn try_run_after_sign_in(
    sign_in_instance: Option<&str>,
    sign_in_office: Option<&str>,
    command: &[String],
    ctx: &mut ExecutionContext,
) -> Option<Result<(), CliError>> {
    let [first, verb, rest @ ..] = command else {
        return None;
    };
    if first != "repo" {
        return None;
    }
    match verb.as_str() {
        "set-credential" => Some(set_credential::run_after_sign_in(
            sign_in_instance,
            sign_in_office,
            rest,
            ctx,
        )),
        "credential-status" => Some(credential_status::run_after_sign_in(
            sign_in_instance,
            sign_in_office,
            rest,
            ctx,
        )),
        "revoke-credential" => Some(revoke_credential::run_after_sign_in(
            sign_in_instance,
            sign_in_office,
            rest,
            ctx,
        )),
        _ => None,
    }
}
