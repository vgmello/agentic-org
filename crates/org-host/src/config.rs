//! How to run the host ([`Config`]): its trusted inputs, the office home and the office server
//! program, and the process hardening it starts with.

use std::io;
use std::path::{Path, PathBuf};

use crate::error::HostError;
use crate::logs::DEFAULT_LOG_MAX_BYTES;
use crate::timings::Timings;
use crate::web::BrowserListener;
use crate::{home, instance, release};
use crate::{APP_TEAM_ID, SERVER_PROGRAM_NAME};

/// How to run the host.
///
/// A release build trusts only what it was built with: the app's bundle id
/// ([`APP_BUNDLE_ID`](crate::APP_BUNDLE_ID)) and team id ([`APP_TEAM_ID`]), its Keychain access group, and the office
/// server next to its own executable. So the trust inputs can't be set from outside: the only
/// release constructor is [`Config::trusted`], and the setters that change them
/// ([`Config::new`], [`Config::set_server_program`], [`Config::set_app_team_id`] and the
/// development fields) exist in debug builds only. The timings and limits stay settable.
#[derive(Debug, Clone)]
pub struct Config {
    pub(crate) runtime_release: Option<release::RuntimeRelease>,
    /// The office home (absolute).
    pub(crate) home: PathBuf,
    /// The office server program (see [`default_server_program`]).
    pub(crate) server_program: PathBuf,
    pub timings: Timings,
    /// The size at which an office's stderr log is rotated.
    pub log_max_bytes: u64,
    /// Client connections beyond this are closed on arrival.
    pub max_connections: usize,
    /// The browser listener setup bound for this instance, held since setup checked its
    /// address (`org_host_setup::run` returns it in the foreground mode): the web process's
    /// first start serves on it instead of binding the address again. A listener that is
    /// not on the instance's address and port is refused by that start.
    pub browser_listener: Option<BrowserListener>,
    /// The TLS pair setup's configuration supplied for an `https` listener, imported at the
    /// instance's first start (`web_tls`). Set by the registered start from its record.
    pub(crate) supplied_tls: Option<instance::setup::SuppliedTls>,
    /// Optional HQ mTLS federation listener configuration from the instance's setup record.
    pub(crate) federation: Option<instance::setup::FederationConfig>,
    /// Signing policy reserved for CLI sessions in 2b.3; it enables no retired host calls.
    pub(crate) app_team_id: Option<String>,
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
            browser_listener: None,
            supplied_tls: None,
            federation: None,
            app_team_id: app_team_id.map(str::to_owned),
            #[cfg(debug_assertions)]
            dev_app_path: None,
            #[cfg(debug_assertions)]
            dev_key_dir: None,
        }
    }

    /// The service account is an ordinary user on every system: the service refuses to run
    /// as root (ADR-0017 decision 10). Every composition that runs the service calls this
    /// first, before anything is opened or created. It is not part of [`Host::start`](crate::Host::start), so a
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
    pub(super) fn admit_runtime(&mut self) -> Result<(), HostError> {
        self.admit_runtime_with(false)
    }
    /// [`Config::admit_runtime`] for a start whose setup record names a credential directory
    /// that the host opens (`recorded_custody`): in a development build that names the
    /// build's custody as a development key folder does.
    pub(super) fn admit_runtime_with(&mut self, recorded_custody: bool) -> Result<(), HostError> {
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
        // in a release Config. It establishes no production signing evidence. A credential
        // directory the setup record names is custody named as explicitly.
        #[cfg(debug_assertions)]
        if self.dev_key_dir.is_some() || recorded_custody {
            crate::host_log::log("explicit debug development custody: unpinned fixture runtime; no production release admission");
            return Ok(());
        }
        #[cfg(not(debug_assertions))]
        let _ = recorded_custody;
        Err(fail(release::Error::MissingBuildTrust))
    }
}

/// Why a process whose effective user is `euid` may not run the service, if it may not.
pub(super) fn root_refusal(euid: u32) -> Result<(), String> {
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
pub(super) fn harden() -> Result<(), HostError> {
    harden_process().map_err(|source| HostError::Io {
        context: "closing this process to other processes before any key is read",
        source,
    })
}

/// The office home the shared rule gives in this environment: `ORG_OFFICE_HOME` when it is an
/// absolute path, else this system's application base for the home folder `HOME` names. The
/// app and `agent-os` find `host.sock` by the same rule.
pub fn default_office_home() -> Option<PathBuf> {
    home::default_office_home()
}

/// The account's default protected release-store folder (`<trusted-app-base>/release-store`),
/// used by `agentic-org release install|status` and release-build `setup` / `serve` admission.
pub fn default_release_store_dir() -> Result<PathBuf, String> {
    trusted_office_home().map(|home| home.join("release-store"))
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
