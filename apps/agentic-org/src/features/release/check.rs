//! `release check`: fetch and verify a signed release pointer against the active release pin,
//! unresolved crash-recovery checkpoints, and local quarantine tombstones without downloading
//! a bundle or mutating any state.

use std::time::Duration;

use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::{Cell, Style};
use org_host::release::{
    check_pointer, HttpsReleaseFetcher, PendingHealthCheck, PointerCheckState, PointerDecision,
    PullConfig, ReleasePin, UpdateStateStore,
};
use org_host_setup::auto_update::read_store_head_readonly;
use serde::Serialize;

use super::{state_dir_for, store_dir};
use crate::error::CliError;

#[derive(Serialize)]
struct CheckView {
    status: &'static str,
    channel: String,
    release_id: String,
    sequence: u64,
    published_at: String,
    target_url: String,
    target_sha256: String,
    target_size: u64,
    store: String,
    state_dir: String,
    current_pin: Option<ReleasePin>,
    highest_sequence: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pending_health_check: Option<PendingHealthCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quarantine_reason: Option<String>,
}

pub(super) struct CheckArgs<'a> {
    pub(super) pointer_url: &'a str,
    pub(super) signature_url: Option<&'a str>,
    pub(super) store: Option<&'a str>,
    pub(super) state_dir: Option<&'a str>,
    pub(super) timeout_secs: u64,
    pub(super) allow_quarantined: bool,
}

pub(super) fn run(args: CheckArgs<'_>, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let store = store_dir(args.store)?;
    let state_dir = state_dir_for(&store, args.store.is_some(), args.state_dir)?;
    let (current_pin, highest_sequence) = read_store_head_readonly(&store)?;
    let update_state = UpdateStateStore::read_readonly(&state_dir)
        .map_err(|e| format!("reading update state ({}): {e}", state_dir.display()))?;

    let mut pull = PullConfig::https(args.pointer_url, args.signature_url.map(str::to_owned));
    pull.metadata_timeout = Duration::from_secs(args.timeout_secs.max(1));

    let fetcher = HttpsReleaseFetcher::new();
    let check_state = PointerCheckState {
        current_pin: current_pin.as_ref(),
        highest_sequence,
        update_state: &update_state,
        allow_quarantined: args.allow_quarantined,
    };
    let checked = check_pointer(&fetcher, &pull, &check_state)
        .map_err(|e| format!("checking signed release pointer: {e}"))?;

    let pending_health_check = update_state.pending_health_check;
    let ptr = checked.verified.pointer();
    let target = ptr.target();
    let (status, style, quarantine_reason, failed) =
        classify_check_outcome(&checked.decision, pending_health_check.as_ref());

    let view = CheckView {
        status,
        channel: ptr.channel().to_owned(),
        release_id: ptr.release_id().to_owned(),
        sequence: ptr.sequence(),
        published_at: ptr.published_at().to_owned(),
        target_url: target.url().to_owned(),
        target_sha256: target.sha256_hex(),
        target_size: target.size(),
        store: store.display().to_string(),
        state_dir: state_dir.display().to_string(),
        current_pin: checked.current_pin,
        highest_sequence: checked.highest_sequence,
        pending_health_check,
        quarantine_reason,
    };

    render_check(&view, style, ctx)?;
    if failed {
        return Err(CliError::Exited(1));
    }
    Ok(())
}

fn classify_check_outcome(
    decision: &PointerDecision,
    pending: Option<&PendingHealthCheck>,
) -> (&'static str, Style, Option<String>, bool) {
    match decision {
        PointerDecision::UpToDate if pending.is_some() => {
            ("pending_recovery", Style::Warn, None, false)
        }
        PointerDecision::UpToDate => ("up_to_date", Style::Active, None, false),
        PointerDecision::UpdateAvailable => ("update_available", Style::Ok, None, false),
        PointerDecision::Quarantined { reason } => {
            ("quarantined", Style::Warn, Some(reason.clone()), false)
        }
        PointerDecision::DowngradeRejected { .. } => {
            ("downgrade_rejected", Style::Error, None, true)
        }
    }
}

fn render_check(
    view: &CheckView,
    style: Style,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let mut fields = vec![
        ("status", Cell::status(view.status, style)),
        ("channel", Cell::plain(&view.channel)),
        ("release", Cell::id(&view.release_id)),
        ("sequence", Cell::plain(view.sequence.to_string())),
        ("published_at", Cell::dim(&view.published_at)),
        ("target_url", Cell::plain(&view.target_url)),
        ("target_sha256", Cell::dim(&view.target_sha256)),
        ("target_size", Cell::plain(view.target_size.to_string())),
        ("store", Cell::plain(&view.store)),
    ];
    if let Some(pending) = &view.pending_health_check {
        let prev = pending
            .previous_pin
            .as_ref()
            .map(|p| p.release_id.as_str())
            .unwrap_or("none");
        fields.push((
            "pending_health_check",
            Cell::status(
                format!(
                    "unresolved `{}` (seq {}, previous `{prev}`); run `agentic-org release update` to reconcile",
                    pending.target_pin.release_id, pending.target_pin.sequence
                ),
                Style::Warn,
            ),
        ));
    }
    if let Some(reason) = &view.quarantine_reason {
        fields.push(("quarantine_reason", Cell::status(reason, Style::Warn)));
    }
    let json =
        serde_json::to_value(view).map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    ctx.render_detail(&view.release_id, &fields, &json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_pending() -> PendingHealthCheck {
        PendingHealthCheck {
            previous_pin: Some(ReleasePin {
                release_id: "v0.1.0".into(),
                manifest_sha256: "1".repeat(64),
                sequence: 1,
            }),
            target_pin: ReleasePin {
                release_id: "v0.2.0".into(),
                manifest_sha256: "2".repeat(64),
                sequence: 2,
            },
            started_at_ms: 1_700_000_000_000,
            pointer_url: Some("https://releases.example.com/release-pointer.v1.json".into()),
            expected_running_instances: Some(vec!["prod".into()]),
        }
    }

    #[test]
    fn unresolved_pending_health_check_prevents_up_to_date_and_serializes_in_check_view() {
        let pending = sample_pending();
        let (status, _, _, failed) =
            classify_check_outcome(&PointerDecision::UpToDate, Some(&pending));
        assert_eq!(status, "pending_recovery");
        assert!(!failed);

        let (clean_status, _, _, _) = classify_check_outcome(&PointerDecision::UpToDate, None);
        assert_eq!(clean_status, "up_to_date");

        let view = CheckView {
            status,
            channel: "stable".into(),
            release_id: "v0.2.0".into(),
            sequence: 2,
            published_at: "2026-11-28T00:00:00Z".into(),
            target_url: "https://releases.example.com/agentic-org-v0.2.0-linux-x86_64.tar.gz"
                .into(),
            target_sha256: "a".repeat(64),
            target_size: 1024,
            store: "/srv/store".into(),
            state_dir: "/srv/state".into(),
            current_pin: Some(pending.target_pin.clone()),
            highest_sequence: 2,
            pending_health_check: Some(pending),
            quarantine_reason: None,
        };
        let encoded = serde_json::to_value(&view).unwrap();
        assert_eq!(encoded["status"], "pending_recovery");
        assert_eq!(
            encoded["pending_health_check"]["target_pin"]["release_id"],
            "v0.2.0"
        );
        assert_eq!(
            encoded["pending_health_check"]["previous_pin"]["release_id"],
            "v0.1.0"
        );
    }
}
