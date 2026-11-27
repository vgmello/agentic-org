//! What the person asked setup for: the request, its default ports and the strict
//! configuration file of non-interactive setup.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use org_host::instance::setup::FederationConfig;
use org_host::instance::setup::{Configured, Input, Record, Service};

/// The port of the `default` instance; named instances take the first free one after it.
pub const DEFAULT_PORT: u16 = 7420;

/// Where the search for a named instance's port starts. Production composition always
/// passes this; only a test-support build may pass another base ([`Request::named_port_base`]).
pub const NAMED_PORT_BASE: u16 = DEFAULT_PORT + 1;

/// Asked once, on the first run only, with the default address; `Ok(None)` accepts it.
pub type PortPrompt<'a> = &'a mut dyn FnMut(&str) -> Result<Option<u16>, String>;

/// What the person asked for.
pub struct Request<'a> {
    pub name: Option<String>,
    /// `--port`; without it the default is offered to `prompt` (if any) and then used.
    pub port: Option<u16>,
    pub prompt: Option<PortPrompt<'a>>,
    /// The first port tried for a named instance without `--port`: [`NAMED_PORT_BASE`],
    /// except in a test-support build whose tests keep away from the ports of other
    /// programs on the machine.
    pub named_port_base: u16,
    /// The service program (`agentic-org` itself) and its explicit environment.
    pub program: PathBuf,
    pub env: Vec<(OsString, OsString)>,
    /// Whether `out` is a terminal. The one-time link is printed only to a terminal.
    pub terminal: bool,
    /// A development build's key folder, which the service is given
    /// (`ORG_OFFICE_DEV_KEY_DIR`), absolute: where its host keeps its keys when the record
    /// names no credential directory (macOS). Setup makes it before it registers the
    /// service, and a development build's registered service without one is refused before
    /// anything is saved (X5 review V-a). Unused by a release build and in the foreground
    /// mode, whose host makes its own.
    pub development_keys: Option<PathBuf>,
    /// Non-interactive setup: the whole input comes from a file, nothing is asked, `port`
    /// and `prompt` are not used, and no link is asked for or printed (ADR-0022, UX §4a.2).
    pub configuration: Option<Configuration>,
    /// Optional HQ mTLS federation listener configuration for interactive/flag setup
    /// (`--federation-bind` / `--federation-advertise`). Unused when `configuration` is `Some`.
    pub federation: Option<FederationConfig>,
}

/// The configuration of non-interactive, offline setup (`setup --non-interactive --config
/// <file>`, `serve --setup <file>`): the setup record's own input, read from a strict JSON
/// file. See [`configuration`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Configuration(pub(super) Configured);

/// A configuration of `input` alone: no supplied pair.
#[cfg(test)]
impl From<Input> for Configuration {
    fn from(input: Input) -> Self {
        Configuration(Configured { input, tls: None })
    }
}

impl Configuration {
    /// Whether the instance runs in the foreground, with nothing registered.
    pub fn foreground(&self) -> bool {
        self.0.input.service == Service::Foreground
    }
}

/// Reads the setup configuration at `path`: one JSON object, version 1, strict (an
/// unknown or repeated property is refused), at most 64 KiB. Its properties:
///
/// - `version`: `1`.
/// - `service`: `"registered"` (the account's service manager) or `"foreground"` (nothing
///   is registered; whoever runs `agentic-org serve --setup` is the supervisor).
/// - `root`: the instance's data folder, absolute.
/// - `credentials`: the directory that holds the host's secrets, absolute. Required with
///   `foreground` on a system that keeps them in a directory (Linux), where it must be a
///   mounted volume that setup never makes; refused on a system that keeps them in the
///   Keychain.
/// - `listen`: `address`, `port`, `scheme`, `origin` and `hsts` (`"off"`, `"on"`,
///   `"clear"`), as the instance stores them. `scheme` is `"http_loopback"` (plain HTTP on
///   exactly `127.0.0.1` or `::1`) or `"http_published"`: plain HTTP on an
///   address that is not loopback, with `127.0.0.1` as the origin's host, accepted only with `"service": "foreground"` (a container whose port the
///   runtime publishes on the Docker host's loopback). It is never implied: `http_loopback`
///   with another address is refused. `"https"` serves TLS on any address, with a
///   certificate the instance makes for the origin's host at its first start.
/// - `tls` (with `https` only): `{"cert": <path>, "key": <path>}`, the absolute paths of a PEM
///   certificate chain (leaf first) and its unencrypted key, supplied instead of the
///   instance's own certificate. Setup reads and verifies them before it saves anything
///   (the key is the leaf's, the leaf is valid now and names the origin's host); the host
///   imports the pair into its secrets at its first start.
///
/// Nothing is created or changed by reading it. It must be a regular file: anything else (a
/// FIFO, a device, a directory) is refused when it is opened, without waiting for a writer,
/// so that a stop signal that arrives meanwhile is not held up behind it.
pub fn configuration(path: &Path) -> Result<Configuration, String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let shown = path.display();
    let mut bytes = Vec::new();
    std::fs::OpenOptions::new()
        .read(true)
        // Opening a FIFO without a writer would wait; with this it returns at once. A
        // terminal named here never becomes this process's controlling terminal.
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
        .open(path)
        .and_then(|file| {
            if !file.metadata()?.is_file() {
                return Err(std::io::Error::other("it is not a regular file"));
            }
            file.take(64 * 1024 + 1).read_to_end(&mut bytes)
        })
        .map_err(|e| format!("reading the setup configuration {shown}: {e}"))?;
    let configured = Input::from_configuration(&bytes).map_err(|e| {
        // The rule alone: the error's own prefix speaks of choosing an instance.
        let why = match e {
            org_host::instance::Error::Invalid(why) => why.to_owned(),
            other => other.to_string(),
        };
        format!("{shown}: {why}; nothing was changed")
    })?;
    configured
        .input
        .fits(org_platform_host::credentials::protected_store().ok())
        .map_err(|why| format!("{shown}: {why}; nothing was changed"))?;
    Ok(Configuration(configured))
}

/// Whether a configuration is the one `record` froze: its input and its supplied pair.
pub(super) fn same_configuration(configured: &Configured, record: &Record) -> bool {
    configured.input == record.input && configured.tls == record.tls
}
