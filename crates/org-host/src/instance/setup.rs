//! Bounded nonsecret setup continuation. No password/session/custody bytes, no account and
//! no organization: setup asks for none (A2). The frozen input is the root and the address;
//! progress only moves forward, in the order the setup flow takes it:
//! instance reserved (`instance_id`), service registered, registry created by the service
//! host at its first start (D1), initial registration link issued (at most once).
use super::{
    directory::{Directory, FileLock},
    Error, Name,
};
use crate::release::ReleasePin;
use org_host_control::listen::ListenConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
const FILE: &str = "setup.v1.json";
const LIMIT: usize = 64 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    pub version: u32,
    pub root: PathBuf,
    #[serde(deserialize_with = "strict_listen")]
    pub listen: ListenConfig,
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
    pub fn validate(&self) -> Result<(), Error> {
        if self.version != 1 || super::directory::absolute(&self.root)? != self.root {
            return Err(Error::Invalid("invalid version-1 setup configuration"));
        }
        // The existing actual gateway composition only supports this profile.
        if self.listen.scheme() != org_host_control::listen::ListenMode::HttpLoopback
            || self
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
                "this setup adapter requires valid loopback HTTP; HTTPS setup is unavailable",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub version: u32,
    pub input: Input,
    pub offline: bool,
    pub release: ReleasePin,
    /// UUIDv7, fixed when the record is first frozen.
    pub setup_operation: String,
    /// The reserved instance, as the registry names it.
    pub instance_id: Option<String>,
    pub service_registered: bool,
    /// The service host created its registry at its first start (D1).
    pub registry_created: bool,
    /// The one initial registration link was issued for `setup_operation`.
    pub initial_link_issued: bool,
}
impl Record {
    fn validate(&self) -> Result<(), Error> {
        self.input.validate()?;
        self.release
            .validate()
            .map_err(|_| Error::Unsafe("invalid setup release pin"))?;
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
            && old.offline == self.offline
            && old.release == self.release
            && old.setup_operation == self.setup_operation
            && old
                .instance_id
                .as_ref()
                .is_none_or(|i| Some(i) == self.instance_id.as_ref())
            && (!old.service_registered || self.service_registered)
            && (!old.registry_created || self.registry_created)
            && (!old.initial_link_issued || self.initial_link_issued)
    }
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
        input.validate()?;
        if let Some(r) = self.read()? {
            if r.input != input || r.offline != offline || r.release != release {
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
            service_registered: false,
            registry_created: false,
            initial_link_issued: false,
        };
        self.write(&r)?;
        Ok(r)
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
