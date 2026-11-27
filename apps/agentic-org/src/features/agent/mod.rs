//! Operator-owned `agent` subcommands handled directly by `agentic-org`:
//! - `agent bind` (`TASK-211`): one-step local BYOA workspace binding
//! - `agent renew` (`TASK-248`): human-only external lease renewal
//! - `agent sessions` (`TASK-248`): active/history sessions & `--pending` BYOA approvals list
//!
//! All other `agent <verb>` commands (`inspect`, `update`, `pause`, `approve`, `revoke`, ...)
//! run over the shared office command table via `sign_in::office_proxy`.

mod bind;
mod common;
mod renew;
mod sessions;

use org_cli_shell::context::ExecutionContext;

use crate::error::CliError;

/// Runs a directly handled `agent <verb>` (`bind`, `renew`, `sessions`), returning `None` for any
/// other verb so the caller can forward it through `sign_in::office_proxy`.
pub(crate) fn try_run(args: &[String], ctx: &mut ExecutionContext) -> Option<Result<(), CliError>> {
    let [verb, rest @ ..] = args else {
        return None;
    };
    match verb.as_str() {
        "bind" => Some(bind::run(rest, ctx)),
        "renew" => Some(renew::run(rest, ctx)),
        "sessions" => Some(sessions::run(rest, ctx)),
        _ => None,
    }
}

/// Runs `agentic-org sign-in [--name ...] [--office ...] -- agent <bind|renew|sessions> ...`.
pub(crate) fn try_run_after_sign_in(
    sign_in_instance: Option<&str>,
    sign_in_office: Option<&str>,
    command: &[String],
    ctx: &mut ExecutionContext,
) -> Option<Result<(), CliError>> {
    let [first, verb, rest @ ..] = command else {
        return None;
    };
    if first != "agent" {
        return None;
    }
    match verb.as_str() {
        "bind" => Some(bind::run_after_sign_in(
            sign_in_instance,
            sign_in_office,
            rest,
            ctx,
        )),
        "renew" => Some(renew::run_after_sign_in(
            sign_in_instance,
            sign_in_office,
            rest,
            ctx,
        )),
        "sessions" => Some(sessions::run_after_sign_in(
            sign_in_instance,
            sign_in_office,
            rest,
            ctx,
        )),
        _ => None,
    }
}
