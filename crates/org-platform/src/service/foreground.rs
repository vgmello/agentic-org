//! The foreground mode: nothing is registered, on any system.
//!
//! Whoever runs `agentic-org serve` is the supervisor (a container runtime, a person at a
//! terminal). This manager therefore changes nothing and asks no service manager anything:
//! `register`, `start`, `stop` and `unregister` succeed without doing anything, and
//! `status` answers [`ServiceStatus::NotRegistered`], which is all a manager that registers
//! nothing can know. Whether the instance runs is known to the instance's own socket, which
//! the consumer asks. A unit or an instance ID that no definition could carry is still
//! refused, as in every mode.
use super::{
    valid_instance_id, validate, Registration, ServiceError, ServiceManager, ServiceStatus, Unit,
};

pub(super) struct Foreground;

impl ServiceManager for Foreground {
    fn register(&self, unit: &Unit) -> Result<Registration, ServiceError> {
        validate(unit)?;
        Ok(Registration::Unchanged)
    }

    fn start(&self, instance_id: &str) -> Result<(), ServiceError> {
        valid_instance_id(instance_id)
    }

    fn stop(&self, instance_id: &str) -> Result<(), ServiceError> {
        valid_instance_id(instance_id)
    }

    fn status(&self, instance_id: &str) -> Result<ServiceStatus, ServiceError> {
        valid_instance_id(instance_id)?;
        Ok(ServiceStatus::NotRegistered)
    }

    fn unregister(&self, instance_id: &str) -> Result<(), ServiceError> {
        valid_instance_id(instance_id)
    }
}
