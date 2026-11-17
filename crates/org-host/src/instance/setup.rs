//! Bounded nonsecret setup continuation. No password/session/custody bytes.
use super::{
    directory::{Directory, FileLock},
    Error, Name,
};
use crate::release::ReleasePin;
use org_host_control::{listen::ListenConfig, provision::Provisioned};
use org_proto::selected_bootstrap::PlanV1;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
const FILE: &str = "setup.v1.json";
const LIMIT: usize = 64 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    pub version: u32,
    pub root: PathBuf,
    pub login: String,
    pub organization: PlanV1,
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
        if self.version != 1
            || super::directory::absolute(&self.root)? != self.root
            || !self.organization.valid()
            || crate::accounts::identity::LoginKey::new(&self.login).is_err()
        {
            return Err(Error::Invalid("invalid version-1 HQ configuration"));
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
    pub bootstrap_operation: String,
    pub hq_operation: String,
    pub instance_id: Option<String>,
    pub provisioned: Option<Provisioned>,
    pub exited: bool,
    pub registered: bool,
}
impl Record {
    fn validate(&self) -> Result<(), Error> {
        self.input.validate()?;
        self.release
            .validate()
            .map_err(|_| Error::Unsafe("invalid setup release pin"))?;
        if self.version != 1
            || org_proto::ulid_time_ms(&self.bootstrap_operation).is_none()
            || org_proto::ulid_time_ms(&self.hq_operation).is_none()
            || self.bootstrap_operation == self.hq_operation
            || self
                .instance_id
                .as_ref()
                .is_some_and(|i| org_proto::ulid_time_ms(i).is_none())
            || self.provisioned.as_ref().is_some_and(|p| {
                [&p.organization_id, &p.office_id, &p.home_team_id]
                    .iter()
                    .any(|id| !org_proto::is_uuid_v7(id))
            })
            || (self.provisioned.is_some() && self.instance_id.is_none())
            || (self.exited && self.provisioned.is_none())
            || (self.registered && !self.exited)
        {
            return Err(Error::Unsafe("incoherent setup continuation"));
        }
        Ok(())
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
    pub fn prepare(account: &Path, user_temp: PathBuf, name: Option<&str>) -> Result<Self, Error> {
        Name::parse(name)?;
        let base = Directory::open(
            &account.join("Library/Application Support/agentic.org"),
            true,
        )?;
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
    /// takes the workflow writer lock or publishes progress; registered may
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
            bootstrap_operation: crate::ids::generate()?,
            hq_operation: crate::ids::generate()?,
            instance_id: None,
            provisioned: None,
            exited: false,
            registered: false,
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
            if old.input != next.input
                || old.offline != next.offline
                || old.release != next.release
                || old.bootstrap_operation != next.bootstrap_operation
                || old.hq_operation != next.hq_operation
                || old
                    .instance_id
                    .as_ref()
                    .is_some_and(|i| Some(i) != next.instance_id.as_ref())
                || old
                    .provisioned
                    .as_ref()
                    .is_some_and(|p| Some(p) != next.provisioned.as_ref())
                || (old.exited && !next.exited)
                || (old.registered && !next.registered)
            {
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
