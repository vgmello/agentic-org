//! The one composition point of `agent-os`: wires every feature slice's guidance and dispatch
//! onto the declarative [`org_command_catalogue`] table (B2.4 / B3, `doc-28` §3.2), its help,
//! and the [`Office`] the table runs against. No slice names another slice or this module.

use std::borrow::Cow;
use std::io::{BufRead, Write};

use org_command::{
    Answer, Command, Field, KeySource, Node, Office, Output, Refusal, Request, Sources, Status,
};
pub use org_command_catalogue::{catalogue_table, runtime_table, table};
use org_proto::ErrorCode;
use serde_json::Value;

use crate::features::{
    auth, budget, docs, files, git, hiring, messaging, office, organization, processes, projects,
    providers, session, tess, work,
};
use crate::runtime::domain::Domain;
use crate::runtime::help::{self, Guidance};
use crate::runtime::transport::{refuse_durable, OfficeTransport, TransportOffice};

/// Every slice of the table, in the order dispatch asks them.
const DOMAINS: &[Domain] = &[
    organization::DOMAIN,
    projects::DOMAIN,
    docs::DOMAIN,
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

/// Runs one parsed command against `office` through `org_command::run`, ensuring offline
/// `skill list|read --offline` responses always retain explicit partial-scope metadata
/// (`mode: "offline_builtin_only"`, `org_skills_included: false`, `caveat`) even under `--fields`.
pub(crate) fn run_command(
    program: &str,
    effective_args: &[String],
    sources: &mut dyn Sources,
    keys: &mut dyn KeySource,
    office: &mut dyn Office,
) -> Output {
    run_command_with_env(program, effective_args, sources, keys, office, None)
}

/// Like [`run_command`], applying `env_format` (`AGENT_OS_FORMAT`) when neither `--json` nor
/// `--format` is passed on the command line.
pub(crate) fn run_command_with_env(
    program: &str,
    effective_args: &[String],
    sources: &mut dyn Sources,
    keys: &mut dyn KeySource,
    office: &mut dyn Office,
    env_format: Option<&str>,
) -> Output {
    let table = runtime_table();
    let mut observer = OfflineSkillObserver {
        inner: office,
        captured: None,
    };
    let output = org_command::run_with_env_format(
        table,
        program,
        effective_args,
        sources,
        keys,
        &mut observer,
        env_format,
    );
    if output.exit_code == 0 {
        if let Some(captured) = observer.captured {
            let format = requested_format(captured.command, effective_args, env_format);
            return format_offline_skill(&captured, format);
        }
    }
    output
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputFormat {
    Json,
    Yaml,
    Compact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OfflineSkillKind {
    List,
    Read,
}

struct CapturedOfflineSkill {
    kind: OfflineSkillKind,
    command: &'static Command,
    answer: Answer,
    fields: Option<Vec<String>>,
}

struct OfflineSkillObserver<'a> {
    inner: &'a mut dyn Office,
    captured: Option<CapturedOfflineSkill>,
}

impl Office for OfflineSkillObserver<'_> {
    fn call(&mut self, request: &Request) -> Result<Answer, Refusal> {
        let kind = if request.command.resource == "skill"
            && request.input.get("offline").and_then(Value::as_bool) == Some(true)
        {
            match request.command.verb {
                "list" => Some(OfflineSkillKind::List),
                "read" => Some(OfflineSkillKind::Read),
                _ => None,
            }
        } else {
            None
        };
        let answer = self.inner.call(request)?;
        if let Some(kind) = kind {
            self.captured = Some(CapturedOfflineSkill {
                kind,
                command: request.command,
                answer: answer.clone(),
                fields: request.fields.clone(),
            });
        }
        Ok(answer)
    }
}

fn requested_format(command: &Command, args: &[String], env_format: Option<&str>) -> OutputFormat {
    let rest = args.get(2..).unwrap_or_default();
    let mut at = 0;
    while let Some(arg) = rest.get(at) {
        at += 1;
        let Some(body) = arg.strip_prefix("--") else {
            continue;
        };
        let (name, inline) = match body.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (body, None),
        };
        if name == "json" && inline.is_none() {
            return OutputFormat::Json;
        }
        let value = match (command.flag_takes_value(name), inline) {
            (Some(true), Some(value)) => Some(value),
            (Some(true), None) => {
                let val = rest.get(at).map(String::as_str);
                at += 1;
                val
            }
            _ => None,
        };
        if name == "format" {
            return match value {
                Some("yaml") => OutputFormat::Yaml,
                Some("compact") => OutputFormat::Compact,
                _ => OutputFormat::Json,
            };
        }
    }
    match env_format.and_then(|raw| org_command::parse_env_format(raw).ok().flatten()) {
        Some(org_command::Format::Yaml) => OutputFormat::Yaml,
        Some(org_command::Format::Compact) => OutputFormat::Compact,
        Some(org_command::Format::Json) | None => OutputFormat::Json,
    }
}

const OFFLINE_SCOPE_FIELDS: [&str; 3] = ["mode", "org_skills_included", "caveat"];

fn build_skill_list_nodes(
    schema_fields: &[Field],
    raw_data: &Value,
    projected: Option<&[String]>,
) -> Vec<Node> {
    let Some(items) = raw_data.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .map(|item| {
            let Some(obj) = item.as_object() else {
                return Node::from_value(item);
            };
            let entries: Vec<(String, Node)> = schema_fields
                .iter()
                .filter(|field| {
                    projected.is_none_or(|wanted| wanted.iter().any(|w| w == field.name))
                })
                .filter_map(|field| {
                    obj.get(field.name)
                        .map(|val| (field.name.to_owned(), Node::from_value(val)))
                })
                .collect();
            Node::Map(entries)
        })
        .collect()
}

fn build_skill_read_node(
    schema_fields: &[Field],
    raw_data: &Value,
    projected: Option<&[String]>,
) -> Node {
    let Some(obj) = raw_data.as_object() else {
        return Node::from_value(raw_data);
    };
    let entries: Vec<(String, Node)> = schema_fields
        .iter()
        .filter(|field| {
            OFFLINE_SCOPE_FIELDS.contains(&field.name)
                || projected.is_none_or(|wanted| wanted.iter().any(|w| w == field.name))
        })
        .filter_map(|field| {
            obj.get(field.name)
                .map(|val| (field.name.to_owned(), Node::from_value(val)))
        })
        .collect();
    Node::Map(entries)
}

fn format_offline_skill(captured: &CapturedOfflineSkill, format: OutputFormat) -> Output {
    let (data, empty_list) = match captured.kind {
        OfflineSkillKind::List => {
            let skills = build_skill_list_nodes(
                captured.command.output,
                &captured.answer.data,
                captured.fields.as_deref(),
            );
            let empty = skills.is_empty();
            let node = Node::map([
                ("mode", Node::text(processes::OFFLINE_SKILL_MODE)),
                ("org_skills_included", Node::Bool(false)),
                ("caveat", Node::text(processes::OFFLINE_SKILL_CAVEAT)),
                ("skills", Node::List(skills)),
            ]);
            (node, empty)
        }
        OfflineSkillKind::Read => {
            let node = build_skill_read_node(
                captured.command.output,
                &captured.answer.data,
                captured.fields.as_deref(),
            );
            (node, false)
        }
    };
    let mut meta_entries = vec![(
        "status".to_owned(),
        Node::text(captured.answer.status.as_str()),
    )];
    if let Some(cursor) = &captured.answer.next_cursor {
        meta_entries.push(("next_cursor".to_owned(), Node::text(cursor)));
    }
    if let Some(total) = captured.answer.total {
        meta_entries.push(("total".to_owned(), Node::Number(total.into())));
    }
    let meta = Node::Map(meta_entries);
    let stdout = match format {
        OutputFormat::Json => {
            let envelope = Node::map([
                ("ok", Node::Bool(true)),
                ("data", data),
                ("meta", meta),
                ("error", Node::Null),
            ]);
            format!("{}\n", envelope.to_json())
        }
        OutputFormat::Yaml => {
            let envelope = Node::map([
                ("ok", Node::Bool(true)),
                ("data", data),
                ("meta", meta),
                ("error", Node::Null),
            ]);
            envelope.to_yaml()
        }
        OutputFormat::Compact => {
            let mut out = data.to_compact(&[]);
            if captured.answer.status != Status::Completed
                || captured.answer.next_cursor.is_some()
                || (captured.answer.total.is_some() && empty_list)
            {
                out.push_str(&format!("meta: {}\n", meta.to_json()));
            }
            out
        }
    };
    Output {
        stdout,
        exit_code: 0,
    }
}

/// Runs the `session attach` line loop over the live table.
pub(crate) fn run_attach_loop<R: BufRead, W: Write>(
    program: &str,
    reader: R,
    writer: W,
    office: &mut dyn Office,
) -> u8 {
    session::run_attach_loop_with(
        runtime_table(),
        program,
        reader,
        writer,
        office,
        run_command,
    )
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
