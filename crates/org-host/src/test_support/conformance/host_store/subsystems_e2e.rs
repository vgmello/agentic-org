//! End-to-end live host subsystem composition conformance case over any `HostStore`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use org_host_control::admin::AdminLinkResult;
use org_host_control::browser::{
    BrowserOrigin, Digest32, LinkRegisterParams, SessionSignInParams, SessionValidateParams,
};
use org_host_control::browser_accounts::{
    InspectLinkKind, LinkInspectParams, LinkInspectResult, LinkSetPasswordParams,
};
use org_host_control::listen::ListenOrigin;
use org_host_control::{LinkToken, Password};
use org_proto::OfficeSecretsKey;

use super::{MemorySecrets, INSTANCE, OP_ID};
use crate::admin::{Admin, Presence};
use crate::app_auth::AppPolicy;
use crate::authentication::Authentication;
use crate::control_listener::Connection;
use crate::host_secrets::HostSecrets;
use crate::hostdb::{HostAccountsStore, HostStore, WebTlsStore};
use crate::keys::{KeyStoreError, OfficeSecretsKeys};
use crate::links::OneTimeLinks;
use crate::peer::{Accepted, Received};
use crate::provisioning::Provisioning;
use crate::web_tls::WebTls;
use crate::writer::Writers;

#[derive(Default)]
struct ConformanceKeys(Mutex<BTreeMap<String, [u8; 32]>>);

impl OfficeSecretsKeys for ConformanceKeys {
    fn create(&self, office_id: &str, key: &OfficeSecretsKey) -> Result<(), KeyStoreError> {
        if !org_office_launch::is_plain_id(office_id) {
            return Err(KeyStoreError::NotPlainId);
        }
        let mut guard = self.0.lock().unwrap();
        if guard.contains_key(office_id) {
            return Err(KeyStoreError::Exists);
        }
        guard.insert(office_id.to_owned(), *key.expose_bytes());
        Ok(())
    }

    fn load(&self, office_id: &str) -> Result<Option<OfficeSecretsKey>, KeyStoreError> {
        if !org_office_launch::is_plain_id(office_id) {
            return Err(KeyStoreError::NotPlainId);
        }
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(office_id)
            .copied()
            .map(OfficeSecretsKey::from_bytes))
    }

    fn delete(&self, office_id: &str) -> Result<(), KeyStoreError> {
        if !org_office_launch::is_plain_id(office_id) {
            return Err(KeyStoreError::NotPlainId);
        }
        self.0.lock().unwrap().remove(office_id);
        Ok(())
    }

    fn list(&self) -> Result<Vec<String>, KeyStoreError> {
        Ok(self.0.lock().unwrap().keys().cloned().collect())
    }
}

pub fn live_host_subsystems_run_end_to_end(store: Arc<dyn HostStore>, state_root: &Path) {
    struct LocalApprove;
    impl Presence for LocalApprove {
        fn local(&self, _: &Connection) -> bool {
            true
        }
        fn confirm(&self, _: &dyn Fn() -> bool) -> Result<(), ()> {
            Ok(())
        }
    }

    let secrets: Arc<dyn HostSecrets> = Arc::new(MemorySecrets::default());

    let auth = Authentication::with_store(Arc::clone(&store), Arc::clone(&secrets), INSTANCE)
        .expect("Authentication wired over HostStore");
    let admin = Admin::new(
        Arc::clone(&store) as Arc<dyn HostAccountsStore>,
        Arc::clone(&store) as Arc<dyn OneTimeLinks + Send + Sync>,
        OP_ID,
        ListenOrigin::parse("http://127.0.0.1:7420").unwrap(),
        AppPolicy::new(crate::HOST_SIGNING_ID, None).unwrap(),
        Arc::new(LocalApprove),
    );

    // 1. Issue initial admin link through Admin -> register through Authentication -> validate & sign in.
    let (host_sock, _peer_sock) = std::os::unix::net::UnixStream::pair().unwrap();
    let (root_conn, _outbound) =
        Connection::new(host_sock.try_clone().unwrap(), Accepted::sample(&host_sock));
    root_conn.mark_root_authority();
    let received = Received {
        sent_at: 0,
        buffered_after: 0,
    };
    let AdminLinkResult::Issued { url, .. } = admin.reset(&root_conn, received).unwrap() else {
        panic!("expected initial registration link");
    };
    let token_str = url
        .expose()
        .strip_prefix("http://127.0.0.1:7420/register#")
        .expect("registration token")
        .to_owned();
    let live_now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let origin = BrowserOrigin {
        scheme: "http".into(),
        host: "127.0.0.1".into(),
        port: 7420,
    };
    let mut ended = Vec::new();
    let registered = auth
        .register(
            &LinkRegisterParams {
                link: LinkToken::new(token_str).unwrap(),
                login: "Alice".into(),
                password: Password::new("initial-secret-passphrase".into()).unwrap(),
                client_address: "127.0.0.1".parse().unwrap(),
                cookie_digest: Digest32([0x11; 32]),
                proof_digest: Digest32([0x22; 32]),
                origin: origin.clone(),
            },
            live_now + 10,
            &mut ended,
        )
        .expect("browser registration over HostStore");
    assert_eq!(store.has_administrator(), Ok(true));
    assert_eq!(registered.account.login, "Alice");

    let validated = auth
        .validate_browser(
            &SessionValidateParams {
                cookie_digest: Digest32([0x11; 32]),
                proof_digest: Digest32([0x22; 32]),
                origin: origin.clone(),
            },
            live_now + 20,
            &mut ended,
        )
        .expect("browser session validates over HostStore");
    assert_eq!(validated.session_id, registered.session_id);

    let signed_in = auth
        .sign_in_browser(
            &SessionSignInParams {
                login: "alice".into(),
                password: Password::new("initial-secret-passphrase".into()).unwrap(),
                client_address: "127.0.0.1".parse().unwrap(),
                cookie_digest: Digest32([0x33; 32]),
                proof_digest: Digest32([0x44; 32]),
                origin,
            },
            live_now + 30,
            &mut ended,
        )
        .expect("browser sign-in over HostStore");
    assert_eq!(signed_in.account.id, registered.account.id);

    // 2. Admin reset issues password-reset link -> inspect & redeem through Authentication.
    let AdminLinkResult::ResetPassword { login, url, .. } =
        admin.reset(&root_conn, received).unwrap()
    else {
        panic!("expected ResetPassword link");
    };
    assert_eq!(login, "Alice");
    let reset_token = url
        .expose()
        .strip_prefix("http://127.0.0.1:7420/set-password#")
        .expect("set-password token")
        .to_owned();
    let reset_now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let inspected = auth
        .inspect_link(
            &LinkInspectParams {
                link: LinkToken::new(reset_token.clone()).unwrap(),
                kind: InspectLinkKind::SetPassword,
            },
            reset_now,
            |_| String::new(),
        )
        .expect("inspect set-password link over HostStore");
    assert_eq!(
        inspected,
        LinkInspectResult::SetPassword {
            login: "Alice".into(),
        }
    );
    ended.clear();
    let updated = auth
        .redeem_set_password(
            &LinkSetPasswordParams {
                link: LinkToken::new(reset_token).unwrap(),
                password: Password::new("rotated-secret-passphrase".into()).unwrap(),
                client_address: "127.0.0.1".parse().unwrap(),
            },
            reset_now + 10,
            &mut ended,
        )
        .expect("redeem set-password link over HostStore");
    assert_eq!(updated.login, "Alice");
    assert_eq!(ended.len(), 2, "both prior browser sessions ended on reset");

    // 3. WebTls, Writers, and Provisioning compose cleanly over HostStore + MemorySecrets.
    let tls = WebTls::start(
        Arc::clone(&store) as Arc<dyn WebTlsStore>,
        Arc::clone(&secrets),
        ListenOrigin::parse("https://127.0.0.1:8443").unwrap(),
        None,
        crate::web_tls::now().unwrap(),
    )
    .expect("WebTls over HostStore");
    assert!(store.web_tls_pair().unwrap().is_some());
    drop(tls);

    let office_home = state_root.join("office-state");
    std::fs::create_dir_all(office_home.join(org_office_launch::OFFICES_DIR_NAME)).unwrap();
    let writers = Writers::open(
        Arc::clone(&secrets),
        Arc::clone(&store) as Arc<dyn HostAccountsStore>,
        INSTANCE,
        state_root,
    )
    .expect("Writers over HostStore");
    let _provisioning = Provisioning::new(
        Arc::clone(&store),
        Arc::clone(&secrets),
        Arc::new(ConformanceKeys::default()),
        writers,
        INSTANCE,
        &office_home,
    );
}
