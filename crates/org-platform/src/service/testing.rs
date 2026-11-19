//! For consumer tests (`test-support`): the build target's own `Registered` adapter with
//! stand-in programs in place of the system's service manager commands.
use super::{imp, Placement, ServiceManager};
use crate::Refused;
use std::path::{Path, PathBuf};

/// The build target's `Registered` adapter, with the programs it runs looked for in
/// `programs` alone and the user's runtime directory given instead of read from the
/// environment. On GNU/Linux the programs are `systemctl` and `loginctl`; a test puts
/// stand-ins there, so the real adapter and its real runner are exercised and systemd is
/// never touched. A target whose adapter cannot be pointed at stand-in programs refuses.
pub fn registered_with_programs(
    placement: Placement,
    programs: &Path,
    runtime_dir: Option<PathBuf>,
) -> Result<Box<dyn ServiceManager>, Refused> {
    imp::registered_with_programs(placement, programs, runtime_dir)
}
