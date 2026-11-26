//! Standing instructions (`INSTR-4..21`) and office deny globs.

use super::*;

// ---------------------------------------------------------------------------
// INSTR-4, 5, 7, 8, 12, 15, 21
// ---------------------------------------------------------------------------

#[test]
fn builtin_instructions_stay_under_10kib_and_all_named_commands_exist_in_live_table() {
    assert!(
        BUILTIN_BEHAVIORAL_RULES.len() <= BUILTIN_INSTRUCTIONS_MAX_BYTES,
        "built-in rules exceeded 10 KiB: {} bytes",
        BUILTIN_BEHAVIORAL_RULES.len()
    );
    let table = org_agent_cli::table::table();
    let missing_core = verify_command_references(BUILTIN_BEHAVIORAL_RULES, &table);
    assert!(
        missing_core.is_empty(),
        "missing core cmds: {missing_core:?}"
    );
    let missing_hr = verify_command_references(HR_LEAD_GUIDANCE, &table);
    assert!(missing_hr.is_empty(), "missing HR cmds: {missing_hr:?}");

    let assembled = assemble_standing_instructions(
        &InstructionInput {
            organization_name: "Acme Org",
            office_name: "HQ Mac",
            employee_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1",
            employee_name: "Ada",
            handle: "ada",
            team_name: "Engineering",
            role_name: Some("Staff Engineer"),
            role_instructions: Some("Write clean Rust."),
            process_instructions: None,
            agent_instructions: "Prefer small diffs.",
            is_hr_lead: true,
            private_home: "/tmp/ada/home",
            workspace: "/tmp/ada/home",
            permission_preset: "workspace_edits",
            sandbox_enabled: true,
        },
        &table,
    );
    assert!(assembled.contains("Organization standards"));
    assert!(assembled.contains("NEVER override higher-precedence organization authority"));
    assert!(assembled.contains(SAME_USER_ISOLATION_NOTICE));
    assert!(assembled.contains("Built-in HR Lead Duties"));
    assert!(assembled.contains("agent-os session inspect"));
    assert!(assembled.contains("agent-os grant list"));
    assert!(assembled.contains("agent-os hr hire --draft"));

    let restart = compose_turn_objective(&TurnTrigger::RestartContinuation {
        previous_objective: "Ship C3".into(),
        accepted_commands: vec!["hr.hire.draft (0190...)".into()],
    });
    assert!(restart.contains("Office restarted"));
    assert!(restart.contains("hr.hire.draft"));

    let hr_obj = compose_turn_objective(&TurnTrigger::HrProposalRequest {
        requester: "owner".into(),
        request: "Hire a Rust engineer into Engineering".into(),
    });
    assert!(hr_obj.contains("agent-os hr hire --draft"));
    assert!(hr_obj.contains("requester confirmation"));
}

#[test]
fn denied_office_globs_cover_durable_private_notes_and_protected_companion_trees() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = SessionManager::new(
        Arc::clone(&office),
        SessionManagerConfig::production(office.durable_root().join("sock")),
    );
    let globs: Vec<String> = mgr
        .denied_office_globs()
        .into_iter()
        .map(|g| g.as_str().to_owned())
        .collect();
    let durable = office
        .durable_root()
        .canonicalize()
        .unwrap_or_else(|_| office.durable_root().to_path_buf());
    let office_local = durable.join("local/offices").join(&office.info().office_id);
    for expected in [
        durable.join("private"),
        durable.join("messaging"),
        durable.join("budgets"),
        durable.join("accounting"),
        durable.join("recovery"),
        office_local.join("security"),
        office_local.join("subscription-usage"),
    ] {
        let needle = format!("{}/**", expected.display());
        assert!(
            globs.contains(&needle),
            "denied_office_globs missing `{needle}`; got {globs:?}"
        );
    }
}

#[test]
fn command_table_or_empty_without_an_installed_catalogue_is_an_empty_table() {
    let empty = crate::instructions::command_table_or_empty(None);
    assert!(empty.rows().is_empty(), "no command rows");
    assert!(empty.resources().is_empty(), "no command resources");
    assert!(empty.find("msg", "send").is_none());
    assert_eq!(
        crate::instructions::render_command_catalogue(&empty),
        "## Available `agent-os` Commands",
        "the catalogue section lists no commands"
    );

    let live = crate::instructions::command_table_or_empty(Some(org_agent_cli::table::table));
    assert!(
        !live.rows().is_empty(),
        "an installed catalogue is used as given"
    );
}
