//! The running host ([`Host`]): composed by its start, stopped by its shutdown.

use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::path::Path;
use std::sync::Arc;

use org_platform_host::credentials::ReleaseRoot;

use crate::broadcast::Broadcast;
use crate::config::{harden, Config};
use crate::error::HostError;
use crate::home::{HostLock, Layout};
use crate::offices::Offices;
use crate::service::listener::{self, BindError, Listener};
use crate::service::Service;
use crate::startup::{
    instance_error, maybe_init_local_authless, startup_app_policy, startup_authentication,
    startup_catalog, startup_keys, startup_listener, startup_protected_services,
    startup_server_check, startup_web, startup_web_tls, ProtectedServices,
};
use crate::supervisor::{Supervisor, SupervisorConfig};
use crate::{
    host_log, host_secrets, instance, provisioning, recovery, release, service, sync, web, web_tls,
    THREAD_END_MARGIN,
};

#[cfg(test)]
use crate::writer;

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

/// A running host. [`Host::shutdown`] (or dropping it) stops it.
pub struct Host {
    pub(crate) _runtime_release: Option<release::RuntimeRelease>,
    pub(crate) layout: Layout,
    pub(crate) offices: Arc<Offices>,
    pub(crate) listener: Option<Listener>,
    pub(crate) lock: Option<HostLock>,
    pub(crate) instance: Option<Arc<instance::registry::Instance>>,
    pub(crate) authentication_lifecycle: Option<service::authentication::Lifecycle>,
    pub(crate) web: Option<web::Supervisor>,
    /// The renewal of an `https` listener's own certificate while the host runs.
    pub(crate) tls_renewal: Option<web_tls::Renewal>,
    pub(crate) service: Arc<Service>,
    /// Organization creation, kept to end its threads when the host stops.
    pub(crate) creating: Option<Arc<provisioning::Provisioning>>,
    #[cfg(test)]
    pub(crate) writers: Option<Arc<writer::Writers>>,
    /// How long the servers get to exit when the host stops.
    pub(crate) shutdown_grace: std::time::Duration,
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

    pub(super) fn start_prepared(
        config: Config,
        instance: Option<instance::registry::Instance>,
        prepared: Option<(HostLock, Arc<dyn host_secrets::HostSecrets>)>,
    ) -> Result<Self, HostError> {
        Self::start_with(config, instance, prepared, None)
    }

    /// The composition. `release_directory` is the credential directory a registered start
    /// opened, where this system keeps a host's secrets in one and the setup record names it
    /// (in every build): the office keys are opened below it, and each view keeps it locked
    /// for as long as the view lives.
    pub(super) fn start_with(
        mut config: Config,
        instance: Option<instance::registry::Instance>,
        prepared: Option<(HostLock, Arc<dyn host_secrets::HostSecrets>)>,
        release_directory: Option<ReleaseRoot>,
    ) -> Result<Self, HostError> {
        harden()?;
        if instance.is_some() {
            config.admit_runtime_with(release_directory.is_some())?;
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
        // With an instance, either the key store is scoped by ID or the release directory
        // was opened with a verified instance binding. An unregistered host cannot judge
        // which keys are orphans.
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
        // Before any office or web process starts: an `https` instance whose pair cannot be
        // served and may not be made again does not start.
        let web_tls = startup_web_tls(&instance, &authentication, config.supplied_tls.take())?;
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
            federation: config.federation.clone(),
        };
        let supervisor = match &instance {
            Some(instance) => Supervisor::new(supervisor_config, instance.id()),
            None => Supervisor::unregistered(supervisor_config),
        }
        .with_protected_offices(protected_offices);
        let ProtectedServices {
            writers,
            supervisor,
            provisioning,
        } = startup_protected_services(&instance, &authentication, supervisor, &keys, &layout)?;
        #[cfg(not(test))]
        let _ = writers;
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
            org_host_control::WebState::NotConfigured,
        ));
        let web_config = startup_web(&config, &instance, &layout, dev_key_dir, web_tls.clone())?;
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
        let browser_origin = instance
            .as_ref()
            .and_then(|i| i.browser().ok().flatten())
            .map(|l| l.origin().as_str().to_owned());
        maybe_init_local_authless(instance.as_ref(), authentication.as_ref());
        let instance = instance.map(Arc::new);
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
            .with_web_tls(web_tls.clone())
            .with_instance(instance.clone())
            .with_browser_origin(browser_origin)
            .with_web_state(Arc::clone(&gateway_state)),
        );
        let tls_renewal = match web_tls {
            Some(tls) => {
                let announcing = Arc::downgrade(&service);
                Some(
                    web_tls::Renewal::start(
                        tls,
                        web_tls::RENEW_EVERY,
                        Box::new(move || {
                            if let Some(service) = announcing.upgrade() {
                                service.announce_tls_renewal();
                            }
                        }),
                    )
                    .map_err(io("starting the TLS certificate's renewal"))?,
                )
            }
            None => None,
        };
        let listener = startup_listener(bound, &service, &offices, config.max_connections)?;
        // A create that reserved its identity before the host last stopped continues now,
        // with no client asking; later ones start from the browser's request. This follows
        // the last step of the start that can fail: a start that is refused has started no
        // creation thread, so none outlives the refusal. Until this line a create or a
        // lookup that arrives on the new listener is answered `unavailable` and asked again.
        if let Some(provisioning) = &creating {
            provisioning.start(Arc::clone(&offices) as Arc<dyn provisioning::Hq>);
        }
        let held = config
            .browser_listener
            .take()
            .and_then(|listener| listener.take());
        let web = web_config.and_then(|mut web_config| {
            let retrying = Arc::downgrade(&service);
            web_config.on_renewal_kept = Some(Arc::new(move |_| {
                if let Some(service) = retrying.upgrade() {
                    service.schedule_tls_renewal_retry();
                }
            }));
            match web::Supervisor::start(
                web_config,
                held,
                service.clone(),
                Arc::clone(&gateway_state),
                service.web_eligibility(),
            ) {
                Ok(web) => Some(web),
                Err(error) => {
                    *sync::lock(&gateway_state) = org_host_control::WebState::Failed;
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
            tls_renewal,
            service,
            creating,
            #[cfg(test)]
            writers,
            shutdown_grace: config.timings.shutdown_grace,
        })
    }

    #[cfg(test)]
    pub(crate) fn close_writer_channel_for_test(&self, office: &str) -> bool {
        self.writers
            .as_ref()
            .is_some_and(|w| w.close_channel_for_test(office))
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
    ///
    /// The stop is bounded from the moment it is asked for: every wait in it ends by one
    /// deadline fixed then, the servers' grace period, or a margin after it
    /// ([`THREAD_END_MARGIN`], the supervisor's for its short runs, provisioning's for its
    /// creation threads), whatever a server, the web process or a request does.
    pub fn shutdown(self) {
        self.shutdown_from(std::time::Instant::now());
    }

    /// [`Host::shutdown`] of a stop asked for at `asked`, which may be before this host
    /// finished starting (a signal taken while it started): the stop's deadline is fixed from
    /// then, so it ends as it would have had the host been running when it was asked for.
    /// Every wait of a stop asked for longer ago than [`Timings::stop_ends_within`](crate::Timings::stop_ends_within) ends at
    /// once: its servers are killed without a grace.
    pub fn shutdown_from(mut self, asked: std::time::Instant) {
        self.stop(asked);
    }

    fn stop(&mut self, asked: std::time::Instant) {
        let Some(mut listener) = self.listener.take() else {
            return;
        };
        let deadline = asked + self.shutdown_grace;
        let until = deadline + THREAD_END_MARGIN;
        listener.close();
        // Its start or its run ends at once: the control connection (and its sessions)
        // closes, and the web process is killed and reaped.
        if let Some(web) = &self.web {
            web.stop();
        }
        // No creation attempt starts from here on, and none records a link.
        if let Some(creating) = &self.creating {
            creating.halt();
        }
        // Before joining the listener and the web process's connection: a create still
        // running answers as soon as the supervisor stops its server.
        self.offices.shutdown_by(deadline, until);
        if let Some(web) = self.web.take() {
            if !web.join_by(until) {
                host_log::log("the browser web process's connection did not end with the host");
            }
        }
        if !listener.join_by(until) {
            host_log::log("a request on host.sock did not end with the host");
        }
        // Nothing of a create writes custody or the registry after this, unless it is
        // logged that a creation did not end.
        if let Some(creating) = self.creating.take() {
            creating.stop_by(deadline);
        }
        if let Some(lifecycle) = self.authentication_lifecycle.take() {
            if !lifecycle.join_by(until) {
                host_log::log("the session expiry did not end with the host");
            }
        }
        if let Some(renewal) = self.tls_renewal.take() {
            if !renewal.join_by(until) {
                host_log::log("the TLS certificate's renewal did not end with the host");
            }
        }
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
        self.stop(std::time::Instant::now());
    }
}
