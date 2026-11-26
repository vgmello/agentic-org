//! The standing prompt in `doc-27` §1 static-to-volatile order, so a runtime's prompt cache is
//! shared as far as possible: across every agent and turn for the built-in text and the command
//! catalogue, across a team chain for its processes, across a role for its text, and across turns
//! for the rest. Nothing per turn is in here: the turn's objective, its task and its process
//! delta go in the turn input ([`super::compose_turn_objective`]).

use org_command::CommandTable;

use super::identity::Placement;
use super::{builtin_instructions, render_command_catalogue, HR_LEAD, SAME_USER_ISOLATION_NOTICE};
use crate::wake::{sanitize_untrusted_meta, MAX_META_FIELD_BYTES};

/// Inputs for assembling an agent's standing prompt.
#[derive(Debug, Clone)]
pub struct InstructionInput<'a> {
    pub organization_name: &'a str,
    pub office_name: &'a str,
    pub employee_id: &'a str,
    pub employee_name: &'a str,
    pub handle: &'a str,
    /// Home team and leads by name ([`super::resolve_placement`]).
    pub placement: &'a Placement,
    pub role_name: Option<&'a str>,
    pub role_instructions: Option<&'a str>,
    /// The organization's processes and standards for the agent's home team chain: the same on
    /// every turn, whatever task the turn is about.
    pub process_instructions: Option<&'a str>,
    /// The agent's own prompt: its execution prompt and profile body.
    pub agent_instructions: &'a str,
    /// Instruction files of the project repository, when assembled.
    pub repository_instructions: Option<&'a str>,
    pub is_hr_lead: bool,
    pub private_home: &'a str,
    pub workspace: &'a str,
    pub permission_preset: &'a str,
    pub sandbox_enabled: bool,
}

fn non_empty(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|t| !t.is_empty())
}

fn meta(raw: &str) -> String {
    sanitize_untrusted_meta(raw, MAX_META_FIELD_BYTES)
}

/// Part 5: who the agent is and where it may work.
fn identity_section(input: &InstructionInput<'_>) -> String {
    let placement = input.placement;
    let mut out = format!(
        "# Who you are\nYou are {} (@{}), employee ID {}, an agent of {} working in the office {}.",
        meta(input.employee_name),
        meta(input.handle),
        meta(input.employee_id),
        meta(input.organization_name),
        meta(input.office_name),
    );
    if let Some(role) = input.role_name.map(meta).filter(|r| !r.is_empty()) {
        out.push_str(&format!("\nRole: {role}."));
    }
    out.push_str(&format!("\nHome team: {}", placement.home_team_name));
    if placement.leads_home_team {
        out.push_str(" (you lead it)");
    }
    out.push('.');
    if placement.leads.is_empty() {
        out.push_str("\nNo lead is set above you; take decisions you cannot make to the owner.");
    } else {
        let chain: Vec<String> = placement.leads.iter().map(|l| l.render()).collect();
        out.push_str(&format!(
            "\nLeads above you, nearest first: {}.",
            chain.join("; ")
        ));
    }
    match &placement.reports_to {
        Some(manager) => out.push_str(&format!("\nYou report to {manager}.")),
        None => out.push_str("\nYou report to the owner."),
    }
    if input.is_hr_lead {
        out.push_str("\n\n");
        out.push_str(HR_LEAD.trim_end());
    }
    let sandbox = if input.sandbox_enabled {
        "enabled (default-on)"
    } else {
        "disabled (explicit opt-out)"
    };
    out.push_str(&format!(
        "\n\n## Isolation and boundaries\n{SAME_USER_ISOLATION_NOTICE}\nPrivate home: {}\nWorkspace: {}\nPermission preset: {}\nSandbox: {sandbox}",
        input.private_home, input.workspace, input.permission_preset,
    ));
    out
}

/// Assembles the standing prompt for an agent session (`doc-27` §1, `FC-171`):
///
/// 1. built-in instructions (release-static, at most `BUILTIN_INSTRUCTIONS_MAX_BYTES`);
/// 2. the command catalogue generated from the live table (release-static, at most 4 KiB);
/// 3. the organization's processes and standards for the home team chain;
/// 4. the role's instructions;
/// 5. identity and boundaries: name, home team and leads by name, HR lead duties, isolation,
///    private home, workspace, permission preset and sandbox;
/// 6. the agent's own prompt;
/// 7. project repository instructions.
pub fn assemble_standing_instructions(
    input: &InstructionInput<'_>,
    table: &CommandTable,
) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(7);
    parts.push(builtin_instructions());
    parts.push(render_command_catalogue(table));
    if let Some(processes) = non_empty(input.process_instructions) {
        parts.push(format!(
            "## Standing processes and standards\nThese apply to your work. A turn about a task that resolves against another chain names the documents that apply to that task instead.\n\n{processes}"
        ));
    }
    if let Some(role_text) = non_empty(input.role_instructions) {
        let role = input
            .role_name
            .map(meta)
            .filter(|r| !r.is_empty())
            .unwrap_or_else(|| "your role".to_owned());
        parts.push(format!("## Role instructions: {role}\n{role_text}"));
    }
    parts.push(identity_section(input));
    if let Some(prompt) = non_empty(Some(input.agent_instructions)) {
        parts.push(format!("## Your agent prompt\n{prompt}"));
    }
    if let Some(files) = non_empty(input.repository_instructions) {
        parts.push(format!("## Project repository instructions\n{files}"));
    }
    parts.join("\n\n")
}
