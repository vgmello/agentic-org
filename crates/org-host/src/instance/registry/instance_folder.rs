//! One registered instance ([`Instance`]): its folder, its control file, its browser listener
//! and its temporary folders.

use std::path::{Path, PathBuf};

use crate::instance::directory::{ControlFile, Directory};
use crate::instance::{temp, Error};

use super::{decode, encode, Config, Instance, InstancePaths, CONFIG, MAX_BYTES, REGISTRY_CREATED};

impl Instance {
    /// Setup holds the instance/root lock. Retry may repeat, never silently
    /// replace, the listener that the protected bootstrap is about to validate.
    pub(crate) fn set_browser(
        &self,
        listen: &org_host_control::listen::ListenConfig,
    ) -> Result<(), Error> {
        let (mut config, previous) = self.config()?;
        if let Some(current) = config.browser {
            if current != *listen {
                return Err(Error::Conflict("listener differs from reserved setup"));
            }
            return Ok(());
        }
        config.browser = Some(listen.clone());
        self.root
            .publish(CONFIG, &encode(&config)?, Some(&previous))?;
        Ok(())
    }

    /// Opt-in HTTPS upgrade (ADR-0033 / TASK-62): switches the instance's browser listener from
    /// plain HTTP (`http_loopback` / `http_published`) to `https` (or updates `hsts` on `https`)
    /// while keeping the exact reserved `(address, port, origin host, origin port)`.
    pub(crate) fn upgrade_browser_to_https(
        &self,
        listen: &org_host_control::listen::ListenConfig,
    ) -> Result<(), Error> {
        use org_host_control::listen::ListenMode;
        if listen.scheme() != ListenMode::Https {
            return Err(Error::Conflict("TLS upgrade requires https scheme"));
        }
        let (mut config, previous) = self.config()?;
        let Some(current) = &config.browser else {
            return Err(Error::Conflict("no browser listener is reserved"));
        };
        if current.address() != listen.address()
            || current.port() != listen.port()
            || current.origin().host() != listen.origin().host()
            || current.origin().port() != listen.origin().port()
        {
            return Err(Error::Conflict(
                "HTTPS upgrade must preserve the reserved listen address, port, and host",
            ));
        }
        config.browser = Some(listen.clone());
        self.root
            .publish(CONFIG, &encode(&config)?, Some(&previous))?;
        Ok(())
    }

    /// Reserved browser listener, if setup configured one.
    pub(crate) fn browser(&self) -> Result<Option<org_host_control::listen::ListenConfig>, Error> {
        self.config().map(|(config, _)| config.browser)
    }
    pub(crate) fn id(&self) -> &str {
        &self.binding.id
    }
    pub(crate) fn name(&self) -> &str {
        self.binding.name.as_str()
    }
    pub(crate) fn root(&self) -> &Path {
        self.root.path()
    }
    /// The instance root itself, as the descriptor this reservation holds its lock on: the
    /// data root a credential directory must stay disjoint from.
    pub(crate) fn root_descriptor(&self) -> std::os::fd::BorrowedFd<'_> {
        self.root.descriptor()
    }

    pub(crate) fn prepare_layout(&self) -> Result<InstancePaths, Error> {
        self.config()?;
        let office_home = self.root.child("office-state", true)?;
        office_home.child(org_office_launch::OFFICES_DIR_NAME, true)?;
        office_home.child("logs", true)?;
        let agents = self.root.child("agents", true)?;
        self.verify()?;
        Ok(InstancePaths {
            office_home: office_home.path().to_owned(),
            agents: agents.path().to_owned(),
        })
    }

    fn config(&self) -> Result<(Config, ControlFile), Error> {
        self.root.verify()?;
        let bytes = self
            .root
            .read(CONFIG, MAX_BYTES)?
            .ok_or(Error::Unsafe("ready instance configuration is missing"))?;
        let config: Config = decode(&bytes)?;
        config.validate(&self.binding)?;
        Ok((config, bytes))
    }

    pub(crate) fn verify(&self) -> Result<(), Error> {
        self.config()?;
        self.instance_temp()?.verify()?;
        Ok(())
    }

    /// Whether the host recorded that it created this instance's host registry. The marker
    /// holds the instance ID; any other content is refused rather than trusted.
    pub(crate) fn registry_created(&self) -> Result<bool, Error> {
        self.root.verify()?;
        match self.root.read(REGISTRY_CREATED, MAX_BYTES)? {
            None => Ok(false),
            Some(bytes) if *bytes == *self.binding.id.as_bytes() => Ok(true),
            Some(_) => Err(Error::Unsafe("host registry marker names another instance")),
        }
    }

    /// Records, durably and once, that the host registry exists. The caller holds this
    /// instance's root lock and `host.lock`, and calls it only after the registry is
    /// published. A later start that finds the marker without the registry refuses.
    pub(crate) fn record_registry_created(&self) -> Result<(), Error> {
        if self.registry_created()? {
            return Ok(());
        }
        self.root
            .publish(REGISTRY_CREATED, self.binding.id.as_bytes(), None)?;
        Ok(())
    }

    pub(crate) fn temp_folder(&self) -> &Path {
        &self.binding.temp_folder
    }

    fn instance_temp(&self) -> Result<Directory, Error> {
        self.user_temp.verify()?;
        self.user_temp
            .child("ao", false)?
            .child(&self.binding.temp_handle, false)
    }

    /// Allocates once under this instance; callers backfill every catalog office before launch.
    pub(crate) fn allocate_office_temp(&mut self, office: &str) -> Result<PathBuf, Error> {
        if !org_office_launch::is_storage_id(office) {
            return Err(Error::Invalid("office ID must be plain"));
        }
        let (mut config, previous) = self.config()?;
        let parent = self.instance_temp()?;
        if let Some(handle) = config.office_temp_handles.get(office) {
            let directory = parent.child(handle, true)?;
            return Ok(directory.path().to_owned());
        }
        let (handle, directory) = temp::allocate(
            &parent,
            config.office_temp_handles.values().map(String::as_str),
        )?;
        config.office_temp_handles.insert(office.to_owned(), handle);
        self.root
            .publish(CONFIG, &encode(&config)?, Some(&previous))?;
        Ok(directory.path().to_owned())
    }
}
