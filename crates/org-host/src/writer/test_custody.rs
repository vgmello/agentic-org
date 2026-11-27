//! Writer custody for tests that start offices, and a custody store that fails on demand.

use std::{
    fs, io,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use crate::host_secrets::{HostSecrets, Name, Value};

use super::{Binding, Writers};

/// Real writer custody for tests that start offices: a development custody directory and a
/// host registry under `dir`, one organization owned by a seeded administrator, and a
/// reservation for each of `offices` (canonical UUIDv7 IDs).
#[cfg(test)]
pub(crate) struct TestCustody {
    pub(crate) writers: Arc<Writers>,
    pub(crate) db: Arc<crate::hostdb::HostDb>,
    pub(crate) account: String,
    pub(crate) organization: String,
    pub(crate) member: String,
    /// While set, every read of the custody store fails with a transient I/O error.
    pub(crate) unreadable: Arc<AtomicBool>,
}

/// A custody store whose reads time out while `fail` is set.
#[cfg(test)]
struct Faulty {
    inner: Arc<dyn HostSecrets>,
    fail: Arc<AtomicBool>,
}

#[cfg(test)]
impl HostSecrets for Faulty {
    fn put(&self, name: &Name, value: &Value) -> Result<(), crate::host_secrets::Error> {
        self.inner.put(name, value)
    }
    fn load(&self, name: &Name) -> Result<Option<Value>, crate::host_secrets::Error> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(crate::host_secrets::Error::Io(io::ErrorKind::TimedOut));
        }
        self.inner.load(name)
    }
    fn delete(&self, name: &Name) -> Result<(), crate::host_secrets::Error> {
        self.inner.delete(name)
    }
    fn list(&self) -> Result<Vec<Name>, crate::host_secrets::Error> {
        self.inner.list()
    }
}

#[cfg(test)]
impl TestCustody {
    pub(crate) fn reserve(dir: &Path, instance: &str, offices: &[&str]) -> Self {
        use std::os::unix::fs::DirBuilderExt;
        let private = |path: &Path| {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(path)
                .unwrap();
        };
        // Its own folder each time: one test may build several supervisors in one `dir`.
        let base = dir.join(format!("custody-{}", crate::ids::generate().unwrap()));
        let root = base.join("instance");
        let home = root.join("office-state");
        let keys = base.join("keys");
        private(&home);
        private(&keys);
        let unreadable = Arc::new(AtomicBool::new(false));
        let custody: Arc<dyn HostSecrets> = Arc::new(Faulty {
            inner: Arc::new(
                crate::host_secrets::dev::DevSecrets::open(&keys, &home, instance).unwrap(),
            ),
            fail: Arc::clone(&unreadable),
        });
        let init = crate::hostdb::HostDbInit {
            instance_id: instance.into(),
            clock_high_ms: 1,
        };
        let db = Arc::new(crate::hostdb::HostDb::create(&home, &*custody, &init).unwrap());
        let account = crate::ids::generate().unwrap();
        let password =
            org_host_control::Password::new("synthetic custody password".into()).unwrap();
        db.seed_account(
            &account,
            "Owner",
            &crate::authentication::hash_password(&password).unwrap(),
            true,
            1,
        )
        .unwrap();
        let admin = db
            .account_authority(&crate::accounts::identity::AccountId::parse(&account).unwrap())
            .unwrap()
            .unwrap();
        let organization = crate::ids::uuid_v7().unwrap();
        let member = crate::ids::uuid_v7().unwrap();
        db.establish_organization_owner(&admin, &organization, &member)
            .unwrap();
        let writers = Writers::open(
            custody,
            Arc::clone(&db) as Arc<dyn crate::hostdb::HostStore>,
            instance,
            &root.canonicalize().unwrap(),
        )
        .unwrap();
        for office in offices {
            fs::create_dir_all(home.join("offices").join(office)).unwrap();
            writers
                .reserve(
                    &crate::ids::uuid_v7().unwrap(),
                    &Binding {
                        organization_id: &organization,
                        office_id: office,
                        owner_account: &account,
                        owner_member: &member,
                    },
                )
                .unwrap();
        }
        Self {
            writers,
            db,
            account,
            organization,
            member,
            unreadable,
        }
    }
}
