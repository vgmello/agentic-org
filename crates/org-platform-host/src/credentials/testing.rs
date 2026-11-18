//! Test hooks of the directory store, for this crate's tests and the host's
//! (`test-support`). A test reaches them as `org_platform_host::testing::credentials`.
//!
//! Every hook belongs to the calling thread and acts on writes made on that thread, so
//! tests that run in parallel do not see each other's. A system without the directory
//! store accepts the hooks and never runs them.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::path::Path;

/// One hook: run once, with the store's directory.
pub(super) type Hook = Box<dyn FnOnce(&Path)>;

// The directory store takes the hooks and the outcomes out of these.
thread_local! {
    pub(super) static PUBLISH_HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    pub(super) static COLLISION_HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    pub(super) static DIRECTORY_SYNC_FAULTS: RefCell<VecDeque<Option<io::ErrorKind>>> =
        const { RefCell::new(VecDeque::new()) };
}

/// Run `hook` once, in this thread's next write, after the value is in its flushed
/// temporary file and before that file is renamed to the item. It is given the store's
/// directory.
pub fn before_publish(hook: impl FnOnce(&Path) + 'static) {
    PUBLISH_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

/// Forget the [`before_publish`] hook. True if it was still waiting to run.
pub fn take_before_publish() -> bool {
    PUBLISH_HOOK.with(|slot| slot.borrow_mut().take().is_some())
}

/// Run `hook` once, in this thread's next add that finds the item's name taken at the
/// rename, before the add looks at the entry that took it. It is given the store's
/// directory.
pub fn after_collision(hook: impl FnOnce(&Path) + 'static) {
    COLLISION_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

/// Decide this thread's next directory flushes, in order: `Some(kind)` fails that flush
/// with `kind` without flushing, `None` lets it happen. Flushes beyond the list happen.
pub fn fail_directory_syncs(outcomes: impl IntoIterator<Item = Option<io::ErrorKind>>) {
    DIRECTORY_SYNC_FAULTS.with(|faults| *faults.borrow_mut() = outcomes.into_iter().collect());
}

/// The outcomes of [`fail_directory_syncs`] that no flush has used yet; the list is
/// emptied.
pub fn take_directory_sync_faults() -> Vec<Option<io::ErrorKind>> {
    DIRECTORY_SYNC_FAULTS.with(|faults| faults.borrow_mut().drain(..).collect())
}
