//! `release check`: fetch and verify a signed release pointer against the active release pin
//! and local quarantine tombstones without downloading a bundle or mutating any state.

use std::time::Duration;

use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::{Cell, Style};
use org_host::release::{
    check_pointer, HttpsReleaseFetcher, PointerDecision, PullConfig, ReleasePin, UpdateStateStore,
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
    let checked = check_pointer(
        &fetcher,
        &pull,
        current_pin.as_ref(),
        highest_sequence,
        &update_state,
        args.allow_quarantined,
    )
    .map_err(|e| format!("checking signed release pointer: {e}"))?;

    let ptr = checked.verified.pointer();
    let target = ptr.target();
    let (status, style, quarantine_reason, failed) = match &checked.decision {
        PointerDecision::UpToDate => ("up_to_date", Style::Active, None, false),
        PointerDecision::UpdateAvailable => ("update_available", Style::Ok, None, false),
        PointerDecision::Quarantined { reason } => {
            ("quarantined", Style::Warn, Some(reason.clone()), false)
        }
        PointerDecision::DowngradeRejected { .. } => {
            ("downgrade_rejected", Style::Error, None, true)
        }
    };

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
        quarantine_reason,
    };

    let fields = [
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
    let json =
        serde_json::to_value(&view).map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    ctx.render_detail(&view.release_id, &fields, &json)?;
    if failed {
        return Err(CliError::Exited(1));
    }
    Ok(())
}
