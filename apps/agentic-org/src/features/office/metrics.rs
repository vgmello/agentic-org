//! `agentic-org office metrics`: live per-office aggregate and per-agent efficiency telemetry
//! (`TASK-136`).

use clap::Args;
use clap_complete::engine::ArgValueCompleter;
use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::{Cell, ListView, Row, Style, Table};
use org_proto::sessions::SessionState;
use org_proto::telemetry::{AgentMetricsView, OfficeMetricsResult};

use crate::args::{InstanceArgs, LoginArgs, OrgArg};
use crate::completers;
use crate::error::CliError;
use crate::selection;
use crate::view;

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org office metrics\n  agentic-org office metrics --office <office-id> --json";

#[derive(Args, Debug)]
pub(crate) struct MetricsArgs {
    /// The office to inspect [default: this host's office]
    #[arg(long, value_name = "OFFICE_ID", add = ArgValueCompleter::new(completers::offices))]
    pub(super) office: Option<String>,
    #[command(flatten)]
    pub(super) org: OrgArg,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

pub(super) fn run(args: MetricsArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let target = selection::select(args.instance.name.as_deref(), args.office.as_deref())?;
    let metrics = org_host_setup::office_metrics(
        target.instance(),
        args.login.login.as_deref(),
        args.org.org.as_deref(),
        target.office(),
    )?;
    render_metrics(&metrics, ctx)
}

pub(crate) fn render_metrics(
    metrics: &OfficeMetricsResult,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    if ctx.modes().json {
        return Ok(ctx.write_json(metrics)?);
    }
    let json = view::json(metrics)?;
    if ctx.modes().format.is_none() && !ctx.modes().quiet {
        ctx.note(&view::format_office_summary_line(metrics));
    }
    let rows: Vec<Row> = metrics.agents.iter().map(agent_row).collect();
    let table = Table {
        headers: vec![
            "AGENT".into(),
            "STATE".into(),
            "MODEL".into(),
            "EFFORT".into(),
            "TURNS".into(),
            "LATENCY (P50/P95/AVG)".into(),
            "TOKENS (IN/OUT/CACHE/THOUGHT)".into(),
            "CACHE".into(),
            "RSS (CUR/PEAK)".into(),
            "DENIALS".into(),
        ],
        rows,
    };
    let list_view = ListView {
        empty: "No agents recorded for this office.",
        total: Some(metrics.agents.len()),
    };
    ctx.render_table(&table, &json, &list_view)?;
    Ok(())
}

fn agent_row(agent: &AgentMetricsView) -> Row {
    let latency = match (
        agent.p50_wall_latency_ms,
        agent.p95_wall_latency_ms,
        agent.avg_wall_latency_ms,
    ) {
        (Some(p50), Some(p95), Some(avg)) => {
            format!(
                "{}/{}/{}",
                view::format_ms(p50),
                view::format_ms(p95),
                view::format_ms(avg)
            )
        }
        _ => "-".to_owned(),
    };
    let tokens = format!(
        "{}/{}/{}/{}",
        agent.input_tokens, agent.output_tokens, agent.cached_read_tokens, agent.thought_tokens
    );
    let rss = format!(
        "{}/{}",
        agent
            .current_rss_bytes
            .map(view::format_bytes)
            .unwrap_or_else(|| "-".to_owned()),
        agent
            .peak_rss_bytes
            .map(view::format_bytes)
            .unwrap_or_else(|| "-".to_owned()),
    );
    let denial_style = if agent.sandbox_denial_count > 0 {
        Style::Warn
    } else {
        Style::Dim
    };
    Row {
        id: agent.employee_id.clone(),
        cells: vec![
            Cell::id(format!("@{}", agent.handle)),
            Cell::status(state_label(agent.state), state_style(agent.state)),
            Cell::plain(agent.active_model.as_deref().unwrap_or("-")),
            Cell::dim(agent.active_effort.as_deref().unwrap_or("default")),
            Cell::plain(format!(
                "{}/{}",
                agent.recent_turn_count, agent.total_turn_count
            )),
            Cell::plain(latency),
            Cell::plain(tokens),
            Cell::plain(view::format_ratio(agent.cache_hit_ratio)),
            Cell::plain(rss),
            Cell::status(agent.sandbox_denial_count.to_string(), denial_style),
        ],
    }
}

fn state_label(state: SessionState) -> &'static str {
    match state {
        SessionState::Idle => "idle",
        SessionState::Starting => "starting",
        SessionState::Running => "running",
        SessionState::WaitingApproval => "waiting_approval",
        SessionState::Suspended => "suspended",
        SessionState::Stopping => "stopping",
        SessionState::Stopped => "stopped",
        SessionState::StallLocked => "stall_locked",
        SessionState::RecoveryRequired => "recovery_required",
        _ => "unknown",
    }
}

fn state_style(state: SessionState) -> Style {
    match state {
        SessionState::Running | SessionState::Starting => Style::Ok,
        SessionState::WaitingApproval | SessionState::Suspended => Style::Warn,
        SessionState::StallLocked | SessionState::RecoveryRequired => Style::Error,
        SessionState::Idle | SessionState::Stopping | SessionState::Stopped => Style::Dim,
        _ => Style::Dim,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use org_cli_shell::context::{testing::captured, Modes};
    use org_cli_shell::terminal::Terminal;
    use org_proto::telemetry::OfficeAggregateMetrics;

    fn sample_metrics() -> OfficeMetricsResult {
        OfficeMetricsResult {
            office_id: "off-hq".into(),
            org_id: "org-1".into(),
            collected_at: "2026-11-27T20:00:00Z".into(),
            aggregate: OfficeAggregateMetrics {
                active_sessions: 1,
                idle_sessions: 1,
                recent_turn_count: 4,
                total_turn_count: 10,
                p50_wall_latency_ms: Some(850),
                p95_wall_latency_ms: Some(2400),
                avg_wall_latency_ms: Some(1200),
                input_tokens: 1000,
                output_tokens: 500,
                cached_read_tokens: 3000,
                thought_tokens: 200,
                cache_hit_ratio: Some(0.75),
                current_rss_bytes: 64 * 1024 * 1024,
                peak_rss_bytes: 128 * 1024 * 1024,
                rss_budget_bytes: Some(512 * 1024 * 1024),
                sandbox_denial_count: 2,
                hibernated_total: 1,
                rss_evicted_total: 0,
            },
            agents: vec![AgentMetricsView {
                employee_id: "emp-1".into(),
                handle: "ada".into(),
                name: "Ada".into(),
                state: SessionState::Running,
                runtime_id: Some("claude".into()),
                active_model: Some("claude-sonnet-4-6".into()),
                active_effort: Some("high".into()),
                recent_turn_count: 4,
                total_turn_count: 10,
                p50_wall_latency_ms: Some(850),
                p95_wall_latency_ms: Some(2400),
                avg_wall_latency_ms: Some(1200),
                last_wall_latency_ms: Some(900),
                input_tokens: 1000,
                output_tokens: 500,
                cached_read_tokens: 3000,
                thought_tokens: 200,
                cache_hit_ratio: Some(0.75),
                current_rss_bytes: Some(64 * 1024 * 1024),
                peak_rss_bytes: Some(128 * 1024 * 1024),
                sandbox_denial_count: 2,
            }],
        }
    }

    #[test]
    fn office_metrics_renders_json_and_human_table() {
        let metrics = sample_metrics();

        // 1. `--json` outputs valid structured JSON preserving all fields.
        let modes = Modes {
            json: true,
            ..Modes::default()
        };
        let (mut ctx, out) = captured(modes, Terminal::piped(), &[]);
        render_metrics(&metrics, &mut ctx).unwrap();
        let stdout = out.stdout.text();
        let parsed: OfficeMetricsResult = serde_json::from_str(&stdout).unwrap();
        assert_eq!(parsed, metrics);
        assert!(out.stderr.text().is_empty());

        // 2. Human TTY table prints aggregate summary notice and per-agent metrics table.
        let (mut ctx, out) = captured(
            Modes::default(),
            org_cli_shell::context::testing::tty(Some(160)),
            &[],
        );
        render_metrics(&metrics, &mut ctx).unwrap();
        let table = out.stdout.text();
        let notice = out.stderr.text();
        assert!(notice.contains("office off-hq:"), "{notice}");
        assert!(notice.contains("75.0%"), "{notice}");
        assert!(notice.contains("64.0 MiB"), "{notice}");
        assert!(notice.contains("2 sandbox denials"), "{notice}");
        assert!(table.contains("@ada"), "{table}");
        assert!(table.contains("claude-sonnet-4-6"), "{table}");
        assert!(table.contains("high"), "{table}");
        assert!(table.contains("75.0%"), "{table}");
        assert!(table.contains("1000/500/3000/200"), "{table}");
    }
}
