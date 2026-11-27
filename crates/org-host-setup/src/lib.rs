//! `agentic-org setup` (A2 B4, SRV-2..SRV-7, ENC-3): the steps, in order, with the platform's
//! service manager and disk-encryption probe injected by the caller.
//!
//! 1. Platform gate: a release build without a pinned runtime release refuses (SRV-1); the
//!    caller's [`org_host::human::paths`] refuses systems without instance locations.
//! 2. Lock and freeze the setup record (`setup.lock`). The address is chosen before anything
//!    is written: port 7420 for `default`, the first free port from 7421 for a named instance;
//!    an occupied requested port is an actionable error and nothing is saved (SRV-4).
//! 3. Reserve the instance, record its listener and prepare the layout. Where a release
//!    host keeps its secrets in a credential directory (Linux, ADR-0017 decision 8), make
//!    that directory, check it as the host will, and record its absolute path with the
//!    instance: once, here, so that the host never derives it from its environment.
//! 4. Report disk encryption and the folder modes, read back from disk (ENC-3).
//! 5. Register the service and start it unless it already runs (SRV-6).
//! 6. Wait until `host.sock` answers hello and `instance.status` for this instance.
//! 7. Ask for the initial registration link once (`admin.link.initial`) and print it, only
//!    when the output is a terminal. Noninteractive setup asks for no link and names
//!    `agentic-org admin reset` instead (ADR-0022, UX §4a.2).
//!
//! Every step is idempotent; a second run reuses the frozen record and prints no link.
//! Ports other instances saved are never chosen, and a saved port another program holds
//! is reported before the service starts.
//!
//! Before step 1, [`run`] asks the service manager whether it answers at all (a Linux
//! machine or container without a systemd user manager is refused there, with the
//! foreground mode named) and makes the account folders a fresh Linux account lacks.
//!
//! Non-interactive setup ([`configuration`], [`Request::configuration`]) takes step 2's
//! whole input from a strict file instead of choosing a port, is offline, and never asks
//! for a link. With the file's service mode `foreground` (a container) the credential
//! directory of step 3 is a mounted volume that setup checks and never makes, and setup
//! ends after step 4: nothing is registered, started or waited for, and no service manager
//! is asked anything. `agentic-org serve --setup <file>` runs exactly that and then serves.
#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::io::Write;
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};

use org_host::browser_listener::published_http_notice;

use org_host::instance::registry::Registry;
pub use org_host::instance::setup::FederationConfig;
use org_host::instance::setup::{Input, Journal, Paths, Service};
pub use org_host_control::InvitationTokenSecret;

pub mod cli_context;
pub(crate) mod cli_session;
pub(crate) mod federation_command;
#[cfg(all(feature = "test-support", debug_assertions))]
pub mod fixture;
pub(crate) mod office_command;
pub(crate) mod org_command;
#[cfg(any(test, all(feature = "test-support", debug_assertions)))]
pub mod seed;
pub use federation_command::{
    office_approve, office_cancel, office_decline, office_invitations, office_invite,
    office_remove, office_sync_status, OfficeInviteInput,
};
pub use office_command::{
    office_join, office_list, office_re_enroll, office_remove_pending, office_restart,
    OfficeOperationCompleted, OfficeOperationOutcome, HELP as OFFICE_HELP,
};
pub use org_command::{
    org_create, org_list, validate_create_plan, OrgCreateInput, OrgCreateOutcome, HELP as ORG_HELP,
};
pub use org_host::cli_error::{
    kind_of, office_refused, refused_by, wire_name, HostCliError, HostCliErrorKind, OUTPUT_CLOSED,
};
pub(crate) mod project_command;
pub use project_command::{
    project_create, project_inspect, project_list, ProjectCreateInput, HELP as PROJECT_HELP,
};
pub(crate) mod provider_command;
pub use provider_command::{
    provider_bind, provider_bindings, provider_create, provider_list, provider_models,
    provider_refresh, ProviderBindInput, ProviderCreateInput, ProviderCreated,
    ProviderRefreshInput, HELP as PROVIDER_HELP,
};
pub mod sign_in_command;
pub use sign_in_command::{
    context_clear, context_show, context_use, open_for_command, sign_in_only,
    ApprovedOfficeChannel, ContextCleared, SignedIn, CONTEXT_HELP, SIGN_IN_HELP,
};
pub(crate) mod tls_command;
pub use tls_command::{admin_tls_set, TlsPairReplaced, HELP as ADMIN_TLS_SET_HELP};
mod service_command;
mod service_refresh;
/// The host-control types the commands answer with, so that the command line names them
/// without depending on `org-host-control` (the `org-proto` ones it names from there).
pub use org_host_control::ops::{OfficeJoinResult, OfficeJoinState, RecoveryState};
pub use org_host_control::organization::{
    CreatingOrganization, ListResult as OrganizationList, OrganizationView,
};
pub use org_host_control::OneTimeLinkUrl;
pub use service_command::{service, ServiceAction, ServiceReport, ServiceState};
pub use service_refresh::{InstallAttempt, RefreshOutcome, ServiceRefresh, ServiceRefreshReport};
mod account;
mod adapters;
mod address;
mod admin_reset;
#[cfg(test)]
mod cli_contract_tests;
mod credentials;
mod output;
mod protection;
mod registration;
mod report;
mod request;
#[cfg(test)]
mod tests;
mod wire;
use account::user_name;
pub(crate) use adapters::release_pin;
pub use adapters::{platform, service_manager, CredentialDirectory, Platform};
pub(crate) use address::{choose_port, loopback};
use address::{port_free, saved_ports, tls_preflight};
pub use admin_reset::{admin_reset, admin_reset_precheck, AdminLink, AdminResetLink};
use credentials::{development_folder, development_keys, open_records, preflight};
use output::{fail, say};
pub(crate) use protection::{mode_exceptions, protection_report};
use registration::{reserve_setup_instance, start_registered_setup};
pub(crate) use report::no_service_manager;
use report::{report_foreground_service, report_setup_summary};
use request::same_configuration;
pub use request::{
    configuration, Configuration, PortPrompt, Request, DEFAULT_PORT, NAMED_PORT_BASE,
};
pub use wire::instance_office_home;

#[cfg(all(feature = "test-support", debug_assertions))]
pub mod test_support {
    use std::io::Write;
    use std::time::Duration;

    use org_host::cli_error::HostCliError;
    use org_host::human::Wire;
    use org_host_control::ops::LinkedOrg;
    use org_host_control::SessionHandle;

    pub use crate::cli_session::Target;
    pub use crate::office_command::{list as office_list_at, restart as office_restart_at};
    pub use crate::project_command::{
        create as project_create_at, inspect as project_inspect_at, list as project_list_at,
        ProjectCreateInput,
    };
    pub use crate::sign_in_command::ApprovedOfficeChannel;
    pub use crate::tls_command::{set as tls_set_at, Target as TlsTarget};
    pub use crate::wire::registered_with_origin;

    pub fn approve_on_target(
        target: &Target<'_>,
        poll_interval: Duration,
        max_wait: Duration,
        out: &mut dyn Write,
    ) -> Result<(Wire, SessionHandle, String, Vec<LinkedOrg>), HostCliError> {
        crate::sign_in_command::approve_on_target(target, poll_interval, max_wait, out)
    }

    pub fn open_approved_office(
        wire: Wire,
        session: SessionHandle,
        office_id: String,
    ) -> Result<ApprovedOfficeChannel, HostCliError> {
        crate::sign_in_command::open_approved_office(wire, session, office_id)
    }
}

/// The environment variables that decided this account's instance locations in this
/// process, with their values: what a registered service must be given so that it resolves
/// the same paths as the setup that registered it (X5 review V7). A service manager starts
/// its units with its own environment, not the shell's that ran setup: without these, a
/// base moved by a variable is looked for elsewhere and the service never becomes ready.
///
/// The names and the rule are the system's (`org_platform::dirs::location_overrides`): a
/// variable that moves the application base, the user temp base or the default credential
/// directory away from its default, and none that names the default folder itself, so that
/// two shells that resolve the same locations render the same unit (X5 review F8); none
/// where nothing can move them.
pub fn path_environment() -> Vec<(OsString, OsString)> {
    let Some(home) = org_host::home::account_dir() else {
        return Vec::new();
    };
    org_platform::dirs::location_overrides(&home)
        .unwrap_or_default()
        .into_iter()
        .map(|(name, value)| (OsString::from(name), value))
        .collect()
}

/// INTERIM, in one place (X3 review, finding S4): making the directories of a credential
/// store below ancestors that were judged first.
///
/// The platform owns the store's ancestor rule (`org_platform_host::credentials`), and the
/// right home for this is a function of that crate that walks the path as
/// `release_directory` does and makes the missing directories below ancestors it judged:
///
/// ```text
/// pub fn create_release_directory(location: ReleaseDirectory<'_>)
///     -> Result<(), CredentialError>;
/// ```
///
/// (with the test entry `testing::credentials::create_release_directory_below(base, ..)`).
/// Until it exists, this module makes nothing below a chain it did not judge itself: it
/// walks from the start one component at a time with `openat(O_DIRECTORY | O_NOFOLLOW)`,
/// so no symbolic link is followed; every existing directory on the way must be owned by
/// root or this user, must not be writable by others, and must not be the data root; and
/// only then is each missing directory made with `mkdirat` (mode 700) below the directory
/// it holds open. These are the rules that hold on every system. The platform's own check
/// runs afterwards on the whole path (the group rule, the filesystem, the mode, the mount),
/// and [`Made::remove`](store_directories::Made::remove) takes away what was made here when that check refuses.
mod store_directories;

/// Runs setup for `request`. See the module docs for the steps.
///
/// A foreground setup from a file that reserved the instance in this run returns the
/// instance's browser listener, bound since setup checked the address: the process that
/// serves the instance next ([`org_host::Config::browser_listener`]) serves on it, so the port
/// stays this instance's from the check on. Every other run returns none.
pub fn run(
    platform: &Platform,
    request: Request<'_>,
    out: &mut dyn Write,
) -> Result<Option<org_host::BrowserListener>, String> {
    // A release build without a pinned runtime refuses before any path is resolved or made.
    release_pin()?;
    service_gate(platform, &request)?;
    account_folders();
    let paths = org_host::human::paths(request.name.as_deref())?;
    run_at(&paths, platform, request, out)
}

/// A system whose service manager does not answer (a Linux machine or container without a
/// systemd user manager) is refused before anything is made or saved, with the way to run
/// without one. The foreground mode never asks a service manager anything.
fn service_gate(platform: &Platform, request: &Request<'_>) -> Result<(), String> {
    if request
        .configuration
        .as_ref()
        .is_some_and(Configuration::foreground)
    {
        return Ok(());
    }
    platform.service.available().map_err(no_service_manager)
}

/// Makes the account's own folders that the instance locations need and that a fresh
/// account does not have: the folder the application base is in and the user temp base,
/// where the system puts them below the account's home (on Linux `~/.local/share` and
/// `~/.cache/agentic-org/tmp`; an account or a container's service user that never ran a
/// desktop session has neither; on macOS only a missing `~/Library/Application Support`).
/// Each missing folder is made with mode 700, one component at a time below the home,
/// without following a link below it. A location outside the home (an `XDG_*` variable
/// that points elsewhere) is made the same way below its deepest folder that exists, which
/// must be root's or this user's and not writable by others (X5 review, fourth round,
/// P2-3): a clean machine with such a variable needs nothing made by hand. The system's own
/// temp folder on macOS exists, and a development base makes its own folders.
///
/// This step refuses nothing. The home is resolved as the system resolves it (a home that
/// is itself a symbolic link, as network and managed homes are, is followed), and where a
/// folder cannot be made here (a link below the home, a folder of another user) nothing is
/// made and the instance locations are judged by their own rules right after, exactly as
/// they were before this step existed.
fn account_folders() {
    if std::env::var_os(org_host::human::DEV_ACCOUNT_BASE).is_some_and(|base| !base.is_empty()) {
        return;
    }
    let Some(home) = org_host::home::account_dir() else {
        return;
    };
    let Ok(resolved) = std::fs::canonicalize(&home) else {
        return;
    };
    let base = org_platform::dirs::application_base(&home).ok();
    let wanted = [
        base.as_deref().and_then(Path::parent).map(Path::to_owned),
        org_platform::dirs::user_temp_base(&home).ok(),
    ];
    make_folders(&home, &resolved, wanted.into_iter().flatten());
}

/// The folders `wanted` of [`account_folders`], for the account whose home is `home`, which
/// the system resolves to `resolved`.
fn make_folders(home: &Path, resolved: &Path, wanted: impl IntoIterator<Item = PathBuf>) {
    for folder in wanted {
        let Ok(folder) = org_host::instance::setup::normalized(&folder) else {
            continue;
        };
        if let Ok(below) = folder.strip_prefix(home) {
            let _ = store_directories::make(&resolved.join(below), None, resolved);
            continue;
        }
        // Outside the home: below the deepest folder of the path that exists, as the
        // system resolves it.
        let existing = folder.ancestors().find(|at| at.is_dir());
        let anchor = existing.and_then(|at| Some((std::fs::canonicalize(at).ok()?, at)));
        if let Some((anchor, at)) = anchor {
            if let Ok(rest) = folder.strip_prefix(at) {
                let _ = store_directories::make(&anchor.join(rest), None, &anchor);
            }
        }
    }
}

pub(crate) fn run_at(
    paths: &Paths,
    platform: &Platform,
    mut request: Request<'_>,
    out: &mut dyn Write,
) -> Result<Option<org_host::BrowserListener>, String> {
    // 1. Platform gate. [`run`] has already asked the service manager whether it answers,
    // before any folder was made.
    let pin = release_pin()?;
    let name = request.name.as_deref();
    let shown = name.unwrap_or("default");
    say(out, format!("Setting up instance \"{shown}\"."))?;

    // 2. Lock and freeze the record. The address is decided before anything is written.
    let journal = Journal::open(&paths.base, name).map_err(|e| fail("opening setup", e))?;
    let records = open_records(&paths.base)?;
    let configured = request
        .configuration
        .take()
        .map(|Configuration(configured)| configured);
    let existing = journal.read().map_err(|e| fail("reading setup", e))?;
    // Before anything is saved or reserved: what a development build's registered service
    // needs, for the input this run continues or freezes.
    let development_needed = |input: &Input, recorded: Option<bool>| {
        development_keys(
            input,
            recorded,
            request.development_keys.as_deref(),
            platform,
        )
    };
    // A record without a reserved instance is what an earlier run froze and could not
    // continue. It pins nothing: a corrected file replaces it (X5 review V1). A record
    // that names its instance is the instance's configuration and never changes.
    let existing = match (existing, &configured) {
        (Some(record), Some(configured))
            if record.instance_id.is_none() && !same_configuration(configured, &record) =>
        {
            journal
                .abandon()
                .map_err(|e| fail("replacing an unfinished setup", e))?;
            None
        }
        (existing, _) => existing,
    };
    // Whether this run froze the record from a file: if a later step of the reservation
    // refuses, the record is taken away again, so that nothing this run saved remains.
    let mut froze_from_file = false;
    // The address of a file's configuration, held from its check until the instance is
    // reserved with it, and beyond (`preflight`).
    let mut held = None;
    // A development build's key folder, which its registered service needs (made below).
    let development;
    // What setup says of an `https` listener's certificate: a supplied pair is verified
    // (and its fingerprint shown) when the input is first taken from its file.
    let mut tls_line = None;
    let mut record = match existing {
        Some(record)
            if configured
                .as_ref()
                .is_some_and(|configured| !same_configuration(configured, &record)) =>
        {
            return Err(format!(
                "instance \"{shown}\" was set up with a different configuration (service \
                 mode, data folder, credential directory, federation or address); its \
                 configuration does not change on a later run; nothing was changed"
            ));
        }
        Some(record) => {
            // A reserved instance is judged by what its record names, which never changes:
            // one reserved without a credential directory (by an earlier build) keeps its
            // keys in the development key folder whatever this build would record.
            development = development_needed(
                &record.input,
                record
                    .instance_id
                    .is_some()
                    .then_some(record.credentials.is_some()),
            )?;
            if request
                .port
                .is_some_and(|p| p != record.input.listen.port())
            {
                return Err(format!(
                    "this instance was set up on port {}; its address does not change on a \
                     later run; nothing was changed",
                    record.input.listen.port()
                ));
            }
            if request.federation.is_some() && request.federation != record.input.federation {
                return Err(
                    "this instance was set up with a different federation configuration; its \
                     configuration does not change on a later run; nothing was changed"
                        .into(),
                );
            }
            // The same file again, after a run that froze the record and stopped before it
            // reserved the instance: asked again, and its address held, as on a first run.
            if configured.is_some() && record.instance_id.is_none() {
                tls_line = tls_preflight(&record.input, record.tls.as_ref())?;
                held = Some(preflight(&record.input, records.as_fd())?);
            }
            record
        }
        None if configured.is_some() => {
            // Offline: nothing is asked and nothing is fetched. The address is the file's.
            let configured = configured.expect("checked above");
            // Everything that can refuse this input is asked before anything is saved.
            development = development_needed(&configured.input, None)?;
            tls_line = tls_preflight(&configured.input, configured.tls.as_ref())?;
            held = Some(preflight(&configured.input, records.as_fd())?);
            froze_from_file = true;
            journal
                .freeze_configured(configured, true, pin)
                .map_err(|e| fail("saving setup", e))?
        }
        None => {
            let saved = saved_ports(paths, name);
            let mut port = choose_port(
                name,
                request.port,
                request.named_port_base,
                port_free,
                &saved,
            )?;
            if request.port.is_none() {
                if let Some(prompt) = request.prompt.as_mut() {
                    if let Some(chosen) = prompt(&format!("http://127.0.0.1:{port}"))? {
                        port = choose_port(
                            name,
                            Some(chosen),
                            request.named_port_base,
                            port_free,
                            &saved,
                        )?;
                    }
                }
            }
            if let Some(federation) = &request.federation {
                federation
                    .validate()
                    .map_err(|e| fail("validating federation", e))?;
            }
            let root = paths
                .instance_root(name)
                .map_err(|e| fail("preparing the instance folder", e))?;
            let input = Input {
                version: 1,
                service: Service::Registered,
                root,
                credentials: None,
                listen: loopback(port)?,
                federation: request.federation.clone(),
            };
            development = development_needed(&input, None)?;
            journal
                .freeze(input, false, pin)
                .map_err(|e| fail("saving setup", e))?
        }
    };
    report_setup_summary(out, &record, tls_line)?;

    // 3. Reserve the instance, record its listener and prepare its layout, then record its
    // ID. Once the ID is recorded all three are done, and a running host holds the root lock,
    // so a later run only locates the instance. The root lock is released before the service
    // starts, since the host reserves the instance again.
    let registry = Registry::open(&paths.locator, &paths.user_temp)
        .map_err(|e| fail("opening the instance locator", e))?;
    let reserved_now = record.instance_id.is_none();
    let (instance_id, socket) = reserve_setup_instance(
        &registry,
        &journal,
        platform,
        records.as_fd(),
        name,
        &mut record,
        froze_from_file,
    )?;
    if let Some(credentials) = &record.credentials {
        say(out, format!("Secrets: {}", credentials.display()))?;
    }
    if record.input.listen.scheme() == org_host_control::listen::ListenMode::HttpPublished {
        say(
            out,
            format!(
                "Browser access: {}",
                published_http_notice(&record.input.listen)
            ),
        )?;
    }

    // 4. ENC-3, with the modes read back from disk: by the run that reserves the instance
    // and by every run of a registered one. A foreground start of an instance that is set up
    // already (every later start of a container) does not walk its data (X5 review F5).
    if reserved_now || record.input.service == Service::Registered {
        let (exceptions, unreadable) = mode_exceptions(&record.input.root)?;
        for line in protection_report(
            &platform.disk.state(&record.input.root),
            org_platform::disk::wording(),
            &user_name(),
            &exceptions,
            &unreadable,
        ) {
            say(out, line)?;
        }
    }

    // 5. The service. In the foreground mode nothing is registered, started or waited
    // for, and no link is asked for: whoever runs `serve` is the supervisor, and the
    // administrator's first link comes from `admin reset`.
    if record.input.service == Service::Foreground {
        report_foreground_service(out, shown, name)?;
        return Ok(held);
    }
    // The service binds the address itself once it is started.
    drop(held);
    if let Some(folder) = &development {
        development_folder(folder, &record.input.root)?;
    }
    start_registered_setup(
        platform,
        request,
        out,
        &journal,
        &mut record,
        &instance_id,
        &socket,
    )
    .map(|()| None)
}
