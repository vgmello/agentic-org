//! The service contract as executable checks, and an in-memory manager.
//!
//! [`check_registered`] and [`check_foreground`] assert the rules in the
//! [module documentation](super). An adapter's own tests run them against the real
//! manager with a throwaway instance; [`MemoryManager`] is the registered-mode
//! manager for a consumer's tests (setup, `serve`) that must not touch the user's
//! real service manager.

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::{Availability, InstanceId, ServiceError, ServiceManager, ServiceStatus, Unit};

/// A registered-mode manager that keeps its registrations in memory.
pub struct MemoryManager {
    availability: Availability,
    units: Mutex<BTreeMap<String, (Unit, bool)>>,
}

impl MemoryManager {
    /// A manager with no registration, reporting `availability` for each one made.
    pub fn new(availability: Availability) -> Self {
        Self {
            availability,
            units: Mutex::new(BTreeMap::new()),
        }
    }

    /// The unit registered for `instance`, if any.
    pub fn registered(&self, instance: &InstanceId) -> Option<Unit> {
        self.lock()
            .get(instance.as_str())
            .map(|(unit, _)| unit.clone())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, (Unit, bool)>> {
        self.units
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl ServiceManager for MemoryManager {
    fn register(&self, unit: &Unit) -> Result<(), ServiceError> {
        let mut units = self.lock();
        match units.get(unit.instance().as_str()) {
            Some((existing, _)) if existing == unit => Ok(()),
            Some(_) => Err(ServiceError::Conflict),
            None => {
                units.insert(unit.instance().as_str().to_owned(), (unit.clone(), false));
                Ok(())
            }
        }
    }

    fn start(&self, instance: &InstanceId) -> Result<(), ServiceError> {
        let mut units = self.lock();
        let (_, running) = units
            .get_mut(instance.as_str())
            .ok_or(ServiceError::NotRegistered)?;
        *running = true;
        Ok(())
    }

    fn stop(&self, instance: &InstanceId) -> Result<(), ServiceError> {
        let mut units = self.lock();
        let (_, running) = units
            .get_mut(instance.as_str())
            .ok_or(ServiceError::NotRegistered)?;
        *running = false;
        Ok(())
    }

    fn status(&self, instance: &InstanceId) -> Result<ServiceStatus, ServiceError> {
        Ok(match self.lock().get(instance.as_str()) {
            None => ServiceStatus::NotRegistered,
            Some((_, running)) => ServiceStatus::Registered {
                running: *running,
                availability: self.availability.clone(),
            },
        })
    }

    fn unregister(&self, instance: &InstanceId) -> Result<(), ServiceError> {
        self.lock().remove(instance.as_str());
        Ok(())
    }
}

fn running(manager: &dyn ServiceManager, instance: &InstanceId) -> Option<bool> {
    match manager.status(instance).expect("status") {
        ServiceStatus::Registered { running, .. } => Some(running),
        ServiceStatus::NotRegistered => None,
        ServiceStatus::Unmanaged => panic!("a registered-mode manager reported Unmanaged"),
    }
}

/// Asserts the registered-mode rules against `manager`.
///
/// `unit` and `different` are two units for the same instance that differ in program,
/// arguments or environment; the manager must hold no registration for that instance
/// when the check starts, and holds none when it returns. The check starts and stops
/// `unit` for real, so against a real manager its program must be one that keeps
/// running until stopped.
///
/// Not covered, because only the adapter can set them up: a foreign registration
/// ([`ServiceError::Foreign`]), and an instance that exits or fails on its own.
pub fn check_registered(manager: &dyn ServiceManager, unit: &Unit, different: &Unit) {
    let instance = unit.instance();
    assert_eq!(instance, different.instance(), "two units of one instance");
    assert_ne!(unit, different, "two different units");

    assert_eq!(running(manager, instance), None, "starts unregistered");
    assert_eq!(manager.start(instance), Err(ServiceError::NotRegistered));
    assert_eq!(manager.stop(instance), Err(ServiceError::NotRegistered));
    assert_eq!(manager.unregister(instance), Ok(()), "nothing to remove");

    assert_eq!(manager.register(unit), Ok(()));
    assert_eq!(running(manager, instance), Some(false), "not started");
    assert_eq!(manager.register(unit), Ok(()), "the same unit again");
    assert_eq!(manager.register(different), Err(ServiceError::Conflict));
    assert_eq!(manager.stop(instance), Ok(()), "already stopped");

    assert_eq!(manager.start(instance), Ok(()));
    assert_eq!(running(manager, instance), Some(true));
    assert_eq!(manager.start(instance), Ok(()), "already running");
    assert_eq!(
        manager.register(unit),
        Ok(()),
        "the same unit while running"
    );
    assert_eq!(running(manager, instance), Some(true), "left running");
    assert_eq!(manager.register(different), Err(ServiceError::Conflict));
    assert_eq!(
        running(manager, instance),
        Some(true),
        "a conflict changes nothing"
    );

    assert_eq!(manager.stop(instance), Ok(()));
    assert_eq!(running(manager, instance), Some(false));
    assert_eq!(manager.stop(instance), Ok(()), "already stopped");

    assert_eq!(manager.start(instance), Ok(()));
    assert_eq!(manager.unregister(instance), Ok(()), "stops and removes");
    assert_eq!(running(manager, instance), None);
    assert_eq!(manager.start(instance), Err(ServiceError::NotRegistered));
    assert_eq!(manager.unregister(instance), Ok(()), "already removed");

    assert_eq!(
        manager.register(different),
        Ok(()),
        "replaced after removal"
    );
    assert_eq!(running(manager, instance), Some(false));
    assert_eq!(manager.unregister(instance), Ok(()));
    assert_eq!(running(manager, instance), None, "ends unregistered");
}

/// Asserts the foreground-mode rules against `manager`.
pub fn check_foreground(manager: &dyn ServiceManager, unit: &Unit) {
    let instance = unit.instance();
    assert_eq!(manager.status(instance), Ok(ServiceStatus::Unmanaged));
    assert_eq!(manager.register(unit), Ok(()));
    assert_eq!(manager.status(instance), Ok(ServiceStatus::Unmanaged));
    assert_eq!(manager.start(instance), Err(ServiceError::Unmanaged));
    assert_eq!(manager.stop(instance), Err(ServiceError::Unmanaged));
    assert_eq!(manager.unregister(instance), Ok(()));
    assert_eq!(manager.status(instance), Ok(ServiceStatus::Unmanaged));
}
