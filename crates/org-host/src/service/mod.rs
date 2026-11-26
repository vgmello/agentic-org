//! host.sock's private control boundary: registered-instance hello and count-only status.
//! Registered hosts additionally dispatch live account sign-in, renewal and logout, the
//! administrator link calls (`admin.link.initial`, `admin.reset`; see `crate::admin`) and,
//! for an `https` listener, the administrator's `tls.set` (`crate::web_tls`).

mod administration;
pub(crate) mod authentication;
mod browser;
mod browser_sessions;
mod cli_approvals;
mod dispatch;
pub(crate) mod listener;
mod local_access;
mod multi_person;
mod native_org;
mod office_join;
mod office_ops;
pub(crate) mod organizations;
mod reply;
mod request;
mod session_office;
mod sessions;

use std::sync::Arc;

use org_host_control::WebState;

use crate::broadcast::Broadcast;
use crate::offices::Offices;
use authentication::Authentication;

pub(crate) struct Service {
    offices: Arc<Offices>,
    broadcast: Arc<Broadcast>,
    instance_id: Option<String>,
    pub(crate) authentication: Option<Arc<Authentication>>,
    gateway: Arc<std::sync::Mutex<WebState>>,
    pub(super) provisioning: Option<Arc<crate::provisioning::Provisioning>>,
    recovery: Option<Arc<crate::recovery::Recovery>>,
    /// The `https` listener's pair: `tls.fetch`, `tls.set` and `instance.view`'s `tls`.
    /// `None` for a plain HTTP listener.
    pub(super) web_tls: Option<Arc<crate::web_tls::WebTls>>,
    pub(super) instance: Option<Arc<crate::instance::registry::Instance>>,
    tls_renewal_retries: std::sync::atomic::AtomicUsize,
    /// Installed once by the registered start after composition; until then the admin calls
    /// answer `unavailable`.
    admin: std::sync::OnceLock<Arc<crate::admin::Admin>>,
    /// The registered instance's browser origin (`http://...` or `https://...`) used when
    /// minting browser-approved CLI sign-in URLs (`cli.approval.create`).
    browser_origin: Option<String>,
    /// The registered instance's name, installed once by the registered start; until then
    /// `instance.view` answers `unavailable`.
    instance_name: std::sync::OnceLock<String>,
    /// Serializes journaled `office.restart` / `browser.office_restart` lookup -> restart ->
    /// journal write so concurrent retries cannot race into false `Conflict` or duplicate restarts.
    pub(super) office_restarting: std::sync::Mutex<()>,
    /// Serializes HTTP->HTTPS upgrade writes across `HostDb::web_tls` and `instance.json`.
    pub(super) tls_upgrading: std::sync::Mutex<()>,
    cli_approvals: cli_approvals::CliApprovals,
    /// Browser ops this service accepted on a web connection; real-process tests prove
    /// that an ingress refusal reaches no host op.
    #[cfg(test)]
    browser_ops: std::sync::atomic::AtomicUsize,
    /// Organization calls answered on the admitted web connection; real-process tests
    /// prove that a refused request reaches none.
    #[cfg(test)]
    organization_ops: std::sync::atomic::AtomicUsize,
}

impl Service {
    pub(crate) fn new(
        offices: Arc<Offices>,
        broadcast: Arc<Broadcast>,
        instance_id: Option<String>,
    ) -> Self {
        Self {
            offices,
            broadcast,
            instance_id,
            authentication: None,
            provisioning: None,
            recovery: None,
            web_tls: None,
            instance: None,
            tls_renewal_retries: std::sync::atomic::AtomicUsize::new(0),
            admin: std::sync::OnceLock::new(),
            browser_origin: None,
            instance_name: std::sync::OnceLock::new(),
            office_restarting: std::sync::Mutex::new(()),
            tls_upgrading: std::sync::Mutex::new(()),
            cli_approvals: cli_approvals::CliApprovals::new(),
            #[cfg(test)]
            browser_ops: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            organization_ops: std::sync::atomic::AtomicUsize::new(0),
            gateway: Arc::new(std::sync::Mutex::new(WebState::NotConfigured)),
        }
    }
    pub(crate) fn with_authentication(
        mut self,
        authentication: Option<Arc<Authentication>>,
    ) -> Self {
        self.authentication = authentication;
        self
    }
    pub(crate) fn with_provisioning(
        mut self,
        provisioning: Option<Arc<crate::provisioning::Provisioning>>,
    ) -> Self {
        self.provisioning = provisioning;
        self
    }
    pub(crate) fn with_recovery(
        mut self,
        recovery: Option<Arc<crate::recovery::Recovery>>,
    ) -> Self {
        self.recovery = recovery;
        self
    }
    pub(crate) fn with_web_tls(mut self, web_tls: Option<Arc<crate::web_tls::WebTls>>) -> Self {
        self.web_tls = web_tls;
        self
    }
    pub(crate) fn with_instance(
        mut self,
        instance: Option<Arc<crate::instance::registry::Instance>>,
    ) -> Self {
        self.instance = instance;
        self
    }
    pub(crate) fn with_browser_origin(mut self, origin: Option<String>) -> Self {
        self.browser_origin = origin;
        self
    }
    #[cfg(test)]
    pub(crate) fn instance_id_for_test(&self) -> Option<&str> {
        self.instance_id.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn web_tls_for_test(&self) -> Option<&Arc<crate::web_tls::WebTls>> {
        self.web_tls.as_ref()
    }
    /// The registered host's administrator link calls. Installed at most once.
    #[cfg(test)]
    pub(crate) fn admin(&self) -> Option<&Arc<crate::admin::Admin>> {
        self.admin.get()
    }
    pub(crate) fn install_admin(&self, admin: Arc<crate::admin::Admin>) -> bool {
        self.admin.set(admin).is_ok()
    }
    #[cfg(test)]
    pub(crate) fn browser_ops(&self) -> usize {
        self.browser_ops.load(std::sync::atomic::Ordering::Acquire)
    }
    /// The creation state a create thread keeps alive until it ends.
    #[cfg(test)]
    pub(crate) fn provisioning_for_test(&self) -> Option<&Arc<crate::provisioning::Provisioning>> {
        self.provisioning.as_ref()
    }
    #[cfg(test)]
    pub(crate) fn organization_ops(&self) -> usize {
        self.organization_ops
            .load(std::sync::atomic::Ordering::Acquire)
    }
    /// The registered name `instance.view` reports. Installed at most once.
    pub(crate) fn install_instance_name(&self, name: &str) -> bool {
        self.instance_name.set(name.to_owned()).is_ok()
    }
    pub(crate) fn with_web_state(mut self, state: Arc<std::sync::Mutex<WebState>>) -> Self {
        self.gateway = state;
        self
    }
    /// Whether the host may start its web process: the instance is registered and its
    /// host authentication is present. No organization or office is required: the person
    /// registers from setup's link and creates the organization in the browser, so routes
    /// that need one answer their own "not yet" state. Persisted listener configuration
    /// alone never qualifies.
    pub(crate) fn web_eligible(&self) -> bool {
        web_start_eligible(self.authentication.is_some(), self.registered())
    }
    /// The start-eligibility predicate the host wires into its web process supervisor:
    /// [`Service::web_eligible`], evaluated live on each supervisor poll.
    pub(crate) fn web_eligibility(self: &Arc<Self>) -> Arc<dyn Fn() -> bool + Send + Sync> {
        let service = Arc::clone(self);
        Arc::new(move || service.web_eligible())
    }
    /// Whether browsers can be served: eligible, and an admitted web process is serving
    /// (the supervisor publishes `Running` only between admission and exit/EOF).
    pub(crate) fn web_ready(&self) -> bool {
        web_serving(
            self.authentication.is_some(),
            self.registered(),
            *crate::sync::lock(&self.gateway),
        )
    }
    fn registered(&self) -> bool {
        self.instance_id.is_some()
    }
    #[cfg(test)]
    pub(crate) fn recovery_for_test(&self) -> Arc<crate::recovery::Recovery> {
        Arc::clone(self.recovery.as_ref().expect("selected test host"))
    }
}

/// The start-eligibility rule: the instance is registered and host authentication is
/// present. HQ being Active is not required. Persisted listener configuration alone never
/// qualifies.
pub(crate) fn web_start_eligible(authenticated: bool, registered: bool) -> bool {
    authenticated && registered
}

/// The readiness rule: all three conditions, never persisted state alone.
pub(crate) fn web_serving(authenticated: bool, registered: bool, state: WebState) -> bool {
    web_start_eligible(authenticated, registered) && state == WebState::Running
}

#[cfg(test)]
pub(crate) mod tests;
