//! Bounded nonsecret setup continuation. No password/session/custody bytes, no account and
//! no organization: setup asks for none (A2). The frozen input is the service mode, the
//! root, the address and, where a file gave it, the credential directory and the files of a
//! supplied TLS pair (their paths, never the key); the same input, as a strict JSON file, is the configuration
//! of non-interactive setup (`setup --non-interactive --config`, `serve --setup`);
//! progress only moves forward, in the order the setup flow takes it:
//! instance reserved (`instance_id`), service registered, registry created by the service
//! host at its first start (D1), initial registration link issued (at most once).
//!
//! `credentials` is where a release host keeps its secrets on a system whose protected
//! store is a directory (Linux host design section 3.2): an absolute path, recorded once
//! and never changed, which the host reads at every start instead of deriving one from its
//! environment. A system that keeps them in the Keychain records none
//! ([`credentials_rule`]).
use super::{
    directory::{Directory, FileLock},
    Error, Name,
};
use crate::release::ReleasePin;
use org_host_control::listen::ListenConfig;
use org_platform_host::credentials::ProtectedStore;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
const FILE: &str = "setup.v1.json";
const LIMIT: usize = 64 * 1024;
/// Who starts and restarts the instance (Linux host design section 3.4). It is data in the
/// configuration: nothing detects a container.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Service {
    /// The account's service manager (launchd, a systemd user unit).
    Registered,
    /// Nothing is registered: whoever runs `agentic-org serve` is the supervisor (a
    /// container runtime). The credential directory is then a mounted volume.
    Foreground,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    pub version: u32,
    pub service: Service,
    pub root: PathBuf,
    /// The credential directory, where the configuration names it: required in the
    /// foreground mode on a system that keeps a host's secrets in a directory, optional in
    /// the registered mode (setup then makes it in the account's state folder), and
    /// refused on a system that keeps them elsewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<PathBuf>,
    #[serde(deserialize_with = "strict_listen")]
    pub listen: ListenConfig,
}

/// The two files of a TLS pair the operator supplies for an `https` listener (`"tls":
/// {"cert", "key"}` in the configuration): absolute, normalized paths of a PEM certificate
/// chain, leaf first, and its unencrypted key. Setup reads and verifies the pair before it
/// saves anything, and the host imports it at its first start (`crate::web_tls`); only the
/// paths are recorded. Without one, an `https` instance makes its own certificate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SuppliedTls {
    pub cert: PathBuf,
    pub key: PathBuf,
}

impl SuppliedTls {
    /// A pair is named only for an `https` listener, by absolute, normalized paths.
    fn validate(&self, input: &Input) -> Result<(), Error> {
        if input.listen.scheme() != org_host_control::listen::ListenMode::Https {
            return Err(Error::Invalid(TLS_NEEDS_HTTPS));
        }
        if !spelled(&self.cert) || !spelled(&self.key) {
            return Err(Error::Invalid(
                "the TLS pair's files (\"tls\": {\"cert\", \"key\"}) must be absolute, \
                 normalized paths",
            ));
        }
        Ok(())
    }
}

/// Spelled exactly as it is walked: the bytes are compared, since two spellings of one path
/// are equal as paths.
fn spelled(path: &Path) -> bool {
    super::directory::absolute(path)
        .ok()
        .is_some_and(|normal| normal.as_os_str() == path.as_os_str())
}

/// The configuration of non-interactive setup, as read: the input and, for `https`, a
/// supplied pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Configured {
    pub input: Input,
    pub tls: Option<SuppliedTls>,
}

fn strict_listen<'de, D: serde::Deserializer<'de>>(d: D) -> Result<ListenConfig, D::Error> {
    #[derive(Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    struct Raw {
        address: std::net::IpAddr,
        port: u16,
        scheme: org_host_control::listen::ListenMode,
        origin: org_host_control::listen::ListenOrigin,
        hsts: org_host_control::listen::Hsts,
    }
    let raw = Raw::deserialize(d)?;
    // No floating-point or arbitrary extension conversion: every field above
    // has the exact existing listener wire type and its required occurrence.
    let value = serde_json::to_value(raw).map_err(serde::de::Error::custom)?;
    serde_json::from_value(value).map_err(serde::de::Error::custom)
}
impl Input {
    /// What every input must satisfy, whoever wrote it. The rules that depend on the
    /// system (whether it keeps secrets in a directory) are [`Input::fits`].
    pub fn validate(&self) -> Result<(), Error> {
        if self.version != 1 {
            return Err(Error::Invalid(
                "the setup configuration's \"version\" must be 1",
            ));
        }
        if !spelled(&self.root) {
            return Err(Error::Invalid(
                "the data folder (\"root\") must be an absolute, normalized path",
            ));
        }
        if self
            .credentials
            .as_deref()
            .is_some_and(|path| !spelled(path))
        {
            return Err(Error::Invalid(
                "the credential directory must be an absolute, normalized path",
            ));
        }
        // Loopback HTTP; HTTPS on any address; or, chosen by its own name, plain HTTP on a
        // published port, which only a foreground instance may have. Plain HTTP with a
        // non-loopback address is never implied (`ListenOrigin::check`).
        if self
            .listen
            .origin()
            .check(
                self.listen.scheme(),
                self.listen.address(),
                self.listen.port(),
            )
            .is_err()
        {
            return Err(Error::Invalid(
                "the listener must be loopback HTTP on its own address (http_loopback), plain \
                 HTTP on a published port with a loopback origin (http_published), or HTTPS, \
                 with an origin that names its scheme and port",
            ));
        }
        if self
            .listen
            .scheme()
            .fits_service(self.service == Service::Foreground)
            .is_err()
        {
            return Err(Error::Invalid(PUBLISHED_NEEDS_FOREGROUND));
        }
        Ok(())
    }

    /// Whether this input fits a system whose protected store for a release build is
    /// `store` (`None`: it has none), pure. A directory is named only where the system
    /// keeps secrets in one, and the foreground mode needs it named there: a container has
    /// no account state folder to derive one from.
    pub fn fits(&self, store: Option<ProtectedStore>) -> Result<(), &'static str> {
        match (store, self.service, self.credentials.is_some()) {
            (Some(ProtectedStore::ReleaseDirectory), Service::Foreground, false) => Err(
                "the foreground mode needs \"credentials\": the absolute path of the mounted \
                 secrets volume",
            ),
            (Some(ProtectedStore::ReleaseDirectory), _, _) => Ok(()),
            (_, _, true) => Err(
                "\"credentials\" names a directory, and this system does not keep a host's \
                 secrets in one; remove it",
            ),
            (_, _, false) => Ok(()),
        }
    }

    /// The configuration of non-interactive setup: this input, and for `https` optionally
    /// `tls` ([`SuppliedTls`]), as one strict JSON object, at most [`LIMIT`] bytes. Unknown
    /// and repeated properties are refused, and so is any version but 1.
    pub fn from_configuration(bytes: &[u8]) -> Result<Configured, Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct File {
            version: u32,
            service: Service,
            root: PathBuf,
            #[serde(default)]
            credentials: Option<PathBuf>,
            #[serde(deserialize_with = "strict_listen")]
            listen: ListenConfig,
            #[serde(default)]
            tls: Option<SuppliedTls>,
        }
        if bytes.len() > LIMIT {
            return Err(Error::Invalid("the setup configuration is too large"));
        }
        let file: File =
            serde_json::from_slice(bytes).map_err(|_| Error::Invalid(CONFIGURATION_SHAPE))?;
        let input = Input {
            version: file.version,
            service: file.service,
            root: file.root,
            credentials: file.credentials,
            listen: file.listen,
        };
        input.validate()?;
        if let Some(tls) = &file.tls {
            tls.validate(&input)?;
        }
        Ok(Configured {
            input,
            tls: file.tls,
        })
    }
}

/// What an input, a configuration or a record is told when its listener is plain HTTP on a
/// published port and its service mode is not `foreground` (`ListenMode::fits_service`).
/// Refused when the input is read, before anything is made, and again whenever the record
/// is read, so a record edited by hand does not start a host.
pub(crate) const PUBLISHED_NEEDS_FOREGROUND: &str = "\"scheme\": \"http_published\" (plain HTTP \
     on a published port) is only for \"service\": \"foreground\", the main process of a \
     container; a registered service listens with http_loopback";

/// What a configuration that names a TLS pair for a plain HTTP listener is told.
pub(crate) const TLS_NEEDS_HTTPS: &str = "\"tls\" names a certificate pair, which only \
     \"scheme\": \"https\" serves; remove it, or listen with https";

/// What a configuration that does not parse is told: the whole shape, since a strict
/// reader cannot say which property a person meant.
pub(crate) const CONFIGURATION_SHAPE: &str = "the setup configuration is not valid: it is one \
     JSON object with \"version\": 1, \"service\": \"registered\" or \"foreground\", \"root\": an \
     absolute path, \"listen\": {\"address\", \"port\", \"scheme\": \"https\", \"http_loopback\" or \
     \"http_published\", \"origin\", \"hsts\": \"off\", \"on\" or \"clear\"}, and optionally \
     \"credentials\": an absolute path and, with https, \"tls\": {\"cert\": a path, \"key\": a path} \
     (a PEM certificate chain and its key); no other property is accepted";
// Before setup recorded a service mode, every instance used the service manager.
#[derive(Deserialize)]
#[serde(remote = "Input", deny_unknown_fields)]
struct RecordedInput {
    version: u32,
    #[serde(default = "registered_service")]
    service: Service,
    root: PathBuf,
    #[serde(default)]
    credentials: Option<PathBuf>,
    #[serde(deserialize_with = "strict_listen")]
    listen: ListenConfig,
}

fn registered_service() -> Service {
    Service::Registered
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub version: u32,
    #[serde(deserialize_with = "RecordedInput::deserialize")]
    pub input: Input,
    pub offline: bool,
    pub release: ReleasePin,
    /// UUIDv7, fixed when the record is first frozen.
    pub setup_operation: String,
    /// The reserved instance, as the registry names it.
    pub instance_id: Option<String>,
    /// The credential directory of a release host, where this system keeps a release
    /// build's secrets in one: absolute and normalized, fixed once it is recorded.
    pub credentials: Option<PathBuf>,
    /// The supplied TLS pair's files, frozen with the input; `None` for an instance that
    /// makes its own certificate, or that does not serve `https`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<SuppliedTls>,
    pub service_registered: bool,
    /// The service host created its registry at its first start (D1).
    pub registry_created: bool,
    /// The one initial registration link was issued for `setup_operation`.
    pub initial_link_issued: bool,
}
impl Record {
    fn validate(&self) -> Result<(), Error> {
        self.input.validate()?;
        if let Some(tls) = &self.tls {
            tls.validate(&self.input)?;
        }
        self.release
            .validate()
            .map_err(|_| Error::Unsafe("invalid setup release pin"))?;
        if let Some(path) = &self.credentials {
            // Absolute, UTF-8, and spelled exactly as it is walked: no `.`, `..`, repeated or
            // trailing separator. The bytes are compared, since two spellings of one path
            // are equal as paths.
            if super::directory::absolute(path)
                .ok()
                .is_none_or(|normal| normal.as_os_str() != path.as_os_str())
            {
                return Err(Error::Invalid(
                    "the credential directory must be an absolute, normalized path",
                ));
            }
        }
        // A directory the configuration names is the one that is recorded.
        if let (Some(configured), Some(recorded)) = (&self.input.credentials, &self.credentials) {
            if configured != recorded {
                return Err(Error::Unsafe(
                    "the recorded credential directory is not the configured one",
                ));
            }
        }
        credentials_rule(
            org_platform_host::credentials::protected_store().ok(),
            !cfg!(debug_assertions),
            self.instance_id.is_some(),
            self.credentials.is_some(),
        )
        .map_err(Error::Unsafe)?;
        if self.version != 1
            || !org_proto::is_uuid_v7(&self.setup_operation)
            || self
                .instance_id
                .as_ref()
                .is_some_and(|i| !super::registry::valid_instance_id(i))
            || (self.service_registered && self.instance_id.is_none())
            || (self.registry_created && !self.service_registered)
            || (self.initial_link_issued && !self.registry_created)
        {
            return Err(Error::Unsafe("incoherent setup continuation"));
        }
        Ok(())
    }
    /// Identity and configuration never change, and progress never goes backwards.
    fn follows(&self, old: &Record) -> bool {
        old.input == self.input
            && old.tls == self.tls
            && old.offline == self.offline
            && old.release == self.release
            && old.setup_operation == self.setup_operation
            && old
                .instance_id
                .as_ref()
                .is_none_or(|i| Some(i) == self.instance_id.as_ref())
            && old
                .credentials
                .as_ref()
                .is_none_or(|c| Some(c) == self.credentials.as_ref())
            && (!old.service_registered || self.service_registered)
            && (!old.registry_created || self.registry_created)
            && (!old.initial_link_issued || self.initial_link_issued)
    }
}
/// Whether a setup record may name a credential directory, and whether it must, pure.
///
/// `store` is the kind of protected store a release build has on this system (`None`: it
/// has none), `release_build` whether this build is one, `reserved` whether the record
/// names its instance, and `recorded` whether it names a credential directory.
///
/// - The Keychain, or no protected store: a directory is never recorded. Nothing falls
///   back to one.
/// - A release directory: a release build's record names it from the moment the instance
///   is reserved (both are written together), so a host never starts without knowing where
///   its secrets are. A development build keeps its development stores and may record none.
pub(crate) fn credentials_rule(
    store: Option<ProtectedStore>,
    release_build: bool,
    reserved: bool,
    recorded: bool,
) -> Result<(), &'static str> {
    match store {
        Some(ProtectedStore::ReleaseDirectory) => {
            if release_build && reserved && !recorded {
                return Err("setup recorded no credential directory for this instance");
            }
            Ok(())
        }
        Some(ProtectedStore::Keychain) | None => {
            if recorded {
                return Err("a credential directory is recorded on a system that keeps none");
            }
            Ok(())
        }
    }
}

/// `path` in the one spelling a record accepts: absolute, UTF-8, without `.`, `..`, a
/// repeated or a trailing separator. `.` and repeated separators are dropped; `..` is
/// refused, never resolved.
pub(crate) fn normalized(path: &Path) -> Result<PathBuf, Error> {
    super::directory::absolute(path)
}

pub(crate) struct Journal {
    directory: Directory,
    lock: FileLock,
}
pub(crate) struct Paths {
    pub base: PathBuf,
    pub locator: PathBuf,
    pub user_temp: PathBuf,
}
impl Paths {
    /// Setup's default root for `name`: `<base>/instances/<name>`, its parent created private.
    pub fn instance_root(&self, name: Option<&str>) -> Result<PathBuf, Error> {
        let name = Name::parse(name)?;
        let instances = Directory::open(&self.base, false)?.child("instances", true)?;
        Ok(instances.path().join(name.as_str()))
    }
}

impl Paths {
    pub fn prepare(account: &Path, user_temp: PathBuf, name: Option<&str>) -> Result<Self, Error> {
        let base = org_platform::dirs::application_base(account)
            .map_err(|_| Error::Invalid("instance locations are unavailable on this system"))?;
        Self::at(&base, user_temp, name)
    }

    /// The paths under an explicit application base (the platform's, or a development
    /// override).
    pub fn at(base: &Path, user_temp: PathBuf, name: Option<&str>) -> Result<Self, Error> {
        Name::parse(name)?;
        let base = Directory::open(base, true)?;
        let locator = base.child("locator", true)?.path().to_owned();
        super::temp::validate_base(&Directory::open(&user_temp, false)?)?;
        Ok(Self {
            base: base.path().to_owned(),
            locator,
            user_temp,
        })
    }
}
impl Journal {
    /// Atomic read of frozen setup facts for the service. This reader never
    /// takes the workflow writer lock or publishes progress; progress may
    /// advance concurrently, while identity/configuration are immutable.
    pub fn snapshot(base: &Path, name: Option<&str>) -> Result<Record, Error> {
        let name = Name::parse(name)?;
        let directory = Directory::open(base, false)?
            .child("setup", false)?
            .child(name.as_str(), false)?;
        let bytes = directory
            .read(FILE, LIMIT)?
            .ok_or(Error::Unsafe("setup continuation is missing"))?;
        let record: Record = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Unsafe("invalid setup continuation"))?;
        record.validate()?;
        Ok(record)
    }
    pub fn open(base: &Path, name: Option<&str>) -> Result<Self, Error> {
        let name = Name::parse(name)?;
        let directory = Directory::open(base, false)?
            .child("setup", true)?
            .child(name.as_str(), true)?;
        let lock = directory.lock_file("setup.lock")?;
        Ok(Self { directory, lock })
    }
    fn verify(&self) -> Result<(), Error> {
        self.directory.verify_file("setup.lock", &self.lock)
    }
    pub fn read(&self) -> Result<Option<Record>, Error> {
        self.verify()?;
        let value = self
            .directory
            .read(FILE, LIMIT)?
            .map(|bytes| {
                serde_json::from_slice::<Record>(&bytes)
                    .map_err(|_| Error::Unsafe("invalid setup continuation"))
            })
            .transpose()?;
        if let Some(r) = &value {
            r.validate()?;
        }
        Ok(value)
    }
    pub fn freeze(
        &self,
        input: Input,
        offline: bool,
        release: ReleasePin,
    ) -> Result<Record, Error> {
        self.freeze_configured(Configured { input, tls: None }, offline, release)
    }
    /// [`Journal::freeze`] with a supplied TLS pair's files.
    pub fn freeze_configured(
        &self,
        configured: Configured,
        offline: bool,
        release: ReleasePin,
    ) -> Result<Record, Error> {
        let Configured { input, tls } = configured;
        input.validate()?;
        if let Some(tls) = &tls {
            tls.validate(&input)?;
        }
        if let Some(r) = self.read()? {
            if r.input != input || r.tls != tls || r.offline != offline || r.release != release {
                return Err(Error::Conflict("setup input or release differs; use the original request; upgrades are explicit"));
            }
            return Ok(r);
        }
        let r = Record {
            version: 1,
            input,
            offline,
            release,
            setup_operation: crate::ids::uuid_v7()?,
            instance_id: None,
            credentials: None,
            tls,
            service_registered: false,
            registry_created: false,
            initial_link_issued: false,
        };
        self.write(&r)?;
        Ok(r)
    }
    /// Takes away a record that pins nothing yet: one without a reserved instance, which a
    /// setup run froze and then could not continue. A record that names its instance is
    /// never removed: its configuration is the instance's. Returns whether one was removed.
    pub fn abandon(&self) -> Result<bool, Error> {
        match self.read()? {
            None => Ok(false),
            Some(record) if record.instance_id.is_some() => Err(Error::Conflict(
                "setup reserved an instance for this record; it is not removed",
            )),
            Some(_) => {
                std::fs::remove_file(self.directory.path().join(FILE))?;
                self.verify()?;
                Ok(true)
            }
        }
    }
    pub fn write(&self, next: &Record) -> Result<(), Error> {
        self.verify()?;
        next.validate()?;
        let previous = self.directory.read(FILE, LIMIT)?;
        if let Some(previous) = &previous {
            let old: Record = serde_json::from_slice(previous)
                .map_err(|_| Error::Unsafe("invalid setup continuation"))?;
            if !next.follows(&old) {
                return Err(Error::Conflict(
                    "setup continuation cannot replace accepted identity or go backwards",
                ));
            }
        }
        let bytes = serde_json::to_vec(next)
            .map_err(|_| Error::Unsafe("setup continuation cannot encode"))?;
        if bytes.len() > LIMIT {
            return Err(Error::Invalid("setup continuation exceeds its profile"));
        }
        self.directory.publish(FILE, &bytes, previous.as_ref())?;
        self.verify()
    }
}

#[cfg(test)]
mod tests;
