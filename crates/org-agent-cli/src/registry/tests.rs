//! The composed table: it holds the `org-command` contract, every write honours `--dry-run`, the
//! shorthands read as their commands, and every slice's guidance covers exactly the live rows.

use std::collections::BTreeSet;

use org_command::{Answer, IfMatch, Kind, MutationPolicy, Office, Request, Type, CATALOGUE_LIMIT};
use org_command_catalogue::shared::DIRECTORY_CARD_FIELDS;
use org_command_catalogue::POLICY_OVERRIDES;
use serde_json::{json, Value};

use super::*;
use crate::runtime::domain::{command_cue, resource_cue};
use crate::runtime::transport::{OfficeTransport, TransportOffice};
use crate::SESSION_ATTACH_HELP;

struct DryRunAuditTransport {
    calls: Vec<(String, Value)>,
}

impl OfficeTransport for DryRunAuditTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, org_command::Refusal> {
        self.calls.push((method.to_owned(), params));
        // A chat read-marker dry run answers what it would mark, in the call's own shape.
        if method == "chat.mark_read" {
            return Ok(json!({
                "conversation_id": "0193a5c0-0000-7000-8000-000000000100",
                "read_at": "2026-11-21T12:06:00.000Z",
                "last_read_message_id": "0193a5c0-0000-7000-8000-000000000104",
                "advanced": true
            }));
        }
        if method == "suggestion.create" {
            return Ok(json!({
                "suggestion": {
                    "id": "0193a5c0-0000-7000-8000-000000000901",
                    "status": "pending",
                    "command": "task create",
                    "call": "tasks.create",
                    "input": {"title": "Review release"},
                    "summary": "Create release review task",
                    "created_by_member_id": "0193a5c0-0000-7000-8000-000000000010",
                    "for_caller_id": "0193a5c0-0000-7000-8000-000000000002",
                    "preflight_ok": true,
                    "created_at": "2026-11-27T00:00:00.000Z",
                    "expires_at": "2026-11-27T00:15:00.000Z"
                },
                "ui_block": {
                    "title": "Create release review task",
                    "suggestion_id": "0193a5c0-0000-7000-8000-000000000901"
                },
                "dry_run": true
            }));
        }
        if method == "suggestion.withdraw" {
            return Ok(json!({
                "suggestion": {
                    "id": "0193a5c0-0000-7000-8000-000000000901",
                    "status": "withdrawn",
                    "command": "task create",
                    "call": "tasks.create",
                    "input": {"title": "Review release"},
                    "summary": "Create release review task",
                    "created_by_member_id": "0193a5c0-0000-7000-8000-000000000010",
                    "for_caller_id": "0193a5c0-0000-7000-8000-000000000002",
                    "preflight_ok": true,
                    "created_at": "2026-11-27T00:00:00.000Z",
                    "expires_at": "2026-11-27T00:15:00.000Z"
                }
            }));
        }
        Ok(json!({ "status": "checked" }))
    }
}

#[test]
fn command_table_passes_all_org_command_contract_checks() {
    let table = table();
    org_command::testkit::check_rows(&table);
}

#[test]
fn the_runtime_table_holds_the_checked_rows_and_is_built_once() {
    let checked = table();
    let runtime = runtime_table();
    assert_eq!(runtime.rows(), checked.rows());
    assert!(std::ptr::eq(runtime, runtime_table()));
}

#[test]
fn every_write_command_honors_dry_run_without_live_side_effects() {
    let table = table();
    let mut checked = 0usize;
    for cmd in table.rows().iter().copied().filter(|c| c.is_write()) {
        checked += 1;
        let mut args = vec![cmd.resource.to_owned(), cmd.verb.to_owned()];
        if !cmd.input.is_empty() {
            args.push("--input-json".to_owned());
            args.push(cmd.example.input.to_owned());
        }
        args.push("--dry-run".to_owned());
        if matches!(
            cmd.kind,
            Kind::Write {
                if_match: IfMatch::Required,
                ..
            }
        ) {
            args.push("--if-match".to_owned());
            args.push("1".to_owned());
        }
        let mut office = TransportOffice(DryRunAuditTransport { calls: vec![] });
        let out = crate::execute_with_office("agent-os", &args, &mut office);
        let env: Value = serde_json::from_str(&out.stdout).unwrap_or_else(|e| {
            panic!(
                "command `{} {}` returned non-JSON stdout: {e}: {}",
                cmd.resource, cmd.verb, out.stdout
            )
        });
        assert_eq!(
            env["meta"]["dry_run"],
            Value::Bool(true),
            "`{} {} --dry-run` must stamp meta.dry_run=true: {}",
            cmd.resource,
            cmd.verb,
            out.stdout
        );
        if office.0.calls.is_empty() {
            assert_eq!(
                out.exit_code, 1,
                "`{} {} --dry-run` refused with unexpected exit code: {}",
                cmd.resource, cmd.verb, out.stdout
            );
            assert_eq!(env["ok"], Value::Bool(false));
            assert_eq!(
                env["error"]["code"], "unsupported",
                "`{} {} --dry-run` with zero RPC calls must fail closed with `unsupported`: {}",
                cmd.resource, cmd.verb, out.stdout
            );
        } else {
            for (method, params) in &office.0.calls {
                assert_eq!(
                    params.get("dry_run"),
                    Some(&Value::Bool(true)),
                    "`{} {} --dry-run` invoked `{method}` without dry_run=true! params={params}",
                    cmd.resource,
                    cmd.verb
                );
            }
            assert_eq!(
                out.exit_code, 0,
                "`{} {} --dry-run` failed despite forwarding dry_run=true: {}",
                cmd.resource, cmd.verb, out.stdout
            );
            assert_eq!(env["ok"], Value::Bool(true));
        }
    }
    assert!(
        checked >= 40,
        "expected at least 40 write commands, checked {checked}"
    );
}

#[test]
fn directory_card_presence_enum_covers_all_member_presence_variants() {
    let presence_field = DIRECTORY_CARD_FIELDS
        .iter()
        .find(|f| f.name == "presence")
        .expect("presence field in DIRECTORY_CARD_FIELDS");
    let allowed = match presence_field.ty {
        Type::Enum(values) => values,
        other => panic!("expected Type::Enum for presence, got {other:?}"),
    };
    let variants = [
        org_proto::directory::MemberPresence::Online,
        org_proto::directory::MemberPresence::Available,
        org_proto::directory::MemberPresence::Busy,
        org_proto::directory::MemberPresence::Idle,
        org_proto::directory::MemberPresence::NeedsYou,
        org_proto::directory::MemberPresence::WaitingForAnswer,
        org_proto::directory::MemberPresence::Paused,
        org_proto::directory::MemberPresence::UnableToRun,
        org_proto::directory::MemberPresence::WithYou,
        org_proto::directory::MemberPresence::Asleep,
        org_proto::directory::MemberPresence::Offline,
        org_proto::directory::MemberPresence::Unknown,
    ];
    assert_eq!(
        allowed.len(),
        variants.len(),
        "DIRECTORY_CARD_FIELDS presence enum must match MemberPresence variants 1:1"
    );
    for variant in variants {
        let wire = serde_json::to_value(variant).unwrap();
        let wire_str = wire.as_str().unwrap();
        assert!(
            allowed.contains(&wire_str),
            "DIRECTORY_CARD_FIELDS presence enum missing `{wire_str}`"
        );
    }
}

fn words(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_owned).collect()
}

#[test]
fn each_shorthand_becomes_its_command_and_keeps_the_rest_of_the_line() {
    for (line, expected) in [
        ("who --team Core", "directory list --team Core"),
        ("context --format yaml", "session context --format yaml"),
        ("task note add --task T-A1", "task-note add --task T-A1"),
        ("who --help", "directory list --help"),
        ("context --schema", "session context --schema"),
    ] {
        assert_eq!(
            normalize_args(&words(line)).as_ref(),
            words(expected),
            "{line}"
        );
    }
}

#[test]
fn anything_else_is_left_as_it_is() {
    for line in [
        "task list",
        "session context",
        "boot",
        "instructions overview",
        "instructions",
        "task notes",
        "msg send --to who --body context",
        "",
    ] {
        let args = words(line);
        assert_eq!(normalize_args(&args).as_ref(), args.as_slice(), "{line}");
    }
}

struct PanicOffice;

impl Office for PanicOffice {
    fn call(&mut self, request: &Request) -> Result<Answer, Refusal> {
        panic!(
            "help must never contact the office: {}",
            request.command.name()
        );
    }
}

#[test]
fn root_catalogue_stays_well_under_catalogue_limit() {
    let table = table();
    let catalogue = table.catalogue();
    assert!(
        catalogue.len() <= CATALOGUE_LIMIT,
        "root catalogue is {} bytes; limit is {CATALOGUE_LIMIT}",
        catalogue.len()
    );
    assert!(
        catalogue.len() <= 3800,
        "expected root catalogue comfortably below {CATALOGUE_LIMIT} bytes, got {}",
        catalogue.len()
    );
}

#[test]
fn guidance_tables_match_live_command_table_exactly() {
    let table = table();
    let live_resources: BTreeSet<&str> = table.resources().into_iter().collect();
    let resource_guidance: Vec<&str> = DOMAINS
        .iter()
        .flat_map(|domain| domain.resources.iter().map(|(name, _)| *name))
        .collect();
    let guided_resources: BTreeSet<&str> = resource_guidance.iter().copied().collect();
    assert_eq!(
        resource_guidance.len(),
        guided_resources.len(),
        "duplicate resource guidance across the slices"
    );
    assert_eq!(
        guided_resources, live_resources,
        "resource guidance drifted from live table resources"
    );

    let live_commands: BTreeSet<(&str, &str)> = table
        .rows()
        .iter()
        .map(|row| (row.resource, row.verb))
        .collect();
    let command_guidance: Vec<(&str, &str)> = DOMAINS
        .iter()
        .flat_map(|domain| domain.cues.iter().copied().flatten())
        .map(|(r, v, _)| (*r, *v))
        .collect();
    let guided_commands: BTreeSet<(&str, &str)> = command_guidance.iter().copied().collect();
    assert_eq!(
        command_guidance.len(),
        guided_commands.len(),
        "duplicate command guidance across the slices"
    );
    assert_eq!(
        guided_commands, live_commands,
        "command guidance drifted from live CommandTable rows"
    );
    // Each slice guides exactly the rows it declares, so a command's guidance moves with it.
    for domain in DOMAINS {
        let own: BTreeSet<(&str, &str)> = domain
            .commands
            .iter()
            .copied()
            .flatten()
            .map(|row| (row.resource, row.verb))
            .collect();
        let guided: BTreeSet<(&str, &str)> = domain
            .cues
            .iter()
            .copied()
            .flatten()
            .map(|(r, v, _)| (*r, *v))
            .collect();
        assert_eq!(own, guided, "a slice guides another slice's rows");
    }
}

#[test]
fn every_agent_os_help_tier_and_alias_includes_use_when_guidance() {
    let table = table();
    let mut office = PanicOffice;

    let top = crate::execute_with_office("agent-os", &["--help".to_owned()], &mut office);
    assert_eq!(top.exit_code, 0);
    assert!(top.stdout.contains("Use when:"), "{}", top.stdout);
    for resource in table.resources() {
        let cue = resource_cue(DOMAINS, resource).unwrap();
        assert!(
            top.stdout.contains(cue),
            "top --help missing resource guidance for `{resource}`:\n{}",
            top.stdout
        );

        let res_help = crate::execute_with_office(
            "agent-os",
            &[resource.to_owned(), "--help".to_owned()],
            &mut office,
        );
        assert_eq!(res_help.exit_code, 0, "{resource} --help");
        assert!(
            res_help.stdout.contains(&format!("Use when: {cue}")),
            "{resource} --help missing resource Use when:\n{}",
            res_help.stdout
        );

        for cmd in table.verbs(resource) {
            let cmd_cue = command_cue(DOMAINS, cmd.resource, cmd.verb).unwrap();
            assert!(
                res_help.stdout.contains(&format!("Use when: {cmd_cue}")),
                "{resource} --help missing verb guidance for `{}`:\n{}",
                cmd.name(),
                res_help.stdout
            );

            let cmd_help = crate::execute_with_office(
                "agent-os",
                &[
                    cmd.resource.to_owned(),
                    cmd.verb.to_owned(),
                    "--help".to_owned(),
                ],
                &mut office,
            );
            assert_eq!(cmd_help.exit_code, 0, "{} --help", cmd.name());
            assert!(
                cmd_help.stdout.contains(&format!("Use when: {cmd_cue}")),
                "{} --help missing Use when:\n{}",
                cmd.name(),
                cmd_help.stdout
            );
        }
    }

    // Aliases & special transport help.
    let who_help = crate::execute_with_office(
        "agent-os",
        &["who".to_owned(), "--help".to_owned()],
        &mut office,
    );
    assert_eq!(who_help.exit_code, 0);
    assert!(who_help.stdout.contains("Use when:"), "{}", who_help.stdout);

    let note_help = crate::execute_with_office(
        "agent-os",
        &["task".to_owned(), "note".to_owned(), "--help".to_owned()],
        &mut office,
    );
    assert_eq!(note_help.exit_code, 0);
    assert!(
        note_help.stdout.contains("Use when:"),
        "{}",
        note_help.stdout
    );

    let note_add_help = crate::execute_with_office(
        "agent-os",
        &[
            "task".to_owned(),
            "note".to_owned(),
            "add".to_owned(),
            "--help".to_owned(),
        ],
        &mut office,
    );
    assert_eq!(note_add_help.exit_code, 0);
    assert!(
        note_add_help.stdout.contains("Use when:"),
        "{}",
        note_add_help.stdout
    );

    assert!(
        SESSION_ATTACH_HELP.contains("Use when:"),
        "{SESSION_ATTACH_HELP}"
    );

    let session_help = crate::execute_with_office(
        "agent-os",
        &["session".to_owned(), "--help".to_owned()],
        &mut office,
    );
    assert!(
        session_help.stdout.contains("attach") && session_help.stdout.contains("Use when:"),
        "{}",
        session_help.stdout
    );

    let human_session_help = crate::execute_with_office(
        "agentic-org",
        &["session".to_owned(), "--help".to_owned()],
        &mut office,
    );
    assert_eq!(human_session_help.exit_code, 0);
    assert!(
        human_session_help.stdout.contains("Use when:")
            && !human_session_help.stdout.contains("attach"),
        "agentic-org session --help must not mention `attach`:\n{}",
        human_session_help.stdout
    );

    let human_top_help =
        crate::execute_with_office("agentic-org", &["--help".to_owned()], &mut office);
    assert_eq!(human_top_help.exit_code, 0);
    assert!(
        !human_top_help.stdout.contains("session attach"),
        "agentic-org top --help must not mention `session attach`:\n{}",
        human_top_help.stdout
    );
}

#[test]
fn the_table_has_every_row_of_every_slice_once_and_dispatch_owns_each() {
    let live = table();
    let domain_rows: usize = DOMAINS
        .iter()
        .flat_map(|domain| domain.commands.iter().copied())
        .map(<[_]>::len)
        .sum();
    assert_eq!(live.rows().len(), domain_rows);
    assert_eq!(
        live.rows().len(),
        org_command_catalogue::rows()
            .iter()
            .map(|r| r.len())
            .sum::<usize>()
    );
    assert_eq!(
        live.rows().len(),
        139,
        "the agent-os catalogue has 139 commands"
    );
    // Every row reaches its slice's dispatch, which sends the office its call: none falls
    // through to `not wired`.
    struct Answering;
    impl OfficeTransport for Answering {
        fn call_method(&mut self, _: &str, _: Value) -> Result<Value, Refusal> {
            Err(Refusal::new(
                org_proto::ErrorCode::Unavailable,
                "stub",
                "stub",
            ))
        }
    }
    for row in live.rows() {
        let request = Request {
            command: row,
            input: serde_json::Map::new(),
            limit: None,
            cursor: None,
            dry_run: false,
            idempotency_key: Some("k".into()),
            if_match: Some(1),
            durable: false,
            fields: None,
        };
        if let Err(refusal) = execute(&mut Answering, &request) {
            assert!(
                !refusal.message.contains("is not wired"),
                "{}: {}",
                row.name(),
                refusal.message
            );
        }
    }
}

#[test]
fn a_hint_from_a_call_outside_the_command_is_never_reported_with_it() {
    struct Hinting(Option<u32>);
    impl OfficeTransport for Hinting {
        fn call_method(&mut self, _: &str, _: Value) -> Result<Value, Refusal> {
            self.0 = Some(3);
            Ok(json!({"agent_id": "0193a5c0-0000-7000-8000-000000000003"}))
        }
        fn take_waiting_messages(&mut self) -> Option<u32> {
            self.0.take()
        }
    }
    let row = runtime_table().find("budget", "status").unwrap();
    let request = Request {
        command: row,
        input: serde_json::Map::new(),
        limit: None,
        cursor: None,
        dry_run: false,
        idempotency_key: None,
        if_match: None,
        durable: false,
        fields: None,
    };
    let mut office = TransportOffice(Hinting(Some(99)));
    let answer = office.call(&request).unwrap();
    assert_eq!(
        answer.waiting_messages,
        Some(3),
        "the command's own call's hint"
    );
    assert_eq!(office.0 .0, None);
}

#[test]
fn mutation_policy_classifies_every_command_and_every_override_targets_a_live_row() {
    let live = table();
    for override_entry in POLICY_OVERRIDES {
        assert!(
            live.find(override_entry.resource, override_entry.verb)
                .is_some(),
            "policy override `{} {}` names no live command",
            override_entry.resource,
            override_entry.verb
        );
    }
    for (resource, verb) in [
        ("suggestion", "create"),
        ("suggestion", "withdraw"),
        ("ui", "present"),
        ("status", "set"),
        ("status", "clear"),
        ("task-note", "add"),
        ("task-note", "update"),
        ("task-note", "remove"),
        ("msg", "reply"),
    ] {
        assert_eq!(
            live.mutation_policy_by_name(resource, verb),
            Some(MutationPolicy::AutoSafe),
            "`{resource} {verb}` must be AutoSafe"
        );
    }
    for (resource, verb) in [
        ("repo", "push"),
        ("agent", "execution-set"),
        ("agent", "approve"),
        ("agent", "revoke"),
        ("agent", "sessions"),
        ("member", "set-hiring"),
    ] {
        assert_eq!(
            live.mutation_policy_by_name(resource, verb),
            Some(MutationPolicy::ForbiddenForAgent),
            "`{resource} {verb}` must be ForbiddenForAgent"
        );
    }
    for (resource, verb) in [
        ("task", "create"),
        ("task", "update"),
        ("team", "create"),
        ("project", "add-team"),
        ("project", "remove-team"),
        ("channel", "create"),
        ("process", "create"),
        ("hr", "hire"),
    ] {
        assert_eq!(
            live.mutation_policy_by_name(resource, verb),
            Some(MutationPolicy::ConfirmRequired),
            "`{resource} {verb}` must be ConfirmRequired"
        );
    }
    for (resource, verb) in [
        ("cost", "query"),
        ("cost", "prices"),
        ("cost", "subscription-usage"),
        ("budget", "list"),
        ("budget", "inspect"),
        ("provider", "list"),
        ("provider", "quota"),
        ("model-profile", "list"),
        ("office", "list"),
        ("office", "sync-status"),
        ("inbox", "list"),
        ("session", "list"),
        ("agent", "readiness"),
        ("agent", "execution-get"),
        ("suggestion", "list"),
        ("suggestion", "inspect"),
    ] {
        assert_eq!(
            live.mutation_policy_by_name(resource, verb),
            Some(MutationPolicy::ReadOnly),
            "`{resource} {verb}` must be ReadOnly"
        );
    }
    assert_eq!(
        live.mutation_policy_for_call("tasks.create"),
        Some(MutationPolicy::ConfirmRequired)
    );
    assert_eq!(
        live.mutation_policy_for_call("suggestion.create"),
        Some(MutationPolicy::AutoSafe)
    );
    assert_eq!(
        live.mutation_policy_for_call("ui.present"),
        Some(MutationPolicy::AutoSafe)
    );
    assert_eq!(
        live.mutation_policy_for_call("git.push"),
        Some(MutationPolicy::ForbiddenForAgent)
    );
}
