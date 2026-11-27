//! The one composition point of `agent-os`: wires every feature slice's guidance and dispatch
//! onto the declarative [`org_command_catalogue`] table (B2.4 / B3, `doc-28` §3.2), its help,
//! and the [`Office`] the table runs against. No slice names another slice or this module.

use std::borrow::Cow;
use std::io::{BufRead, Write};

use org_command::{Answer, Office, Output, Refusal, Request};
pub use org_command_catalogue::{catalogue_table, runtime_table, table};
use org_proto::ErrorCode;
use serde_json::Value;

use crate::features::{
    auth, budget, files, git, hiring, messaging, office, organization, processes, projects,
    providers, session, tess, work,
};
use crate::runtime::domain::Domain;
use crate::runtime::help::{self, Guidance};
use crate::runtime::transport::{refuse_durable, OfficeTransport, TransportOffice};

/// Every slice of the table, in the order dispatch asks them.
const DOMAINS: &[Domain] = &[
    organization::DOMAIN,
    projects::DOMAIN,
    hiring::DOMAIN,
    session::DOMAIN,
    messaging::DOMAIN,
    work::DOMAIN,
    files::DOMAIN,
    git::DOMAIN,
    processes::DOMAIN,
    budget::DOMAIN,
    providers::DOMAIN,
    office::DOMAIN,
    tess::DOMAIN,
];

/// What the help text adds to `org-command`'s: every slice's guidance, `session attach`, and the
/// sign-in verbs outside the table.
const GUIDANCE: Guidance = Guidance {
    domains: DOMAINS,
    operator_resources: session::OPERATOR_RESOURCES,
    agent_verbs: session::AGENT_VERBS,
    operator_verbs: hiring::OPERATOR_VERBS,
    agent_footer: auth::SIGN_IN_HELP,
};

/// What `agent-os login` answers from the rest of the crate.
const LOGIN_HOOKS: auth::LoginHooks = auth::LoginHooks { briefing };

/// The bound `agent-os context` briefing (with `catalogue` inside it), for `agent-os login`.
fn briefing(transport: &mut dyn OfficeTransport) -> Result<Value, Refusal> {
    session::bound_briefing(transport, &runtime_table().catalogue())
}

/// `args` with a leading shorthand replaced by the command it stands for; anything else as it is.
pub(crate) fn normalize_args(args: &[String]) -> Cow<'_, [String]> {
    runtime_table().expand(args)
}

/// `agent approve <code>` and the other operator verbs with their bare words read as named flags,
/// plus `file render --format pdf` mapped onto `--to pdf` without colliding with `--format json|yaml|compact`.
pub(crate) fn rewrite_positional(args: &[String]) -> Option<Vec<String>> {
    let table = runtime_table();
    hiring::rewrite_positional(table, args).or_else(|| files::rewrite_render_args(table, args))
}

/// `org-command`'s plain-text help of `args`, enriched with every slice's `Use when:` guidance.
pub(crate) fn augment_help(program: &str, args: &[String], output: Output) -> Output {
    help::augment_help(runtime_table(), &GUIDANCE, program, args, output)
}

/// Runs `agent-os login|whoami|logout|exec`; `None` for any other command line.
pub(crate) fn run_lifecycle(args: &[String]) -> Option<(String, u8)> {
    auth::run_lifecycle(args, &LOGIN_HOOKS)
}

/// Runs the `session attach` line loop over the live table.
pub(crate) fn run_attach_loop<R: BufRead, W: Write>(
    program: &str,
    reader: R,
    writer: W,
    office: &mut dyn Office,
) -> u8 {
    session::run_attach_loop(runtime_table(), program, reader, writer, office)
}

/// Dispatches one parsed `org-command` [`Request`] over `transport` to the slice that owns its
/// command. A `--durable` the command cannot honor is refused here, before any slice sends
/// anything ([`refuse_durable`]).
fn execute(transport: &mut dyn OfficeTransport, request: &Request) -> Result<Answer, Refusal> {
    refuse_durable(request)?;
    let table = runtime_table();
    for domain in DOMAINS {
        if domain.owns(request.command) {
            if let Some(outcome) = (domain.dispatch)(transport, request, table) {
                return outcome;
            }
        }
    }
    let (resource, verb) = (request.command.resource, request.command.verb);
    Err(Refusal::new(
        ErrorCode::Unsupported,
        format!("`{resource} {verb}` is not wired to the office yet."),
        "Run `--help` to see available commands.",
    ))
}

/// The command table's [`Office`] over a transport: each request is dispatched by [`execute`],
/// and answered with the `waiting_messages` hint of its own calls only.
impl<T: OfficeTransport> Office for TransportOffice<T> {
    fn call(&mut self, request: &Request) -> Result<Answer, Refusal> {
        // A hint left by a call outside this command (a direct call, a bind) is never reported.
        let _ = self.0.take_waiting_messages();
        let mut answer = execute(&mut self.0, request)?;
        answer.waiting_messages = self.0.take_waiting_messages();
        Ok(answer)
    }
}

#[cfg(test)]
mod tests;
