//! `agentic-org office metrics` and non-interactive `doctor` telemetry probe (`TASK-136`).

use org_host_control::ops::{SessionLocal, SessionLocalParams};
use org_proto::telemetry::{OfficeMetrics, OfficeMetricsParams, OfficeMetricsResult};

use super::cli_session::{connect_local_peer, end_session, Target};
use super::project_command::{call_office, OfficeSession};
use super::wire::registered;
use org_host::cli_error::{office_refused, HostCliError};

/// `agentic-org office metrics [--org <org_id>] [--office <office_id>]` (owner-only).
pub fn office_metrics(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    office_id: Option<&str>,
) -> Result<OfficeMetricsResult, HostCliError> {
    call_office::<OfficeMetrics>(
        instance_name,
        login,
        org_id,
        office_id,
        false,
        &OfficeMetricsParams {},
    )?
    .map_err(|error| office_refused("reading office metrics", &error))
}

/// Best-effort, strictly non-interactive probe for `agentic-org doctor`.
///
/// Only connects over a live local authless `host.sock` session; never prompts for a password,
/// never prints a browser approval link, and returns `None` cleanly whenever the host/office
/// is stopped, unreachable, or requires interactive sign-in.
pub fn try_office_metrics_noninteractive(
    instance_name: Option<&str>,
    office_id: Option<&str>,
) -> Option<OfficeMetricsResult> {
    let paths = org_host::human::paths(instance_name).ok()?;
    let (_, id, socket) = registered(&paths, instance_name).ok()?;
    let target = Target {
        socket: &socket,
        instance_id: &id,
        name: instance_name,
    };
    let mut wire = connect_local_peer(&target).ok()?;
    let signed = wire
        .call_answer::<SessionLocal>(&SessionLocalParams {}, None)
        .ok()?
        .ok()?;
    let session = signed.session;
    let answer = (|| {
        let mut office =
            OfficeSession::open_for_office(&mut wire, &session, None, office_id).ok()?;
        office
            .call::<OfficeMetrics>(&mut wire, &session, &OfficeMetricsParams {})
            .ok()?
            .ok()
    })();
    end_session(&mut wire, &session);
    answer
}
