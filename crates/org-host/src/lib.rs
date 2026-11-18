//! # agentic-org: the login service, as a library
//!
//! The one per-user login service on a Mac (see `docs/design/offices-hq-and-satellites.md`,
//! "Processes on one Mac", and the host/server boundary design). It:
//! - supervises one `agentic-org-office` per office folder on this Mac, restarting a crashed
//!   one with backoff (`supervisor`),
//! - maintains the native office catalog and lifecycle, and
//! - serves registered-instance hello and count-only status over `org-host-control` on
//!   `<home>/host.sock` (`service`). Retired session-less operations are refused.
//!
//! Organization content stays in office databases: the catalog is the folders under `<home>/offices/` that hold
//! a valid `office.json` (`catalog`). It talks to its servers only through
//! `org-office-launch`, and never depends on `org-office`. The private `hostdb` stores the
//! host's registry in plain SQLite; organization databases remain in office servers.
//!
//! It holds every office's secrets key (`keys`): it creates each one, keeps it in its key store
//! (in release builds the system's protected store: the Data Protection keychain on macOS, the
//! credential directory setup recorded on Linux; a development folder in debug builds) and
//! hands it to the office's server on the launch pipe. Its servers never touch a key store.
//!
//! The agentic-org serve composition parses arguments, waits for shutdown and calls [`Host`].

// A release-profile build (or any build for the app bundle, `ORG_BUNDLE=1`) with debug
// assertions on would ship every development path (see build.rs): refuse it, whatever turned
// them on (the profile, `-C debug-assertions`, `-C opt-level=0`). rustdoc compiles with debug
// assertions on whatever the profile, so documentation and doctests are exempt: they ship nothing.
// (`doc` and `doctest` are set only by rustdoc; passing `--cfg doc` to rustc on purpose would
// bypass this, which takes intent, not a mistake.)
#[cfg(all(
    any(release_profile, bundle_build),
    debug_assertions,
    not(doc),
    not(doctest)
))]
compile_error!(
    "agentic-org: refusing a release or app-bundle build with debug assertions on: it would \
     ship the host's development paths (dev keys, no server signature check, the unsigned \
     development app, the trust-changing arguments). Build it without debug assertions."
);

mod accounts;
mod admin;
mod app_auth;
mod broadcast;
mod catalog;
mod clock;
mod cloexec;
mod code_signing;
mod error;
mod home;
mod host_log;
mod host_secrets;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "A2 B3 composes the registry into service start and setup"
    )
)]
mod hostdb;
pub mod human;
mod ids;
// Composed into `admin.link.initial` and `admin.reset` by A2 B3.
#[cfg_attr(not(test), allow(dead_code))]
mod links;
pub mod release;
mod web;

// Real-process tests (debug builds only).
#[cfg(all(test, debug_assertions))]
mod enc1_process_tests;
#[cfg(all(test, debug_assertions))]
mod web_process_tests;
// B2-01 foundation: compiled now; host wiring follows in the next reviewed slice.
#[cfg_attr(not(test), expect(dead_code, reason = "B2-01.2 wires the host"))]
mod instance;
mod keys;
mod logs;
mod offices;
mod peer;
mod provisioning;
mod recovery;
mod registered;
mod release_store;
mod secrets;
mod server_check;
mod service;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "A3 B4 wires browser sessions into the host ops")
)]
mod sessions;
pub mod setup_flow;
pub mod signals;
mod supervisor;
mod sync;
#[cfg(test)]
mod test_programs;
mod timings;
mod writer;

use std::fs;
use std::io;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use org_platform_host::credentials::ReleaseRoot;

use crate::app_auth::AppPolicy;
use crate::broadcast::Broadcast;
use crate::catalog::Catalog;
use crate::home::{HostLock, Layout};
use crate::offices::Offices;
use crate::server_check::ServerCheck;
use crate::service::listener::{self, BindError, Listener, ListenerConfig};
use crate::service::Service;
use crate::supervisor::{Supervisor, SupervisorConfig};

#[cfg(all(feature = "test-support", debug_assertions))]
pub mod test_support;

pub use crate::app_auth::APP_BUNDLE_ID;
pub use crate::server_check::OFFICE_SIGNING_ID;
pub use crate::server_check::WEB_SIGNING_ID;

/// The host's own signing identifier (ADR-0003 §2.1), for the bundle's signing of
/// the agentic-org executable in host mode. The signing identifier remains stable
/// across the executable migration; agent-os must never receive this entitlement.
pub const HOST_SIGNING_ID: &str = "com.vgmello.agentic-org.org-office-host";
pub use crate::error::HostError;
pub use crate::logs::DEFAULT_LOG_MAX_BYTES;
pub use crate::timings::Timings;

/// The office server's program name, looked for next to the host's own executable.
pub const SERVER_PROGRAM_NAME: &str = "agentic-org-office";

/// Requests one connection may have running at once.
const MAX_IN_FLIGHT_PER_CONNECTION: usize = 16;

/// The team id that signs the desktop app, fixed when the host is built (`ORG_APP_TEAM_ID`).
/// Without it no signed app can use the app-only calls (logged loudly at startup).
pub const APP_TEAM_ID: Option<&str> = option_env!("ORG_APP_TEAM_ID");

/// How to run the host.
///
/// A release build trusts only what it was built with: the app's bundle id
/// ([`APP_BUNDLE_ID`]) and team id ([`APP_TEAM_ID`]), its Keychain access group, and the office
/// server next to its own executable. So the trust inputs can't be set from outside: the only
/// release constructor is [`Config::trusted`], and the setters that change them
/// ([`Config::new`], [`Config::set_server_program`], [`Config::set_app_team_id`] and the
/// development fields) exist in debug builds only. The timings and limits stay settable.
#[derive(Debug, Clone)]
pub struct Config {
    runtime_release: Option<release::RuntimeRelease>,
    /// The office home (absolute).
    home: PathBuf,
    /// The office server program (see [`default_server_program`]).
    server_program: PathBuf,
    pub timings: Timings,
    /// The size at which an office's stderr log is rotated.
    pub log_max_bytes: u64,
    /// Client connections beyond this are closed on arrival.
    pub max_connections: usize,
    /// Signing policy reserved for CLI sessions in 2b.3; it enables no retired host calls.
    app_team_id: Option<String>,
    /// Debug builds only (release builds don't have the field): an unsigned development app
    /// (this exact executable, `Contents/MacOS/<exe>`, run by this user), reserved for CLI sessions.
    /// It enables no retired host calls. The binary sets it from the build-time constant
    /// `ORG_DEV_APP_PATH`, never from run-time input; tests set it directly. If the path doesn't
    /// exist when the host starts (the app isn't built yet) or isn't an executable file, that's
    /// logged and the fallback stays off. Every use is logged.
    #[cfg(debug_assertions)]
    pub dev_app_path: Option<PathBuf>,
    /// Debug builds only (release builds don't have the field): the folder (absolute) where the
    /// host keeps the offices' secrets keys, as plain files, instead of the Keychain. A debug
    /// host without it refuses to start ([`HostError::NoDevKeyDir`]), so it never writes a real
    /// Keychain item. The binary takes it from `ORG_OFFICE_DEV_KEY_DIR`; tests set it directly.
    /// It must not be inside the office home, and two homes must not share one.
    #[cfg(debug_assertions)]
    pub dev_key_dir: Option<PathBuf>,
}

impl Config {
    /// What this build trusts: the office home (`trusted_office_home`), the office server
    /// next to this executable, and the team id it was built with. The only way to make a
    /// release config.
    pub fn trusted() -> Result<Self, String> {
        let home = trusted_office_home()?;
        let server =
            default_server_program().map_err(|e| format!("finding the office server: {e}"))?;
        Ok(Self::with(home, server, APP_TEAM_ID))
    }

    /// Debug builds only: the agreed defaults for this home and server program, with no app
    /// team id.
    #[cfg(debug_assertions)]
    pub fn new(home: impl Into<PathBuf>, server_program: impl Into<PathBuf>) -> Self {
        Self::with(home.into(), server_program.into(), None)
    }

    fn with(home: PathBuf, server_program: PathBuf, app_team_id: Option<&str>) -> Self {
        Self {
            runtime_release: None,
            home,
            server_program,
            timings: Timings::default(),
            log_max_bytes: DEFAULT_LOG_MAX_BYTES,
            max_connections: 64,
            app_team_id: app_team_id.map(str::to_owned),
            #[cfg(debug_assertions)]
            dev_app_path: None,
            #[cfg(debug_assertions)]
            dev_key_dir: None,
        }
    }

    /// The service account is an ordinary user on every system: the service refuses to run
    /// as root (ADR-0017 decision 10). Every composition that runs the service calls this
    /// first, before anything is opened or created. It is not part of [`Host::start`], so a
    /// test can run a host as whatever user the tests run as. A command that only talks to a
    /// running service (`admin reset` run by root for the service user) is not the service
    /// and does not call it.
    pub fn refuse_root() -> Result<(), String> {
        root_refusal(home::euid())
    }

    /// Debug builds only: another office server program.
    #[cfg(debug_assertions)]
    pub fn set_server_program(&mut self, program: impl Into<PathBuf>) {
        self.server_program = program.into();
    }

    /// Debug builds only: another app team id (or none).
    #[cfg(debug_assertions)]
    pub fn set_app_team_id(&mut self, team_id: Option<String>) {
        self.app_team_id = team_id;
    }

    /// The office home.
    pub fn home(&self) -> &Path {
        &self.home
    }
    /// Bind registered host/bootstrap composition to an independently verified
    /// current release. There is no unverified root/key/version setter.
    pub fn with_runtime_release(
        mut self,
        release: release::RuntimeRelease,
    ) -> Result<Self, HostError> {
        self.runtime_release = Some(release);
        self.admit_runtime()?;
        Ok(self)
    }
    fn admit_runtime(&mut self) -> Result<(), HostError> {
        let fail = |e| HostError::Io {
            context: "admitting the complete runtime release before custody",
            source: io::Error::other(e),
        };
        if let Some(release) = &self.runtime_release {
            release.check_self().map_err(fail)?;
            self.server_program = release.office_program();
            return Ok(());
        }
        // Named development custody is already debug-only and cannot be present
        // in a release Config. It establishes no production signing evidence.
        #[cfg(debug_assertions)]
        if self.dev_key_dir.is_some() {
            host_log::log("explicit debug development custody: unpinned fixture runtime; no production release admission");
            return Ok(());
        }
        Err(fail(release::Error::MissingBuildTrust))
    }
}

/// Why a process whose effective user is `euid` may not run the service, if it may not.
fn root_refusal(euid: u32) -> Result<(), String> {
    if euid == 0 {
        return Err(
            "the service does not run as root: start it as the service user, an ordinary \
             account (a root-owned service could be read by nothing but root, and every \
             office worker and agent it starts would be root too)"
                .into(),
        );
    }
    Ok(())
}

/// Closes this process to the other processes of its user: no core dump, and where the
/// system has the attribute, not dumpable (`org_platform::process::harden_self`). The
/// executable calls it as its first action, before it reads an argument: a descriptor of
/// this process's memory that another process of the user opened earlier keeps working, so
/// the time before this call is kept as short as it can be. It can be called again and
/// changes nothing then.
pub fn harden_process() -> io::Result<()> {
    org_platform::process::harden_self()
}

/// [`harden_process`] before a host reads a key. Every way a host starts goes through it
/// first, whatever its executable did: a host started inside another program (a test) is
/// closed here.
fn harden() -> Result<(), HostError> {
    harden_process().map_err(|source| HostError::Io {
        context: "closing this process to other processes before any key is read",
        source,
    })
}

/// Temporary test entry for the private bootstrap while setup's CLI is gated.
#[cfg(all(feature = "test-support", debug_assertions))]
pub fn start_instance_for_test(
    config: Config,
    locator: &Path,
    user_temp: &Path,
    name: Option<&str>,
    root: &Path,
) -> Result<Host, HostError> {
    let registry =
        instance::registry::Registry::open(locator, user_temp).map_err(instance_error)?;
    Host::start_instance(config, &registry, name, root)
}

fn instance_error(error: instance::Error) -> HostError {
    HostError::Io {
        context: "preparing the instance",
        source: io::Error::other(error),
    }
}

/// The office home the shared rule gives in this environment: `ORG_OFFICE_HOME` when it is an
/// absolute path, else this system's application base for the home folder `HOME` names. The
/// app and `agent-os` find `host.sock` by the same rule.
pub fn default_office_home() -> Option<PathBuf> {
    home::default_office_home()
}

/// The office home a trusted (release) host serves: its account's own office home, this
/// system's application base for `<pw_dir>`, and only when the shared rule
/// ([`default_office_home`]: `ORG_OFFICE_HOME`, else the base for `$HOME`), the one the app and
/// `agent-os` use to find `host.sock`, gives that same home in this environment
/// ([`home::pinned_office_home`]). Otherwise the host doesn't start, saying why: its home can't
/// be moved by `ORG_OFFICE_HOME` or `HOME`, and it never serves a home its clients don't look
/// in.
fn trusted_office_home() -> Result<PathBuf, String> {
    let home_env = std::env::var_os("HOME").map(PathBuf::from);
    home::pinned_office_home(
        home::default_office_home().as_deref(),
        home_env.as_deref(),
        home::account_dir().as_deref(),
        home::office_home_in,
        home::same_path,
    )
}

/// `agentic-org-office` next to this executable (both live in the app bundle), found through the
/// executable's canonical path (symlinks resolved). Defence in depth only: a hard link or a
/// copied bundle still puts another server next to the host, which is why a release host checks
/// every server's code signature before it gives it a key (`server_check`).
pub fn default_server_program() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?.canonicalize()?;
    let dir = exe
        .parent()
        .ok_or_else(|| io::Error::other("the host's executable has no folder"))?;
    Ok(dir.join(SERVER_PROGRAM_NAME))
}

/// A running host. [`Host::shutdown`] (or dropping it) stops it.
pub struct Host {
    _runtime_release: Option<release::RuntimeRelease>,
    layout: Layout,
    offices: Arc<Offices>,
    listener: Option<Listener>,
    lock: Option<HostLock>,
    instance: Option<instance::registry::Instance>,
    authentication_lifecycle: Option<service::authentication::Lifecycle>,
    web: Option<web::Supervisor>,
    service: Arc<Service>,
    /// Organization creation, kept to end its threads when the host stops.
    creating: Option<Arc<provisioning::Provisioning>>,
}

fn startup_server_check(config: &Config) -> Result<ServerCheck, HostError> {
    let server_check =
        ServerCheck::for_build(cfg!(debug_assertions), config.app_team_id.as_deref())
            .map_err(|why| HostError::InvalidAppIdentity { why })?;
    let server_check = match &config.runtime_release {
        Some(release) => {
            server_check.pinned(release.image_pin(SERVER_PROGRAM_NAME).map_err(|e| {
                HostError::Io {
                    context: "binding office image",
                    source: io::Error::other(e),
                }
            })?)
        }
        None => server_check,
    };
    host_log::log(&server_check.describe());
    Ok(server_check)
}

fn startup_listener(
    bound: std::os::unix::net::UnixListener,
    service: &Arc<Service>,
    offices: &Offices,
    max_connections: usize,
) -> Result<Listener, HostError> {
    let listener = {
        let config = ListenerConfig {
            max_connections,
            max_in_flight: MAX_IN_FLIGHT_PER_CONNECTION,
            expected_uid: org_socket::effective_uid(),
        };
        Listener::start(bound, config, service.clone()).map_err(|source| HostError::Io {
            context: "starting the listener",
            source,
        })
    };
    let listener = match listener {
        Ok(listener) => listener,
        Err(e) => {
            offices.shutdown();
            return Err(e);
        }
    };
    Ok(listener)
}

fn startup_keys(
    config: &Config,
    instance: &Option<instance::registry::Instance>,
    dev_key_dir: Option<&Path>,
    release_directory: Option<&ReleaseRoot>,
) -> Result<Arc<dyn keys::OfficeSecretsKeys>, HostError> {
    let keys = keys::store_for(
        dev_key_dir,
        instance
            .as_ref()
            .map_or(config.home.as_path(), |i| i.root()),
        instance.as_ref().map(|i| i.id()),
        code_signing::own_signing,
        release_directory,
    )
    .map_err(|e| match e {
        keys::StoreError::NoDevKeyDir => HostError::NoDevKeyDir,
        keys::StoreError::NoKeychainGroup => HostError::NoKeychainGroup,
        keys::StoreError::KeychainGroupNotEntitled { group } => {
            HostError::KeychainGroupNotEntitled { group }
        }
        keys::StoreError::NotHardened => HostError::NotHardened,
        keys::StoreError::NoCredentialDirectory => HostError::Io {
            context: "choosing the key store",
            source: io::Error::other(NO_CREDENTIAL_DIRECTORY),
        },
        keys::StoreError::CredentialDirectory(why) => HostError::Io {
            context: "opening the office keys in the credential directory",
            source: io::Error::other(why),
        },
    })?;
    Ok(keys)
}

/// What a release host says when it keeps its keys in a credential directory and has none:
/// only a registered instance has one, at the path its setup recorded.
const NO_CREDENTIAL_DIRECTORY: &str = "this host keeps the offices' keys in the credential \
     directory that setup records, and it serves no registered instance: run agentic-org \
     setup, then agentic-org serve --name <instance>";

fn startup_app_policy(config: &Config, selected: bool) -> Result<AppPolicy, HostError> {
    // Registered selected instances admit the same human CLI/host image as
    // protected reverse bootstrap. Keep the retired desktop requirement
    // only on the legacy, unregistered host entry.
    let app = AppPolicy::new(
        if selected {
            HOST_SIGNING_ID
        } else {
            APP_BUNDLE_ID
        },
        config.app_team_id.as_deref(),
    );
    #[cfg(debug_assertions)]
    let app = app.and_then(|app| match &config.dev_app_path {
        Some(path) => app.with_dev_app(path),
        None => Ok(app),
    });
    let app = app.map_err(|why| HostError::InvalidAppIdentity { why })?;
    for line in app.describe() {
        host_log::log(&line);
    }
    Ok(app)
}

fn startup_authentication(
    layout: &Layout,
    instance: &Option<instance::registry::Instance>,
    custody: &mut Option<Arc<dyn host_secrets::HostSecrets>>,
    dev_key_dir: Option<&Path>,
    release_directory: Option<&ReleaseRoot>,
) -> Result<Option<Arc<service::authentication::Authentication>>, HostError> {
    let authentication = if let Some(instance) = instance {
        match fs::symlink_metadata(layout.home.join("host-registry.db")) {
            Ok(_) => {
                let custody = match custody.take() {
                    Some(custody) => custody,
                    None => host_secrets::store_for(
                        dev_key_dir,
                        instance.root(),
                        instance.id(),
                        code_signing::own_signing,
                        release_directory,
                    )
                    .map_err(|_| HostError::Io {
                        context: "opening protected account custody",
                        source: io::Error::other("protected custody unavailable"),
                    })?,
                };
                Some(Arc::new(
                    service::authentication::Authentication::open(
                        &layout.home,
                        custody,
                        instance.id(),
                    )
                    .map_err(|_| HostError::Io {
                        context: "opening protected account registry",
                        source: io::Error::other("protected registry unavailable"),
                    })?,
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(source) => {
                return Err(HostError::Io {
                    context: "checking protected account registry",
                    source,
                })
            }
        }
    } else {
        None
    };
    Ok(authentication)
}

fn startup_catalog(
    layout: &Layout,
    authentication: &Option<Arc<service::authentication::Authentication>>,
) -> Result<(Catalog, std::collections::BTreeSet<String>, Vec<String>), HostError> {
    let catalog = Catalog::scan(&layout.home).map_err(|source| HostError::Io {
        context: "reading the office folders",
        source,
    })?;
    let protected_offices = match authentication {
        Some(auth) => auth
            .provisioning_storage()
            .0
            .provisioning_offices()
            .map_err(|_| HostError::Io {
                context: "reading protected provisioning reservations",
                source: io::Error::other("provisioning recovery required"),
            })?,
        None => std::collections::BTreeSet::new(),
    };
    let mut unfinished =
        catalog::unfinished_folders(&layout.home).map_err(|source| HostError::Io {
            context: "reading the office folders",
            source,
        })?;
    unfinished.retain(|office| !protected_offices.contains(office));
    Ok((catalog, protected_offices, unfinished))
}

fn startup_web(
    config: &Config,
    instance: &Option<instance::registry::Instance>,
    layout: &Layout,
    dev_key_dir: Option<&Path>,
) -> Result<Option<web::Configuration>, HostError> {
    let Some(instance) = instance else {
        return Ok(None);
    };
    let Some(listen) = instance.browser().map_err(instance_error)? else {
        return Ok(None);
    };
    let (check, release) = web::admission(
        cfg!(debug_assertions),
        config.app_team_id.as_deref(),
        config
            .runtime_release
            .as_ref()
            .map(|release| (release.image_pin("agentic-org-web"), release.web_binding())),
    );
    let mut critical_paths = vec![layout.home.clone(), instance.root().to_owned()];
    if let Some(dir) = dev_key_dir {
        critical_paths.push(dir.to_owned());
    }
    Ok(Some(web::Configuration {
        release,
        program: config.server_program.with_file_name("agentic-org-web"),
        instance: instance.id().into(),
        runtime: layout.run.clone(),
        listen,
        check,
        critical_paths,
    }))
}

struct ProtectedServices {
    writers: Option<Arc<writer::Writers>>,
    supervisor: Supervisor,
    provisioning: Option<Arc<provisioning::Provisioning>>,
}

fn startup_protected_services(
    instance: &Option<instance::registry::Instance>,
    authentication: &Option<Arc<service::authentication::Authentication>>,
    supervisor: Supervisor,
    keys: &Arc<dyn keys::OfficeSecretsKeys>,
    layout: &Layout,
    timings: &Timings,
) -> Result<ProtectedServices, HostError> {
    let io = |context| move |source| HostError::Io { context, source };
    let writers = match (instance, authentication) {
        (Some(instance), Some(authentication)) => Some(
            authentication
                .writers(instance.id(), instance.root())
                .map_err(io("opening protected writer authority"))?,
        ),
        _ => None,
    };
    let supervisor = match &writers {
        Some(writers) => supervisor.with_writers(Arc::clone(writers)),
        None => supervisor,
    };
    let provisioning = match (instance, authentication, &writers) {
        (Some(instance), Some(authentication), Some(writers)) => {
            let (db, custody) = authentication.provisioning_storage();
            Some(Arc::new(
                provisioning::Provisioning::new(
                    db,
                    custody,
                    Arc::clone(keys),
                    Arc::clone(writers),
                    instance.id(),
                    instance.root(),
                    &layout.home,
                )
                .with_shutdown_grace(timings.shutdown_grace),
            ))
        }
        _ => None,
    };
    Ok(ProtectedServices {
        writers,
        supervisor,
        provisioning,
    })
}

impl Host {
    /// Starts the host: chooses its key store, checks the socket paths fit, makes the office
    /// home owner-only, takes `host.lock` and binds `host.sock`. This legacy entry has no
    /// registered instance: it starts and discards no office server, logs that setup must run
    /// first and refuses every valid `host.sock` request. Registered
    /// entry points serve only hello and count-only status, with hello required first.
    pub fn start(config: Config) -> Result<Self, HostError> {
        Self::start_inner(config, None)
    }

    /// Private composition entry for the matching debug test-support adapter.
    #[cfg(all(feature = "test-support", debug_assertions))]
    fn start_instance(
        config: Config,
        registry: &instance::registry::Registry,
        name: Option<&str>,
        root: &Path,
    ) -> Result<Self, HostError> {
        registry.restore_temp_folders().map_err(instance_error)?;
        let instance = registry.reserve(name, root).map_err(instance_error)?;
        Self::start_inner(config, Some(instance))
    }

    fn start_inner(
        config: Config,
        instance: Option<instance::registry::Instance>,
    ) -> Result<Self, HostError> {
        Self::start_prepared(config, instance, None)
    }

    fn start_prepared(
        config: Config,
        instance: Option<instance::registry::Instance>,
        prepared: Option<(HostLock, Arc<dyn host_secrets::HostSecrets>)>,
    ) -> Result<Self, HostError> {
        Self::start_with(config, instance, prepared, None)
    }

    /// The composition. `release_directory` is the credential directory a registered start
    /// opened, where a release build keeps its secrets in one: the office keys are opened
    /// below it, and each view keeps it locked for as long as the view lives.
    fn start_with(
        mut config: Config,
        instance: Option<instance::registry::Instance>,
        prepared: Option<(HostLock, Arc<dyn host_secrets::HostSecrets>)>,
        release_directory: Option<ReleaseRoot>,
    ) -> Result<Self, HostError> {
        harden()?;
        if instance.is_some() {
            config.admit_runtime()?;
        }
        let layout = match &instance {
            Some(instance) => {
                let paths = instance.prepare_layout().map_err(instance_error)?;
                config.home = paths.office_home;
                Layout::with_runtime(&config.home, instance.temp_folder())
            }
            None => Layout::new(&config.home),
        };

        if !config.home.is_absolute() {
            return Err(HostError::RelativeHome { path: config.home });
        }
        let app = startup_app_policy(&config, instance.is_some())?;
        // Before anything is made on disk or started: no office runs without its key store.
        #[cfg(debug_assertions)]
        let dev_key_dir = config.dev_key_dir.as_deref();
        #[cfg(not(debug_assertions))]
        let dev_key_dir = None;
        let keys = startup_keys(&config, &instance, dev_key_dir, release_directory.as_ref())?;
        layout.check_socket_paths()?;
        layout.prepare()?;
        let (held, mut custody) = prepared.unzip();
        let lock = match held {
            Some(lock) => lock,
            None => HostLock::acquire(&layout)?,
        };
        // Decided only by the host that holds the home.
        // An unregistered host reads no catalog, so it may never judge a key an orphan.
        let sweep_orphans = instance.is_some();
        let server_check = startup_server_check(&config)?;
        let io = |context| move |source| HostError::Io { context, source };
        let authentication = startup_authentication(
            &layout,
            &instance,
            &mut custody,
            dev_key_dir,
            release_directory.as_ref(),
        )?;
        // From here on the views hold the credential directory: this handle is not needed
        // again and goes when the composition returns.
        let authentication_lifecycle = authentication
            .as_ref()
            .map(service::authentication::Lifecycle::start)
            .transpose()
            .map_err(io("starting session lifecycle"))?;
        // An unregistered host starts and discards no office server: setup must run first.
        let (catalog, protected_offices, unfinished) = match &instance {
            Some(_) => startup_catalog(&layout, &authentication)?,
            None => {
                host_log::log(
                    "no registered instance on this host: setup must run first; no office \
                     server is started or discarded",
                );
                Default::default()
            }
        };
        // Bound first, so nothing has started when binding fails; clients that connect before
        // the listener starts (moments later) wait in the backlog.
        let bound = listener::bind(&layout.host_socket).map_err(|e| match e {
            BindError::NotASocket => HostError::SocketInTheWay {
                path: layout.host_socket.clone(),
            },
            BindError::Io(source) => HostError::Io {
                context: "binding host.sock",
                source,
            },
        })?;

        let supervisor_config = SupervisorConfig {
            program: config.server_program.clone(),
            home: layout.home.clone(),
            runtime_dir: Some(layout.run.clone()),
            logs_dir: layout.logs.clone(),
            log_max_bytes: config.log_max_bytes,
            timings: config.timings.clone(),
            keys: Arc::clone(&keys),
            server_check: Arc::new(server_check),
            sweep_orphans,
        };
        let supervisor = match &instance {
            Some(instance) => Supervisor::new(supervisor_config, instance.id()),
            None => Supervisor::unregistered(supervisor_config),
        }
        .with_protected_offices(protected_offices);
        let ProtectedServices {
            writers: _writers,
            supervisor,
            provisioning,
        } = startup_protected_services(
            &instance,
            &authentication,
            supervisor,
            &keys,
            &layout,
            &config.timings,
        )?;
        let broadcast = Arc::new(Broadcast::default());
        let offices = Offices::start(
            &layout.home,
            catalog,
            unfinished,
            supervisor,
            Arc::clone(&broadcast),
            &config.timings,
        );

        let gateway_state = Arc::new(std::sync::Mutex::new(
            org_host_control::GatewayState::NotConfigured,
        ));
        let web_config = startup_web(&config, &instance, &layout, dev_key_dir)?;
        let recovery = match (&instance, &authentication) {
            (Some(instance), Some(authentication)) => Some(Arc::new(recovery::Recovery::new(
                Arc::clone(authentication),
                Arc::clone(&offices),
                Arc::clone(&broadcast),
                instance.id(),
                app,
            ))),
            _ => None,
        };
        let creating = provisioning.clone();
        let service = Arc::new(
            Service::new(
                Arc::clone(&offices),
                broadcast,
                instance.as_ref().map(|i| i.id().to_owned()),
            )
            .with_authentication(authentication)
            .with_provisioning(provisioning)
            .with_recovery(recovery)
            .with_gateway_state(Arc::clone(&gateway_state)),
        );
        let listener = startup_listener(bound, &service, &offices, config.max_connections)?;
        // A create that reserved its identity before the host last stopped continues now,
        // with no client asking; later ones start from the browser's request. This follows
        // the last step of the start that can fail: a start that is refused has started no
        // creation thread, so none outlives the refusal. Until this line a create or a
        // lookup that arrives on the new listener is answered `unavailable` and asked again.
        if let Some(provisioning) = &creating {
            provisioning.start(Arc::clone(&offices) as Arc<dyn provisioning::Hq>);
        }
        let web = web_config.and_then(|web_config| {
            match web::Supervisor::start(
                web_config,
                service.clone(),
                Arc::clone(&gateway_state),
                service.web_eligibility(),
            ) {
                Ok(web) => Some(web),
                Err(error) => {
                    *sync::lock(&gateway_state) = org_host_control::GatewayState::Failed;
                    host_log::log(&format!(
                        "browser service not started: {}",
                        host_log::untrusted(&error.to_string())
                    ));
                    None
                }
            }
        });
        Ok(Self {
            _runtime_release: config.runtime_release,
            layout,
            offices,
            listener: Some(listener),
            lock: Some(lock),
            instance,
            authentication_lifecycle,
            web,
            service,
            creating,
        })
    }

    /// The office home this host serves.
    pub fn home(&self) -> &Path {
        &self.layout.home
    }

    /// Whether the web process is serving: host authentication is present, HQ is
    /// Active and an admitted web process's control connection is live.
    pub fn web_ready(&self) -> bool {
        self.service.web_ready()
    }

    /// `host.sock`, where clients connect.
    pub fn socket_path(&self) -> &Path {
        &self.layout.host_socket
    }

    /// Stops accepting, stops every office server (stdin closed, then SIGKILL after the grace
    /// period), removes `host.sock` and releases `host.lock`.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        let Some(mut listener) = self.listener.take() else {
            return;
        };
        listener.close();
        // Ends the web process control connection (and its sessions), then kills/reaps it.
        self.web.take();
        // No creation attempt starts from here on, and none records a link.
        if let Some(creating) = &self.creating {
            creating.halt();
        }
        // Before joining the listener: a create still running answers as soon as the
        // supervisor stops its server.
        self.offices.shutdown();
        listener.join();
        // Nothing of a create writes custody or the registry after this.
        if let Some(creating) = self.creating.take() {
            creating.stop();
        }
        self.authentication_lifecycle.take();
        if let Ok(meta) = fs::symlink_metadata(&self.layout.host_socket) {
            if meta.file_type().is_socket() {
                let _ = fs::remove_file(&self.layout.host_socket);
            }
        }
        self.lock.take();
        self.instance.take();
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_root_is_refused_the_service() {
        let refusal = root_refusal(0).unwrap_err();
        assert!(refusal.contains("does not run as root"), "{refusal}");
        assert!(refusal.contains("service user"), "{refusal}");
        for user in [1, 501, 1000, 61000, u32::MAX] {
            assert_eq!(root_refusal(user), Ok(()), "{user}");
        }
        // The composition's check is this rule for the process's own effective user.
        assert_eq!(Config::refuse_root().is_err(), home::euid() == 0);
    }

    /// Set in the child of [`a_host_start_closes_the_process_before_it_reads_a_key`].
    #[cfg(debug_assertions)]
    const START_CHILD: &str = "ORG_HOST_TEST_START_CHILD";

    /// A host start closes its process to the other processes of its user before it reaches
    /// its key store: a start that is refused before any store is chosen (a relative home)
    /// has already done it. A child of the test starts the host, so the test binary itself is
    /// not changed for the other tests.
    #[cfg(debug_assertions)]
    #[test]
    fn a_host_start_closes_the_process_before_it_reads_a_key() {
        use org_platform::process::testing::hardened;
        if std::env::var_os(START_CHILD).is_some() {
            // A fresh process is not closed, unless its environment already forbids core
            // dumps on a system where that is all there is to it.
            if hardened().unwrap() {
                println!("this process was closed before the start: only the end is shown");
            }
            // No development key folder is set: reaching the key store would say so.
            let refused = Host::start(Config::new("relative/home", "/nonexistent/worker"));
            assert!(
                matches!(refused, Err(HostError::RelativeHome { .. })),
                "{:?}",
                refused.err().map(|e| e.to_string())
            );
            assert!(hardened().unwrap(), "the start did not close the process");
            println!("closed before the key store");
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::a_host_start_closes_the_process_before_it_reads_a_key",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(START_CHILD, "1")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && text.contains("closed before the key store"),
            "{text}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
