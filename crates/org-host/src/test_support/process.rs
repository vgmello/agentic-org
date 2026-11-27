//! Debug-only test harness helpers for external host adapter/composition crates
//! (`org-host-sqlite`, `org-host-setup`). Never compiled in release builds.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use org_host_control::listen::ListenOrigin;
use org_host_control::Password;
use org_proto::{ApiError, OfficeInfo};

use crate::admin::{Admin, NativePresence};
use crate::control_listener::Connection;
use crate::home::HostLock;
use crate::instance::registry::Instance;
use crate::instance::setup::Paths;
use crate::ports::{
    accounts::Verifier, HostAccountsStore, HostPorts, HostSecrets, HostStore, HostStoreOpener,
};
use crate::recovery::TestPresence;
use crate::writer::{Binding, Writers};
use crate::{Config, Host, HostError};

pub use crate::registered::{RegistryState, REGISTRY_MISSING};

pub fn hash_password(password: &Password) -> Result<Verifier, ApiError> {
    crate::authentication::hash_password(password)
}

pub fn open_dev_secrets(
    dev_key_dir: &Path,
    instance_root: &Path,
    instance_id: &str,
) -> Result<Arc<dyn HostSecrets>, String> {
    crate::host_secrets::store_for(
        Some(dev_key_dir),
        instance_root,
        instance_id,
        crate::code_signing::own_signing,
        None,
    )
    .map_err(|e| format!("{e:?}"))
}

pub fn open_dev_secrets_raw(
    dev_key_dir: &Path,
    office_home: &Path,
    instance_id: &str,
) -> Result<Arc<dyn HostSecrets>, String> {
    crate::host_secrets::dev::DevSecrets::open(dev_key_dir, office_home, instance_id)
        .map(|s| Arc::new(s) as Arc<dyn HostSecrets>)
        .map_err(|e| format!("{e:?}"))
}

pub struct WriterBinding<'a> {
    pub organization_id: &'a str,
    pub office_id: &'a str,
    pub owner_account: &'a str,
    pub owner_member: &'a str,
}

pub fn reserve_writer_office(
    custody: Arc<dyn HostSecrets>,
    accounts: Arc<dyn HostAccountsStore>,
    instance_id: &str,
    instance_root: &Path,
    binding: WriterBinding<'_>,
) -> Result<(), String> {
    let writers =
        Writers::open(custody, accounts, instance_id, instance_root).map_err(|e| e.to_string())?;
    let operation = crate::ids::uuid_v7().map_err(|e| e.to_string())?;
    writers
        .reserve(
            &operation,
            &Binding {
                organization_id: binding.organization_id,
                office_id: binding.office_id,
                owner_account: binding.owner_account,
                owner_member: binding.owner_member,
            },
        )
        .map_err(|e| format!("reserving writer custody for {}: {e}", binding.office_id))?;
    Ok(())
}

pub fn registry_state_with(
    instance: &Instance,
    office_home: &Path,
    opener: &dyn HostStoreOpener,
) -> Result<RegistryState, HostError> {
    crate::registered::registry_state_with(instance, office_home, opener)
}

pub fn create_registry_with(
    office_home: &Path,
    custody: &dyn HostSecrets,
    instance_id: &str,
    opener: &dyn HostStoreOpener,
) -> Result<Arc<dyn HostStore>, HostError> {
    crate::registered::create_registry_with(office_home, custody, instance_id, opener)
}

pub fn start_registered_at_with_ports(
    config: Config,
    paths: &Paths,
    name: Option<&str>,
    ports: HostPorts,
) -> Result<Host, HostError> {
    Host::start_registered_at_with_ports(config, paths, name, ports)
}

pub fn start_prepared_with_ports(
    config: Config,
    instance: Option<Instance>,
    prepared: Option<(HostLock, Arc<dyn HostSecrets>)>,
    ports: HostPorts,
) -> Result<Host, HostError> {
    Host::start_prepared_with_ports(config, instance, prepared, ports)
}

pub fn install_instance_name(host: &Host, name: &str) -> bool {
    host.service.install_instance_name(name)
}

pub fn install_admin_for_test(
    host: &Host,
    config: &Config,
    origin: ListenOrigin,
) -> Result<bool, HostError> {
    let policy = crate::startup::startup_app_policy(config, true)?;
    let auth = host
        .service
        .authentication
        .as_ref()
        .ok_or_else(|| HostError::Io {
            context: "installing admin for test",
            source: std::io::Error::other("registered host has no authentication registry"),
        })?;
    let op_id = crate::ids::uuid_v7().map_err(|source| HostError::Io {
        context: "generating test admin setup op",
        source,
    })?;
    Ok(host.service.install_admin(Arc::new(Admin::new(
        auth.accounts_store(),
        auth.links_store(),
        &op_id,
        origin,
        policy,
        Arc::new(NativePresence),
    ))))
}

pub fn offices(host: &Host) -> Vec<OfficeInfo> {
    host.offices.offices()
}

pub fn kill_office(host: &Host, office_id: &str) {
    host.offices.kill_office_for_test(office_id);
}

pub fn close_writer_channel(host: &Host, office_id: &str) -> bool {
    host.close_writer_channel_for_test(office_id)
}

pub fn browser_ops(host: &Host) -> usize {
    host.service.browser_ops()
}

pub fn organization_ops(host: &Host) -> usize {
    host.service.organization_ops()
}

pub fn instance_id(host: &Host) -> Option<String> {
    host.service.instance_id_for_test().map(str::to_owned)
}

pub fn has_web_tls(host: &Host) -> bool {
    host.service.web_tls_for_test().is_some()
}

pub fn web_tls_chain(host: &Host) -> Option<String> {
    host.service
        .web_tls_for_test()
        .and_then(|tls| tls.fetch().ok().map(|(chain, _)| chain))
}

type ConfirmFn = Box<dyn Fn(&dyn Fn() -> bool) -> Result<(), ()> + Send + Sync>;

struct ClosurePresence {
    local: Box<dyn Fn() -> bool + Send + Sync>,
    confirm: ConfirmFn,
}

impl TestPresence for ClosurePresence {
    fn local(&self, _: &Connection) -> bool {
        (self.local)()
    }

    fn confirm(&self, cancelled: &dyn Fn() -> bool) -> Result<(), ()> {
        (self.confirm)(cancelled)
    }
}

pub fn with_recovery_presence<T>(
    host: &Host,
    local: impl Fn() -> bool + Send + Sync + 'static,
    confirm: impl Fn(&dyn Fn() -> bool) -> Result<(), ()> + Send + Sync + 'static,
    run: impl FnOnce() -> T,
) -> T {
    let presence = Arc::new(ClosurePresence {
        local: Box::new(local),
        confirm: Box::new(confirm),
    });
    host.service
        .recovery_for_test()
        .with_test_presence(presence, run)
}

pub fn captured_web_stderr(instance: &str) -> Option<Vec<u8>> {
    crate::web::captured_stderr(instance)
}

pub fn image_identity() -> org_platform_host::admission::ImageIdentity {
    org_platform_host::admission::image_identity()
        .expect("the host's tests need an admission adapter")
}

pub fn sibling_program(name: &str) -> PathBuf {
    let test_binary = std::env::current_exe().expect("current test executable");
    let profile = test_binary
        .parent()
        .and_then(Path::parent)
        .expect("cargo profile dir");
    let built = profile.join(name);
    assert!(
        built.is_file(),
        "{} is not built: run `cargo test --workspace`, or `cargo build -p {name}` first",
        built.display()
    );
    built
}

pub fn fake_web_program() -> PathBuf {
    static COPY: OnceLock<PathBuf> = OnceLock::new();
    COPY.get_or_init(|| {
        let test_binary = std::env::current_exe().expect("current test executable");
        let profile = test_binary
            .parent()
            .and_then(Path::parent)
            .expect("cargo profile dir");
        let built = profile.join("examples").join("fake_web");
        assert!(
            built.is_file(),
            "{} is not built: run `cargo build -p org-host --example fake_web` first",
            built.display()
        );
        let folder = built
            .parent()
            .expect("examples dir")
            .join("private-programs");
        fs::create_dir_all(&folder).expect("create private-programs");
        let staged = folder.join(format!(".fake_web.{}", std::process::id()));
        let _ = fs::remove_file(&staged);
        fs::copy(&built, &staged).expect("copy fake_web");
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o700)).expect("chmod fake_web");
        let private = folder.join("fake_web");
        fs::rename(&staged, &private).expect("rename fake_web");
        private
    })
    .clone()
}

pub fn open_authentication(
    store: Arc<dyn HostStore>,
    custody: Arc<dyn HostSecrets>,
    instance: &str,
) -> Result<(), ApiError> {
    crate::authentication::Authentication::with_store(store, custody, instance).map(drop)
}

pub fn pair_for(names: &[&str], from: i64, to: i64) -> (String, String) {
    use rcgen::{CertificateParams, KeyPair};
    let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).expect("generate key");
    let mut params =
        CertificateParams::new(names.iter().map(|n| (*n).to_string()).collect::<Vec<_>>())
            .expect("certificate params");
    params.not_before = time::OffsetDateTime::from_unix_timestamp(from).expect("not_before");
    params.not_after = time::OffsetDateTime::from_unix_timestamp(to).expect("not_after");
    let cert = params.self_signed(&key).expect("self-signed cert");
    (cert.pem(), key.serialize_pem())
}

pub fn has_admin(host: &Host) -> bool {
    host.service.admin().is_some()
}

pub fn host_store(host: &Host) -> Arc<dyn HostStore> {
    host.service
        .authentication
        .as_ref()
        .expect("registered host authentication")
        .provisioning_storage()
        .0
}

pub fn host_custody(host: &Host) -> Arc<dyn HostSecrets> {
    host.service
        .authentication
        .as_ref()
        .expect("registered host authentication")
        .provisioning_storage()
        .1
}

pub fn verify_host_instance(
    host: &Host,
    instance_id: &str,
) -> Result<(), crate::ports::host_store::Error> {
    host.service
        .authentication
        .as_ref()
        .expect("registered host authentication")
        .accounts_store()
        .verify_instance(instance_id)
}

pub fn admit_runtime(config: &mut Config) -> Result<(), HostError> {
    config.admit_runtime()
}

pub fn hold_creation_drives(host: &Host) {
    host.creating
        .as_ref()
        .expect("a registered host creates organizations")
        .hold_drives();
}

pub fn create_organization_in_process(
    host: &Host,
    login: &str,
    operation_id: &str,
    plan: &org_proto::selected_bootstrap::PlanV1,
    fingerprint: [u8; 32],
) -> Result<
    org_host_control::HostOpResult<org_host_control::organization::OrganizationView>,
    ApiError,
> {
    let creating = host
        .creating
        .as_ref()
        .expect("a registered host creates organizations");
    let account = creating.account_for_test(login);
    creating.create(&account, operation_id, plan, fingerprint)
}

pub fn web_supervisor_ended(host: &Host) -> bool {
    host.web
        .as_ref()
        .expect("the web process is supervised")
        .ended()
}

pub fn watch_provisioning(host: &Host) -> impl Fn() -> bool {
    let creating = Arc::downgrade(
        host.creating
            .as_ref()
            .expect("a registered host creates organizations"),
    );
    move || creating.strong_count() == 0
}
