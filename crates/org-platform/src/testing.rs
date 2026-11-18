//! Test adapters built from the contracts, for consumer tests (`test-support`).
use crate::{
    disk::{DiskEncryption, EncryptionState},
    service::{Registration, ServiceError, ServiceManager, ServiceStatus, Unit},
};
use std::{collections::BTreeMap, path::Path, sync::Mutex};

/// One recorded service-manager call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    Register(Unit),
    Start(String),
    Stop(String),
    Status(String),
    Unregister(String),
}

/// Records every call and keeps the registrations in memory. A registered unit starts
/// running (as `RunAtLoad` does); it never runs a process.
#[derive(Default)]
pub struct FakeServiceManager {
    state: Mutex<(Registrations, Vec<Call>)>,
}

/// Each instance's unit and whether it runs.
type Registrations = BTreeMap<String, (Unit, bool)>;

impl FakeServiceManager {
    pub fn calls(&self) -> Vec<Call> {
        self.state.lock().unwrap().1.clone()
    }
}

impl ServiceManager for FakeServiceManager {
    fn register(&self, unit: &Unit) -> Result<Registration, ServiceError> {
        crate::service::validate(unit)?;
        let mut s = self.state.lock().unwrap();
        s.1.push(Call::Register(unit.clone()));
        match s.0.get(&unit.instance_id) {
            Some((existing, _)) if existing != unit => Err(ServiceError::Conflict(
                "another definition uses this instance".into(),
            )),
            Some(_) => Ok(Registration::Unchanged),
            None => {
                s.0.insert(unit.instance_id.clone(), (unit.clone(), true));
                Ok(Registration::Loaded)
            }
        }
    }
    fn start(&self, id: &str) -> Result<(), ServiceError> {
        let mut s = self.state.lock().unwrap();
        s.1.push(Call::Start(id.into()));
        let entry =
            s.0.get_mut(id)
                .ok_or(ServiceError::Invalid("the service is not registered"))?;
        entry.1 = true;
        Ok(())
    }
    fn stop(&self, id: &str) -> Result<(), ServiceError> {
        let mut s = self.state.lock().unwrap();
        s.1.push(Call::Stop(id.into()));
        if let Some(entry) = s.0.get_mut(id) {
            entry.1 = false;
        }
        Ok(())
    }
    fn status(&self, id: &str) -> Result<ServiceStatus, ServiceError> {
        let mut s = self.state.lock().unwrap();
        s.1.push(Call::Status(id.into()));
        Ok(match s.0.get(id) {
            None => ServiceStatus::NotRegistered,
            Some((_, true)) => ServiceStatus::Running { pid: 1 },
            Some((_, false)) => ServiceStatus::Stopped,
        })
    }
    fn unregister(&self, id: &str) -> Result<(), ServiceError> {
        let mut s = self.state.lock().unwrap();
        s.1.push(Call::Unregister(id.into()));
        s.0.remove(id);
        Ok(())
    }
}

/// Reports a fixed state.
pub struct FakeDiskEncryption(pub EncryptionState);

impl DiskEncryption for FakeDiskEncryption {
    fn state(&self, _path: &Path) -> EncryptionState {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fake_manager_is_idempotent_and_refuses_a_different_definition() {
        let fake = FakeServiceManager::default();
        let unit = Unit {
            instance_id: "abc".into(),
            program: "/bin/x".into(),
            args: vec![],
            env: vec![],
        };
        assert_eq!(fake.register(&unit).unwrap(), Registration::Loaded);
        assert_eq!(fake.register(&unit).unwrap(), Registration::Unchanged);
        let other = Unit {
            args: vec!["y".into()],
            ..unit.clone()
        };
        assert!(matches!(
            fake.register(&other),
            Err(ServiceError::Conflict(_))
        ));
        fake.stop("abc").unwrap();
        assert_eq!(fake.status("abc").unwrap(), ServiceStatus::Stopped);
        assert_eq!(fake.calls()[0], Call::Register(unit));
    }
}
