//! The steps of a host start: the server check, `host.sock`, the key store, the app policy,
//! authentication, the catalog, the web listener's TLS pair and the web process.

use std::path::Path;
use std::sync::Arc;
use std::{fs, io};

use org_platform_host::credentials::ReleaseRoot;

use crate::app_auth::{AppPolicy, APP_BUNDLE_ID};
use crate::catalog::Catalog;
use crate::config::Config;
use crate::control_listener::{Listener, ListenerConfig};
use crate::error::HostError;
use crate::home::Layout;
use crate::offices::Offices;
use crate::server_check::ServerCheck;
use crate::service::Service;
use crate::supervisor::Supervisor;
use crate::{catalog, code_signing, host_log, host_secrets, instance, keys, provisioning};
use crate::{web, web_tls, writer};
use crate::{HOST_SIGNING_ID, MAX_IN_FLIGHT_PER_CONNECTION, SERVER_PROGRAM_NAME};

pub(super) fn instance_error(error: instance::Error) -> HostError {
    HostError::Io {
        context: "preparing the instance",
        source: io::Error::other(error),
    }
}

pub(super) fn startup_server_check(config: &Config) -> Result<ServerCheck, HostError> {
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

pub(super) fn startup_listener(
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

pub(super) fn startup_keys(
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

pub(super) fn startup_app_policy(config: &Config, selected: bool) -> Result<AppPolicy, HostError> {
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

pub(super) fn startup_authentication(
    layout: &Layout,
    instance: &Option<instance::registry::Instance>,
    custody: &mut Option<Arc<dyn host_secrets::HostSecrets>>,
    dev_key_dir: Option<&Path>,
    release_directory: Option<&ReleaseRoot>,
) -> Result<Option<Arc<crate::authentication::Authentication>>, HostError> {
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
                    crate::authentication::Authentication::open(
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

pub(super) fn startup_catalog(
    layout: &Layout,
    authentication: &Option<Arc<crate::authentication::Authentication>>,
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

/// The listener's TLS pair, for an instance whose listener is `https` and whose registry
/// exists (`web_tls::WebTls::start`): made, imported, renewed or kept before the web process
/// is started. A pair that cannot be served and may not be made again (a supplied one)
/// refuses the start, before anything is served.
pub(super) fn startup_web_tls(
    instance: &Option<instance::registry::Instance>,
    authentication: &Option<Arc<crate::authentication::Authentication>>,
    supplied: Option<instance::setup::SuppliedTls>,
) -> Result<Option<Arc<web_tls::WebTls>>, HostError> {
    let (Some(instance), Some(authentication)) = (instance, authentication) else {
        return Ok(None);
    };
    let Some(listen) = instance.browser().map_err(instance_error)? else {
        return Ok(None);
    };
    if listen.scheme() != org_host_control::listen::ListenMode::Https {
        return Ok(None);
    }
    let refused = |why: String| {
        host_log::log(&format!("the host does not start: {why}"));
        HostError::Io {
            context: "preparing the browser listener's TLS certificate",
            source: io::Error::other(why),
        }
    };
    let now = web_tls::now().ok_or_else(|| refused("the clock is unavailable".into()))?;
    let (db, secrets) = authentication.provisioning_storage();
    web_tls::WebTls::start(
        db,
        secrets,
        listen.origin().clone(),
        supplied
            .as_ref()
            .map(|tls| (tls.cert.as_path(), tls.key.as_path())),
        now,
    )
    .map(Some)
    .map_err(|why| refused(why.0))
}

pub(super) fn startup_web(
    config: &Config,
    instance: &Option<instance::registry::Instance>,
    layout: &Layout,
    dev_key_dir: Option<&Path>,
    tls: Option<Arc<web_tls::WebTls>>,
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
        tls,
        on_renewal_kept: None,
    }))
}

pub(super) struct ProtectedServices {
    pub(super) writers: Option<Arc<writer::Writers>>,
    pub(super) supervisor: Supervisor,
    pub(super) provisioning: Option<Arc<provisioning::Provisioning>>,
}

pub(super) fn startup_protected_services(
    instance: &Option<instance::registry::Instance>,
    authentication: &Option<Arc<crate::authentication::Authentication>>,
    supervisor: Supervisor,
    keys: &Arc<dyn keys::OfficeSecretsKeys>,
    layout: &Layout,
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
            Some(Arc::new(provisioning::Provisioning::new(
                db,
                custody,
                Arc::clone(keys),
                Arc::clone(writers),
                instance.id(),
                &layout.home,
            )))
        }
        _ => None,
    };
    Ok(ProtectedServices {
        writers,
        supervisor,
        provisioning,
    })
}

/// Activates local authless onboarding mode (ADR-0034) on a brand-new local HTTP instance
/// that has zero accounts and zero issued registration links.
pub(super) fn maybe_init_local_authless(
    instance: Option<&instance::registry::Instance>,
    authentication: Option<&Arc<crate::authentication::Authentication>>,
) {
    let is_local_http = instance
        .and_then(|i| i.browser().ok().flatten())
        .is_some_and(|l| match l.scheme() {
            org_host_control::listen::ListenMode::HttpLoopback => true,
            org_host_control::listen::ListenMode::HttpPublished => matches!(
                l.origin().host().to_string().as_str(),
                "127.0.0.1" | "[::1]" | "::1" | "localhost"
            ),
            org_host_control::listen::ListenMode::Https => false,
        });
    if !is_local_http {
        return;
    }
    let Some(auth) = authentication else {
        return;
    };
    let Some(now_ms) = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
    else {
        return;
    };
    let (db, _) = auth.provisioning_storage();
    if let Err(err) = db.init_local_authless_if_empty(now_ms) {
        host_log::log(&format!(
            "could not initialize local authless onboarding mode ({err:?}); authentication remains enforced"
        ));
    }
}
