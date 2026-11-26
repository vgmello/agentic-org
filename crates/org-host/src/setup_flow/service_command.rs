//! `agentic-org service start|stop|status`: the registered instance's service, through the
//! platform's service manager, answered as a [`ServiceReport`] for the command line to show.

use org_platform::service::{ServiceError, ServiceManager, ServiceStatus};
use serde::Serialize;

use crate::instance::setup::Service;

use super::account::user_name;
use super::output::fail;
use super::report::{lingering_report, service_id};
use super::wire::{connect, registered};
use crate::cli_error::HostCliError;

/// `agentic-org service start|stop|status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceAction {
    Start,
    Stop,
    Status,
}

/// What [`service`] found or did, for the command line to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServiceReport {
    /// The instance's name, `default` when unnamed.
    pub instance: String,
    pub action: ServiceAction,
    pub state: ServiceState,
    /// The service's process, when the service manager says it runs.
    pub pid: Option<u32>,
    /// The instance runs in the foreground mode (`serve --setup <file>`): no service is
    /// registered, and the state is whether its own socket answers.
    pub foreground: bool,
    /// What `status` adds about a registered service: whether it keeps running after its
    /// user logs out (Linux host design section 3.4), as sentences; the command it names is
    /// printed, never run.
    pub notes: Vec<String>,
}

/// The state [`service`] reports: what `status` found, or what `start` or `stop` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    /// `status`: running (`pid` when the service manager knows it); in the foreground mode,
    /// the instance's socket answers.
    Running,
    /// `status`: registered and loaded, not running; in the foreground mode, not running.
    Stopped,
    /// `status`: registered, not loaded by the service manager.
    Unloaded,
    /// `status`: no service is registered.
    NotRegistered,
    /// `start`: the service manager started the service.
    Started,
    /// `stop`: the service manager stopped the service.
    StoppedByRequest,
}

/// `agentic-org service start|stop|status` for the registered instance `name`. Start or
/// stop of a foreground-mode instance is refused (`Usage`): it has no registered service.
/// A service manager that fails is `Runtime`.
pub fn service(
    name: Option<&str>,
    manager: &dyn ServiceManager,
    action: ServiceAction,
) -> Result<ServiceReport, HostCliError> {
    let (record, id, socket) = registered(&crate::human::paths(name)?, name)?;
    let shown = name.unwrap_or("default");
    let report = |state, pid, notes| ServiceReport {
        instance: shown.to_owned(),
        action,
        state,
        pid,
        foreground: record.input.service == Service::Foreground,
        notes,
    };
    if record.input.service == Service::Foreground {
        // Nothing is registered: the service manager is not asked. Whether the instance
        // runs is what its own socket says.
        return match action {
            ServiceAction::Status => Ok(report(
                match connect(&socket, &id) {
                    Ok(_) => ServiceState::Running,
                    Err(_) => ServiceState::Stopped,
                },
                None,
                Vec::new(),
            )),
            ServiceAction::Start | ServiceAction::Stop => Err(HostCliError::usage(format!(
                "instance \"{shown}\" runs in the foreground and has no registered service: \
                 whoever runs agentic-org serve --setup <file> starts and stops it (in a \
                 container, the container runtime)"
            ))),
        };
    }
    let id = service_id(&id);
    let failed = |doing: &str, why: ServiceError| HostCliError::runtime(fail(doing, why));
    match action {
        ServiceAction::Start => {
            manager
                .start(&id)
                .map_err(|e| failed("starting the service", e))?;
            Ok(report(ServiceState::Started, None, Vec::new()))
        }
        ServiceAction::Stop => {
            manager
                .stop(&id)
                .map_err(|e| failed("stopping the service", e))?;
            Ok(report(ServiceState::StoppedByRequest, None, Vec::new()))
        }
        ServiceAction::Status => {
            let status = manager
                .status(&id)
                .map_err(|e| failed("reading the service", e))?;
            let (state, pid) = match status {
                ServiceStatus::Running { pid } => (ServiceState::Running, Some(pid)),
                ServiceStatus::Stopped => (ServiceState::Stopped, None),
                ServiceStatus::Unloaded => (ServiceState::Unloaded, None),
                ServiceStatus::NotRegistered => (ServiceState::NotRegistered, None),
            };
            let notes = if status == ServiceStatus::NotRegistered {
                Vec::new()
            } else {
                lingering_report(&manager.lingering(), &user_name())
            };
            Ok(report(state, pid, notes))
        }
    }
}
