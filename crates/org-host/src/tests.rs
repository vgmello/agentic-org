use super::config::root_refusal;
use super::*;

#[test]
fn only_root_is_refused_the_service() {
    let refusal = root_refusal(0).unwrap_err();
    assert!(refusal.contains("does not run as root"), "{refusal}");
    assert!(refusal.contains("service user"), "{refusal}");
    for user in [1, 501, 1000, 61000, u32::MAX] {
        assert_eq!(root_refusal(user), Ok(()), "{user}");
    }
    // The composition's check is this rule for the process's own effective user.
    assert_eq!(Config::refuse_root().is_err(), home::euid() == 0);
}

/// Set in the child of [`a_host_start_closes_the_process_before_it_reads_a_key`].
#[cfg(debug_assertions)]
const START_CHILD: &str = "ORG_HOST_TEST_START_CHILD";

/// A host start closes its process to the other processes of its user before it reaches
/// its key store: a start that is refused before any store is chosen (a relative home)
/// has already done it. A child of the test starts the host, so the test binary itself is
/// not changed for the other tests.
#[cfg(debug_assertions)]
#[test]
fn a_host_start_closes_the_process_before_it_reads_a_key() {
    use org_platform::process::testing::hardened;
    if std::env::var_os(START_CHILD).is_some() {
        // A fresh process is not closed, unless its environment already forbids core
        // dumps on a system where that is all there is to it.
        if hardened().unwrap() {
            println!("this process was closed before the start: only the end is shown");
        }
        // No development key folder is set: reaching the key store would say so.
        let refused = Host::start(Config::new("relative/home", "/nonexistent/worker"));
        assert!(
            matches!(refused, Err(HostError::RelativeHome { .. })),
            "{:?}",
            refused.err().map(|e| e.to_string())
        );
        assert!(hardened().unwrap(), "the start did not close the process");
        println!("closed before the key store");
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::a_host_start_closes_the_process_before_it_reads_a_key",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(START_CHILD, "1")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && text.contains("closed before the key store"),
        "{text}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(debug_assertions)]
struct InMemoryHostSecrets {
    items: std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>,
}

#[cfg(debug_assertions)]
impl InMemoryHostSecrets {
    fn new() -> Self {
        Self {
            items: std::sync::Mutex::new(std::collections::BTreeMap::new()),
        }
    }
}

#[cfg(debug_assertions)]
impl crate::host_secrets::HostSecrets for InMemoryHostSecrets {
    fn put(
        &self,
        name: &crate::host_secrets::Name,
        value: &crate::host_secrets::Value,
    ) -> Result<(), crate::host_secrets::Error> {
        self.items
            .lock()
            .unwrap()
            .insert(name.as_str().to_owned(), value.expose().to_vec());
        Ok(())
    }

    fn load(
        &self,
        name: &crate::host_secrets::Name,
    ) -> Result<Option<crate::host_secrets::Value>, crate::host_secrets::Error> {
        let map = self.items.lock().unwrap();
        match map.get(name.as_str()) {
            Some(bytes) => Ok(Some(crate::host_secrets::Value::new(bytes.clone())?)),
            None => Ok(None),
        }
    }

    fn delete(&self, name: &crate::host_secrets::Name) -> Result<(), crate::host_secrets::Error> {
        self.items.lock().unwrap().remove(name.as_str());
        Ok(())
    }

    fn list(&self) -> Result<Vec<crate::host_secrets::Name>, crate::host_secrets::Error> {
        self.items
            .lock()
            .unwrap()
            .keys()
            .map(|k| crate::host_secrets::Name::parse(k))
            .collect()
    }
}

#[cfg(debug_assertions)]
#[test]
fn host_start_with_ports_uses_injected_keys_without_dev_key_dir() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();

    // Without injected keys and without `dev_key_dir`, Host::start refuses before binding.
    let base_config = Config::new(&home, "/nonexistent/worker");
    assert!(matches!(
        Host::start(base_config.clone()),
        Err(HostError::NoDevKeyDir)
    ));

    // With an injected key store port, Host::start_with_ports boots without touching local key files.
    let ports = HostPorts {
        keys: Some(Arc::new(crate::keys::memory::MemoryKeys::default())),
        ..HostPorts::default()
    };
    let host =
        Host::start_with_ports(base_config.clone(), ports).expect("start_with_ports succeeds");
    assert!(host.socket_path().exists());
    assert!(!host.home().join("host-registry.db").exists());
    host.shutdown();

    // Unregistered Host::start_with_ports fails closed if store or secrets are supplied
    // (those ports require a registered instance via Host::start_registered_with_ports).
    let with_store = HostPorts {
        store: Some(Arc::new(crate::hostdb::memory::MemoryHostStore::new(
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071b0",
            10_000,
        ))),
        keys: Some(Arc::new(crate::keys::memory::MemoryKeys::default())),
        ..HostPorts::default()
    };
    let err = Host::start_with_ports(base_config.clone(), with_store)
        .err()
        .expect("unregistered start_with_ports must refuse store");
    assert!(
        err.to_string().contains("start_registered_with_ports"),
        "unexpected error: {err}"
    );

    let with_secrets = HostPorts {
        secrets: Some(Arc::new(InMemoryHostSecrets::new())),
        keys: Some(Arc::new(crate::keys::memory::MemoryKeys::default())),
        ..HostPorts::default()
    };
    let err = Host::start_with_ports(base_config, with_secrets)
        .err()
        .expect("unregistered start_with_ports must refuse secrets");
    assert!(
        err.to_string().contains("start_registered_with_ports"),
        "unexpected error: {err}"
    );
}

#[cfg(debug_assertions)]
#[test]
fn host_start_with_injected_ports_wires_memory_store_without_sqlite() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    let base = std::env::var_os("ORG_INSTANCE_TEST_DISK_BASE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| org_platform::dirs::testing::disk_backed_temp().unwrap());
    let tmp = tempfile::tempdir_in(&base).unwrap();
    let base_dir = tmp.path().canonicalize().unwrap();
    let locator = base_dir.join("locator");
    let user_temp = base_dir.join("tmp");
    let root = base_dir.join("data");
    for dir in [&locator, &user_temp, &root] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let registry = crate::instance::registry::Registry::open(&locator, &user_temp).unwrap();
    let instance = registry.reserve(Some("mem"), &root).unwrap();
    let instance_id = instance.id().to_owned();

    let store: Arc<dyn crate::ports::HostStore> = Arc::new(
        crate::hostdb::memory::MemoryHostStore::new(&instance_id, 10_000),
    );
    let secrets: Arc<dyn crate::ports::HostSecrets> = Arc::new(InMemoryHostSecrets::new());
    let keys: Arc<dyn crate::ports::OfficeSecretsKeys> =
        Arc::new(crate::keys::memory::MemoryKeys::default());

    let config = Config::new(root.join("unused"), "/nonexistent/worker");
    let ports = HostPorts {
        store: Some(Arc::clone(&store)),
        secrets: Some(Arc::clone(&secrets)),
        keys: Some(Arc::clone(&keys)),
    };
    let host = Host::start_with(config, Some(instance), None, None, ports)
        .expect("Host::start_with wires injected ports");
    assert!(
        !host.home().join("host-registry.db").exists(),
        "injected HostStore must not open or create SQLite host-registry.db"
    );
    let auth = host
        .service
        .authentication
        .as_ref()
        .expect("injected store wires Authentication");
    assert_eq!(auth.accounts_store().verify_instance(&instance_id), Ok(()));
    assert!(
        host.service.provisioning_for_test().is_some(),
        "injected ports wire Provisioning and Writers"
    );
    host.shutdown();

    // Negative check: mismatched instance ID in injected store fails closed.
    let instance_bad = registry.reserve(Some("mem"), &root).unwrap();
    let wrong_store: Arc<dyn crate::ports::HostStore> = Arc::new(
        crate::hostdb::memory::MemoryHostStore::new("0190a5d2-7c3e-7a01-8b2c-3d4e5f6071ff", 10_000),
    );
    let bad_ports = HostPorts {
        store: Some(wrong_store),
        secrets: Some(secrets),
        keys: Some(keys),
    };
    let bad_config = Config::new(root.join("unused"), "/nonexistent/worker");
    assert!(
        Host::start_with(bad_config, Some(instance_bad), None, None, bad_ports).is_err(),
        "mismatched instance ID in injected store must fail closed"
    );
}
