//! The refusing service adapter: no service manager and no foreground mode.

use super::{ServiceManager, ServiceMode};
use crate::Refused;

pub(super) fn service(_mode: ServiceMode) -> Result<Box<dyn ServiceManager>, Refused> {
    Err(Refused::unsupported("service"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_refuses() {
        for mode in [ServiceMode::Registered, ServiceMode::Foreground] {
            let refused = service(mode).err().expect("the refusing adapter refuses");
            assert_eq!(refused, Refused::unsupported("service"));
        }
    }
}
