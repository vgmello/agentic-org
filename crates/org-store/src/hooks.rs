//! Where the writer, the flusher and a rebuild can be made to die, for crash tests.

/// Where the writer and the flusher can be made to die, for crash tests. Production
/// composes [`NoHooks`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Site {
    DuringDecide,
    DuringApply,
    AfterAppend,
    AfterCommit,
    AfterCommittedReply,
    BeforeMark,
    DuringRebuild,
}

pub(crate) trait Hooks: Send + Sync {
    /// `true`: the process dies here. The caller does nothing more.
    fn crash_at(&self, site: Site) -> bool;
}

pub(crate) struct NoHooks;

impl Hooks for NoHooks {
    fn crash_at(&self, _site: Site) -> bool {
        false
    }
}
