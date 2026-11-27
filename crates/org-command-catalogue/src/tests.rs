//! Contract tests for the pure declarative `agent-os` command catalogue (`TASK-254`).

use org_command::{MutationPolicy, Type, CATALOGUE_LIMIT};

use crate::shared::DIRECTORY_CARD_FIELDS;
use crate::{catalogue_table, rows, runtime_table, table, POLICY_OVERRIDES};

#[test]
fn command_table_passes_all_org_command_contract_checks() {
    let live = table();
    org_command::testkit::check_rows(&live);
    assert_eq!(
        live.rows().len(),
        rows().iter().map(|r| r.len()).sum::<usize>()
    );
    assert_eq!(
        live.rows().len(),
        135,
        "the agent-os catalogue has 135 commands"
    );
}

#[test]
fn runtime_table_and_catalogue_table_match_checked_table_and_build_once() {
    let checked = table();
    let runtime = runtime_table();
    assert_eq!(runtime.rows(), checked.rows());
    assert!(std::ptr::eq(runtime, runtime_table()));
    let cloned = catalogue_table();
    assert_eq!(cloned.rows(), checked.rows());
    assert_eq!(cloned.catalogue(), checked.catalogue());
}

#[test]
fn root_catalogue_stays_well_under_catalogue_limit() {
    let live = table();
    let catalogue = live.catalogue();
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
fn aliases_expand_cleanly_without_touching_unrelated_commands() {
    let live = runtime_table();
    let words =
        |line: &str| -> Vec<String> { line.split_whitespace().map(str::to_owned).collect() };
    for (line, expected) in [
        ("who --team Core", "directory list --team Core"),
        ("context --format yaml", "session context --format yaml"),
        ("task note add --task T-A1", "task-note add --task T-A1"),
        ("who --help", "directory list --help"),
        ("context --schema", "session context --schema"),
    ] {
        assert_eq!(
            live.expand(&words(line)).as_ref(),
            words(expected),
            "{line}"
        );
    }
    for line in [
        "task list",
        "session context",
        "msg send --to who --body context",
        "",
    ] {
        let args = words(line);
        assert_eq!(live.expand(&args).as_ref(), args.as_slice(), "{line}");
    }
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
