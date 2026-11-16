//! Protected reverse setup connection. Bootstrap/exit exist only here; the
//! ordinary host.sock service cannot dispatch them. Setup owns the listener.
use std::{
    fs, io,
    os::unix::net::UnixStream,
    path::Path,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use org_host_control::{
    journal,
    listen::{ListenConfig, ListenMode},
    ops::{
        Bootstrap, BootstrapOutcome, BootstrapParams, BootstrapRefusal, BootstrapResult, Exit,
        TlsRefusal,
    },
    Hello, HelloResult,
};
use org_proto::{ApiError, Empty, ErrorCode, Request, Response};

use crate::{
    accounts::identity::LoginKey,
    app_auth::AppPolicy,
    home::{HostLock, Layout},
    host_secrets::HostSecrets,
    hostdb::{BootstrapRecord, FirstAccount, HostDb, HostDbInit},
    instance::registry::{Instance, Registry},
    peer::Received,
    service::{
        authentication::hash_password,
        listener::{Connection, Handler, SetupConnection},
    },
    Config, Host, HostError, HOST_SIGNING_ID,
};

#[cfg(all(test, target_os = "macos"))]
mod browser_fixture;
#[cfg(all(test, debug_assertions))]
mod release_gate_tests;

/// A bootstrap-mode host. Drop stops its reverse connection and supervised
/// runtime, retaining all committed registry, custody and organization state.
pub struct BootstrapHost {
    connection: Option<SetupConnection>,
    state: Arc<BootstrapService>,
}
impl BootstrapHost {
    /// The caller verifies its complete local release before invoking this
    /// composition port. Paths are explicit and are checked by the existing
    /// instance registry. No secrets are accepted through argv/environment.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        mut config: Config,
        locator: &Path,
        user_temp: &Path,
        name: Option<&str>,
        root: &Path,
        listen: ListenConfig,
        setup_listener: &Path,
    ) -> Result<Self, HostError> {
        config.admit_runtime()?;
        let state = Self::prepare(config, locator, user_temp, name, root, listen)?;
        // Connect only; never unlink or bind a setup-supplied socket path.
        let stream = UnixStream::connect(setup_listener)
            .map_err(|_| host_error("connecting to setup listener"))?;
        let connection = SetupConnection::start(stream, state.clone())
            .map_err(|_| host_error("starting protected setup connection"))?;
        Ok(Self {
            connection: Some(connection),
            state,
        })
    }
    fn prepare(
        config: Config,
        locator: &Path,
        user_temp: &Path,
        name: Option<&str>,
        root: &Path,
        listen: ListenConfig,
    ) -> Result<Arc<BootstrapService>, HostError> {
        if !cfg!(target_os = "macos") {
            return Err(host_error(
                "protected setup peer admission is unavailable on this platform",
            ));
        }
        let registry = Registry::open(locator, user_temp).map_err(crate::instance_error)?;
        registry
            .restore_temp_folders()
            .map_err(crate::instance_error)?;
        let instance = registry
            .reserve(name, root)
            .map_err(crate::instance_error)?;
        instance
            .set_browser(&listen)
            .map_err(crate::instance_error)?;
        let paths = instance.prepare_layout().map_err(crate::instance_error)?;
        let layout = Layout::with_runtime(&paths.office_home, instance.temp_folder());
        layout.check_socket_paths()?;
        layout.prepare()?;
        let lock = HostLock::acquire(&layout)?;
        let policy = AppPolicy::new(HOST_SIGNING_ID, config.app_team_id.as_deref())
            .map_err(|why| HostError::InvalidAppIdentity { why })?;
        #[cfg(debug_assertions)]
        let policy = match &config.dev_app_path {
            Some(path) => policy
                .with_dev_app(path)
                .map_err(|why| HostError::InvalidAppIdentity { why })?,
            None => policy,
        };
        #[cfg(debug_assertions)]
        let dev = config.dev_key_dir.as_deref();
        #[cfg(not(debug_assertions))]
        let dev = None;
        let custody = crate::host_secrets::store_for(
            dev,
            instance.root(),
            instance.id(),
            crate::code_signing::own_signing,
        )
        .map_err(|_| host_error("protected setup custody unavailable"))?;
        let id = instance.id().to_owned();
        Ok(Arc::new(BootstrapService {
            policy,
            id,
            home: paths.office_home,
            listen,
            custody,
            state: Mutex::new(State {
                pending: Some(Pending {
                    config,
                    instance,
                    lock,
                }),
                host: None,
            }),
        }))
    }
    pub fn live(&self) -> bool {
        self.connection.as_ref().is_some_and(SetupConnection::live)
    }
    pub fn shutdown(mut self) {
        self.stop();
    }
    fn stop(&mut self) {
        self.connection.take();
        if let Ok(mut state) = self.state.state.lock() {
            state.host.take();
            state.pending.take();
        }
    }
}
impl Drop for BootstrapHost {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Pending {
    config: Config,
    instance: Instance,
    lock: HostLock,
}
struct State {
    pending: Option<Pending>,
    host: Option<Host>,
}
struct BootstrapService {
    policy: AppPolicy,
    id: String,
    home: std::path::PathBuf,
    listen: ListenConfig,
    custody: Arc<dyn HostSecrets>,
    state: Mutex<State>,
}
fn host_error(message: &'static str) -> HostError {
    HostError::Io {
        context: "protected bootstrap",
        source: io::Error::other(message),
    }
}
fn unavailable<T>(_: T) -> ApiError {
    ApiError::new(
        ErrorCode::Unavailable,
        "bootstrap is unresolved; resume the original operation",
    )
}
fn invalid() -> ApiError {
    ApiError::new(ErrorCode::InvalidRequest, "invalid bootstrap request")
}
fn refuse(refusal: BootstrapRefusal) -> BootstrapOutcome {
    BootstrapOutcome::Rejected { refusal }
}

impl BootstrapService {
    fn admitted(&self, connection: &Connection, received: Received) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.policy.allows(&crate::app_auth::SocketPeer {
                stream: connection.socket(),
                accepted: connection.accepted(),
                received,
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (connection, received);
            false
        }
    }
    fn database(&self, state: &State) -> Result<Option<Arc<HostDb>>, ApiError> {
        if let Some(host) = &state.host {
            return host
                .service
                .authentication
                .as_ref()
                .map(|auth| Some(auth.provisioning_storage().0))
                .ok_or_else(|| unavailable(()));
        }
        if state.pending.is_none() {
            return Err(unavailable(()));
        }
        match fs::symlink_metadata(self.home.join("host-registry.db")) {
            Ok(_) => {
                let db = HostDb::open(&self.home, &*self.custody).map_err(unavailable)?;
                db.verify_instance(&self.id).map_err(unavailable)?;
                Ok(Some(Arc::new(db)))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(unavailable(e)),
        }
    }
    fn start_runtime<'a>(&self, state: &'a mut State) -> Result<&'a Host, ApiError> {
        if state.host.is_none() {
            let pending = state.pending.take().ok_or_else(|| unavailable(()))?;
            // The same host.lock and instance lock cross creation/start; no
            // unlocked promotion window or second writer is introduced.
            state.host = Some(
                Host::start_prepared(
                    pending.config,
                    Some(pending.instance),
                    Some((pending.lock, Arc::clone(&self.custody))),
                )
                .map_err(unavailable)?,
            );
        }
        state.host.as_ref().ok_or_else(|| unavailable(()))
    }
    fn bootstrap(
        &self,
        p: &BootstrapParams,
        connection: &Connection,
    ) -> Result<BootstrapOutcome, ApiError> {
        let mut state = self.state.lock().map_err(unavailable)?;
        let db = self.database(&state)?;
        // This lookup intentionally precedes credentials, identifiers, TLS and
        // fingerprints. A different operation never probes account existence.
        if let Some(db) = db {
            let record = db.bootstrap_record().map_err(unavailable)?;
            if record.operation_id != p.operation_id {
                return Ok(refuse(BootstrapRefusal::AlreadyBootstrapped));
            }
            if record.finished {
                return Ok(refuse(BootstrapRefusal::BootstrapFinished));
            }
            drop(db);
            let host = self.start_runtime(&mut state)?;
            let auth = host
                .service
                .authentication
                .as_ref()
                .ok_or_else(|| unavailable(()))?;
            let mut signed = Some(auth.verify(connection, &p.login, &p.password)?);
            let checked = (|| {
                if bootstrap_fingerprint(p)? != record.fingerprint {
                    return Err(ApiError::new(
                        ErrorCode::OperationReused,
                        "bootstrap operation reused",
                    ));
                }
                if let Some(pair) = &p.tls {
                    if pair.fingerprint().is_err() {
                        return Ok(refuse(BootstrapRefusal::Tls(TlsRefusal::TlsInvalid)));
                    }
                    // This admitted initial profile has no stored TLS pair.
                    return Ok(refuse(BootstrapRefusal::TlsMismatch));
                }
                Ok(BootstrapOutcome::Completed {
                    result: BootstrapResult {
                        session: signed.take().ok_or_else(|| unavailable(()))?.session,
                    },
                })
            })();
            // verify creates a handle only on the actual connection. Refused
            // replays must not leave an unreturned session usable in memory.
            if let Some(signed) = signed {
                auth.end(connection, &signed.session)?;
            }
            return checked;
        }
        if let Some(refusal) = self.initialize(p)? {
            return Ok(refusal);
        }
        let host = self.start_runtime(&mut state)?;
        let auth = host
            .service
            .authentication
            .as_ref()
            .ok_or_else(|| unavailable(()))?;
        let signed = auth.verify(connection, &p.login, &p.password)?;
        Ok(BootstrapOutcome::Completed {
            result: BootstrapResult {
                session: signed.session,
            },
        })
    }
    /// The caller holds both registered instance and host locks and has checked
    /// that the registry is absent. Refusals never publish a registry or secret.
    fn initialize(&self, p: &BootstrapParams) -> Result<Option<BootstrapOutcome>, ApiError> {
        if org_proto::ulid_time_ms(&p.operation_id).is_none()
            || org_proto::ulid_time_ms(&p.hq_operation_id).is_none()
            || p.login.len() > 4096
            || p.password.expose().len() > 4096
            || p.password.expose().chars().count() < 12
        {
            return Err(invalid());
        }
        LoginKey::new(&p.login).map_err(|_| invalid())?;
        if self.listen.scheme() == ListenMode::Https && p.tls.is_none() {
            return Ok(Some(refuse(BootstrapRefusal::TlsRequired)));
        }
        if let Some(pair) = &p.tls {
            if pair.fingerprint().is_err() {
                return Ok(Some(refuse(BootstrapRefusal::Tls(TlsRefusal::TlsInvalid))));
            }
            // No false certificate match/coverage claim. The selected usable
            // initial profile is HTTP loopback; a verified TLS adapter remains
            // required before accepting any supplied certificate/private key.
            return Err(unavailable(()));
        }
        if self.listen.scheme() != ListenMode::HttpLoopback {
            return Err(unavailable(()));
        }
        let account = crate::ids::generate().map_err(unavailable)?;
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(unavailable)?
            .as_millis();
        let init = HostDbInit {
            instance_id: self.id.clone(),
            clock_high_ms: i64::try_from(wall).map_err(unavailable)?,
            bootstrap: BootstrapRecord {
                operation_id: p.operation_id.clone(),
                hq_operation_id: p.hq_operation_id.clone(),
                fingerprint: bootstrap_fingerprint(p)?,
                account: account.clone(),
                finished: false,
            },
            first_account: FirstAccount::new(&account, &p.login, hash_password(&p.password)?)
                .map_err(unavailable)?,
            tls_pair: None,
        };
        drop(HostDb::create(&self.home, &*self.custody, &init).map_err(unavailable)?);
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
fn bootstrap_fingerprint(p: &BootstrapParams) -> Result<String, ApiError> {
    let login = LoginKey::new(&p.login).map_err(|_| invalid())?;
    Ok(
        journal::bootstrap_fingerprint(login.as_str(), &p.hq_operation_id)
            .map_err(unavailable)?
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}
impl Handler for BootstrapService {
    fn inline(&self, _: &Request, _: &Connection) -> bool {
        true
    }
    fn disconnected(&self, connection: &Arc<Connection>) {
        let service = self
            .state
            .lock()
            .ok()
            .and_then(|state| state.host.as_ref().map(|host| Arc::clone(&host.service)));
        if let Some(service) = service {
            service.disconnected(connection);
        }
    }
    fn handle(&self, request: Request, connection: &Arc<Connection>, received: Received) {
        if request.is::<Hello>() {
            let valid = request
                .params::<Hello>()
                .is_ok_and(|p| p.api_range().contains(org_host_control::API_VERSION));
            if !valid {
                connection.refuse_and_finish(Response::error(
                    request.id,
                    ApiError::new(ErrorCode::UnsupportedVersion, "unsupported setup protocol"),
                ));
                return;
            }
            connection.complete_hello();
            let result = HelloResult::host(env!("CARGO_PKG_VERSION"), crate::offices::THIS_MAC)
                .with_instance_id(&self.id);
            connection.send_response(crate::service::ok::<Hello>(request.id, &result));
            return;
        }
        if !connection.hello_completed() || !self.admitted(connection, received) {
            connection.refuse_and_finish(Response::error(
                request.id,
                ApiError::new(ErrorCode::NotAuthorized, "protected setup client required"),
            ));
            return;
        }
        if request.is::<Bootstrap>() {
            let result = request
                .params::<Bootstrap>()
                .map_err(|_| invalid())
                .and_then(|p| self.bootstrap(&p, connection));
            match result {
                Ok(result) => crate::service::secret_response(connection, request.id, &result),
                Err(error) => {
                    let failed = self
                        .state
                        .lock()
                        .is_ok_and(|state| state.pending.is_none() && state.host.is_none());
                    let response = Response::error(request.id, error);
                    if failed {
                        connection.refuse_and_finish(response);
                    } else {
                        connection.send_response(response);
                    }
                }
            }
        } else if request.is::<Exit>() {
            let result = request
                .params::<Exit>()
                .map_err(|_| invalid())
                .and_then(|_| {
                    let state = self.state.lock().map_err(unavailable)?;
                    if let Some(db) = self.database(&state)? {
                        db.finish_bootstrap().map_err(unavailable)?;
                    }
                    Ok(Empty {})
                });
            match result {
                Ok(result) => {
                    connection.refuse_and_finish(crate::service::ok::<Exit>(request.id, &result))
                }
                Err(error) => connection.send_response(Response::error(request.id, error)),
            }
        } else {
            let service = self
                .state
                .lock()
                .ok()
                .and_then(|state| state.host.as_ref().map(|host| Arc::clone(&host.service)));
            match service {
                Some(service) => service.handle(request, connection, received),
                None => connection.send_response(Response::error(request.id, unavailable(()))),
            }
        }
    }
}
