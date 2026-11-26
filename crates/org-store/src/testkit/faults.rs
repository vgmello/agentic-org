//! A disk that fails, writes short, holds a flush back or dies on request, and records
//! every call. It wraps the real disk: what it lets through really happens.

use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use super::HELD_BOUND;
use crate::{
    disk::{Disk, DiskFile, RealDisk},
    error::FilesError,
    files::Files,
    hooks::{Hooks, Site},
};

/// Where a flush error is injected.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlushSite {
    Segment,
    File,
    Directory,
}

/// One value per row of the design's crash table (office-store.md, section 4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrashPoint {
    /// Killed after a decision ran, before its entry is framed.
    DuringDecide,
    /// Killed after an operation is applied to the projection, before the append.
    DuringApply,
    /// Killed mid-write: only this many bytes of the batch reach the segment.
    MidAppend(usize),
    /// Killed after the append, before the projection commit.
    AfterAppend,
    /// Killed after the projection commit, before any reply.
    AfterCommit,
    /// Killed after the committed-class replies. With [`Faults::power_cut`], the row
    /// "after a committed reply, before the flush".
    AfterCommittedReply,
    /// Killed during materialization: a temporary file is flushed, nothing renamed.
    MaterializeTempFlushed,
    /// Killed during materialization: a file is renamed into place, the round unfinished.
    MaterializeRenamed,
    /// Killed after the files of a round are written, before the materialized mark.
    BeforeMark,
    /// Killed while a rebuild is building the new projection.
    DuringRebuild,
    /// A flush returns an error. The process lives and must stop accepting.
    FlushError(FlushSite),
    /// The append returns an error. The batch fails alone.
    AppendError,
    /// The append writes fewer bytes than the batch. The batch fails alone.
    ShortAppend,
    /// A short append whose truncation also fails. The store stops.
    TruncateError,
}

impl CrashPoint {
    /// Every point, with a representative cut for the mid-append one.
    pub const ALL: [CrashPoint; 16] = [
        CrashPoint::DuringDecide,
        CrashPoint::DuringApply,
        CrashPoint::MidAppend(9),
        CrashPoint::AfterAppend,
        CrashPoint::AfterCommit,
        CrashPoint::AfterCommittedReply,
        CrashPoint::MaterializeTempFlushed,
        CrashPoint::MaterializeRenamed,
        CrashPoint::BeforeMark,
        CrashPoint::DuringRebuild,
        CrashPoint::FlushError(FlushSite::Segment),
        CrashPoint::FlushError(FlushSite::File),
        CrashPoint::FlushError(FlushSite::Directory),
        CrashPoint::AppendError,
        CrashPoint::ShortAppend,
        CrashPoint::TruncateError,
    ];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiskOp {
    Open,
    Create,
    Write,
    Flush,
    Truncate,
    Rename,
    Link,
    Unlink,
    CreateDir,
    SyncDir,
}

/// What a call acts on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// A journal segment.
    Segment,
    /// Any other file.
    File,
    Directory,
}

fn target(op: DiskOp, path: &Path) -> Target {
    if matches!(op, DiskOp::SyncDir | DiskOp::CreateDir) {
        Target::Directory
    } else if path.extension().is_some_and(|e| e == "log")
        && path.parent().is_some_and(|p| p.ends_with("segments"))
    {
        Target::Segment
    } else {
        Target::File
    }
}

#[derive(Clone, Copy, Debug)]
enum Action {
    Fail,
    /// Write only this many bytes of the call.
    Short(usize),
    /// Write this many bytes, then die.
    CrashAfterWriting(usize),
    CrashBefore,
    CrashAfter,
}

#[derive(Debug)]
struct Rule {
    /// `None` matches every call.
    on: Option<(DiskOp, Target)>,
    /// Matching calls still to let through.
    skip: usize,
    action: Action,
    repeat: bool,
}

/// A change to a directory's entries that no flush of that directory has covered yet.
#[derive(Debug)]
enum EntryChange {
    /// A name that was not there before: a created file or a new link.
    Created(PathBuf),
    /// A name taken away, and what it held.
    Unlinked(PathBuf, Vec<u8>),
    /// A rename, and what the new name held before it, if anything.
    Renamed {
        from: PathBuf,
        to: PathBuf,
        replaced: Option<Vec<u8>>,
    },
}

impl EntryChange {
    fn directory(&self) -> Option<&Path> {
        match self {
            Self::Created(path) | Self::Unlinked(path, _) | Self::Renamed { to: path, .. } => {
                path.parent()
            }
        }
    }

    /// Puts the directory back as it was before this change.
    fn undo(&self) -> io::Result<()> {
        match self {
            Self::Created(path) => match std::fs::remove_file(path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            },
            Self::Unlinked(path, bytes) => std::fs::write(path, bytes),
            Self::Renamed { from, to, replaced } => {
                std::fs::rename(to, from)?;
                match replaced {
                    Some(bytes) => std::fs::write(to, bytes),
                    None => Ok(()),
                }
            }
        }
    }
}

#[derive(Default)]
struct State {
    dead: bool,
    fired: bool,
    rules: Vec<Rule>,
    log: Vec<(DiskOp, PathBuf)>,
    /// Per appended file: its length now, and at its last full flush.
    lengths: BTreeMap<PathBuf, (u64, u64)>,
    /// Entry changes since the last flush of their directory, oldest first.
    entries: Vec<EntryChange>,
    hold_flushes: bool,
    blocked_flushes: usize,
    /// Sites in the writer and flusher to die at, each after this many passes.
    sites: Vec<(Site, usize)>,
    /// What a real process does when a site kills it.
    on_site_crash: Option<Arc<dyn Fn() + Send + Sync>>,
}

#[derive(Default)]
pub struct Faults {
    state: Mutex<State>,
    changed: Condvar,
}

enum Verdict {
    Proceed,
    Short(usize),
    /// Do the call, then die.
    ProceedThenDie,
    /// Write this many bytes, then die.
    WriteThenDie(usize),
}

impl Faults {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub(crate) fn disk(self: &Arc<Self>) -> Arc<dyn Disk> {
        Arc::new(FaultDisk(Arc::clone(self)))
    }

    /// The files under `root`, written through this disk.
    pub fn files(self: &Arc<Self>, root: &Path) -> Result<Files, FilesError> {
        Files::new(root, self.disk())
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn rule(&self, on: Option<(DiskOp, Target)>, nth: usize, action: Action, repeat: bool) {
        assert!(nth >= 1, "calls are counted from one");
        self.lock().rules.push(Rule {
            on,
            skip: nth - 1,
            action,
            repeat,
        });
    }

    /// Arms `point`: it acts at its `nth` opportunity from now (counted from one).
    pub fn arm(&self, point: CrashPoint, nth: usize) {
        let site = |site| {
            assert!(nth >= 1, "opportunities are counted from one");
            self.lock().sites.push((site, nth - 1));
        };
        match point {
            CrashPoint::DuringDecide => site(Site::DuringDecide),
            CrashPoint::DuringApply => site(Site::DuringApply),
            CrashPoint::AfterAppend => site(Site::AfterAppend),
            CrashPoint::AfterCommit => site(Site::AfterCommit),
            CrashPoint::AfterCommittedReply => site(Site::AfterCommittedReply),
            CrashPoint::BeforeMark => site(Site::BeforeMark),
            CrashPoint::DuringRebuild => site(Site::DuringRebuild),
            CrashPoint::MidAppend(cut) => self.crash_mid_write(Target::Segment, nth, cut),
            CrashPoint::MaterializeTempFlushed => {
                self.crash(DiskOp::Flush, Target::File, nth, true)
            }
            CrashPoint::MaterializeRenamed => self.crash(DiskOp::Rename, Target::File, nth, true),
            CrashPoint::FlushError(FlushSite::Segment) => {
                self.fail(DiskOp::Flush, Target::Segment, nth)
            }
            CrashPoint::FlushError(FlushSite::File) => self.fail(DiskOp::Flush, Target::File, nth),
            CrashPoint::FlushError(FlushSite::Directory) => {
                self.fail(DiskOp::SyncDir, Target::Directory, nth)
            }
            CrashPoint::AppendError => self.fail(DiskOp::Write, Target::Segment, nth),
            CrashPoint::ShortAppend => self.short_write(Target::Segment, nth, 7),
            CrashPoint::TruncateError => {
                self.short_write(Target::Segment, nth, 7);
                self.fail(DiskOp::Truncate, Target::Segment, 1);
            }
        }
    }

    /// The `nth` matching call from now fails with an error.
    pub fn fail(&self, op: DiskOp, target: Target, nth: usize) {
        self.rule(Some((op, target)), nth, Action::Fail, false);
    }

    /// The `nth` matching write from now writes only `keep` bytes and reports that.
    pub fn short_write(&self, target: Target, nth: usize, keep: usize) {
        self.rule(
            Some((DiskOp::Write, target)),
            nth,
            Action::Short(keep),
            false,
        );
    }

    /// The `nth` matching write from now writes `keep` bytes; then the process is dead.
    pub fn crash_mid_write(&self, target: Target, nth: usize, keep: usize) {
        self.rule(
            Some((DiskOp::Write, target)),
            nth,
            Action::CrashAfterWriting(keep),
            false,
        );
    }

    /// The process dies just before (`after` false) or just after the `nth` matching call.
    pub fn crash(&self, op: DiskOp, target: Target, nth: usize, after: bool) {
        let action = if after {
            Action::CrashAfter
        } else {
            Action::CrashBefore
        };
        self.rule(Some((op, target)), nth, action, false);
    }

    /// The `nth` call of any kind from now fails.
    pub fn fail_call(&self, nth: usize) {
        self.rule(None, nth, Action::Fail, false);
    }

    /// The process dies just before the `nth` call of any kind from now.
    pub fn crash_before_call(&self, nth: usize) {
        self.rule(None, nth, Action::CrashBefore, false);
    }

    /// Every call from now fails.
    pub fn fail_everything(&self) {
        self.rule(None, 1, Action::Fail, true);
    }

    /// Runs `act` on the thread that reaches an armed site, at the site, before anything
    /// else happens there. A subprocess test ends its own process in it, so a real kill
    /// lands exactly where the simulated one does.
    pub fn on_site_crash(&self, act: impl Fn() + Send + Sync + 'static) {
        self.lock().on_site_crash = Some(Arc::new(act));
    }

    /// The process is dead: no call reaches the disk any more.
    pub fn kill(&self) {
        self.lock().dead = true;
        self.changed.notify_all();
    }

    pub fn dead(&self) -> bool {
        self.lock().dead
    }

    /// Whether any armed fault has acted.
    pub fn fired(&self) -> bool {
        self.lock().fired
    }

    /// A new process on the same disk: faults and the log are forgotten, file lengths kept.
    pub fn revive(&self) {
        let mut state = self.lock();
        state.dead = false;
        state.fired = false;
        state.rules.clear();
        state.sites.clear();
        state.log.clear();
        state.hold_flushes = false;
    }

    pub fn log(&self) -> Vec<(DiskOp, PathBuf)> {
        self.lock().log.clone()
    }

    pub fn clear_log(&self) {
        self.lock().log.clear();
    }

    /// The paths of every call of `op` that reached the disk or failed, in order.
    pub fn paths(&self, op: DiskOp) -> Vec<PathBuf> {
        let state = self.lock();
        let calls = state.log.iter().filter(|(called, _)| *called == op);
        calls.map(|(_, path)| path.clone()).collect()
    }

    /// Segment flushes block until [`Faults::release_flushes`].
    pub fn hold_flushes(&self) {
        self.lock().hold_flushes = true;
    }

    /// Returns once a segment flush is waiting behind the hold. Panics when none has
    /// arrived within [`HELD_BOUND`]: the thread meant to flush may have died before
    /// reaching the hold, and the test must then fail instead of waiting forever.
    pub fn wait_for_held_flush(&self) {
        self.wait_for_held_flush_within(HELD_BOUND);
    }

    fn wait_for_held_flush_within(&self, bound: Duration) {
        let deadline = Instant::now() + bound;
        let mut state = self.lock();
        while state.blocked_flushes == 0 {
            assert!(state.hold_flushes, "no hold is set");
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                // Lift the hold first: a store dropped while this panic unwinds joins its
                // threads, and one that reached the hold meanwhile must not wait forever.
                state.hold_flushes = false;
                self.changed.notify_all();
                drop(state);
                panic!(
                    "no segment flush reached the hold within {bound:?}: \
                 the thread meant to flush may have died before it"
                );
            }
            let (next, _) = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(|e| e.into_inner());
            state = next;
        }
    }

    pub fn release_flushes(&self) {
        self.lock().hold_flushes = false;
        self.changed.notify_all();
    }

    /// A power cut: every appended file loses what was written after its last full flush.
    /// Directory entries survive, flushed or not. The process is dead afterwards.
    pub fn power_cut(&self) -> io::Result<()> {
        self.cut(false)
    }

    /// A power cut that also takes every directory entry changed since the last flush of
    /// its directory: a created file or a new link is gone, an unlinked file is back, a
    /// rename is undone. A drive may keep any of them; [`Faults::power_cut`] is the cut
    /// that keeps them all. Only files are modelled, not directories.
    pub fn power_cut_losing_entries(&self) -> io::Result<()> {
        self.cut(true)
    }

    fn cut(&self, lose_entries: bool) -> io::Result<()> {
        let mut state = self.lock();
        state.dead = true;
        let changes = std::mem::take(&mut state.entries);
        if lose_entries {
            for change in changes.iter().rev() {
                change.undo()?;
            }
        }
        state.lengths.retain(|path, _| path.exists());
        for (path, (length, flushed)) in &mut state.lengths {
            if path.exists() {
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(path)?
                    .set_len(*flushed)?;
            }
            *length = *flushed;
        }
        drop(state);
        self.changed.notify_all();
        Ok(())
    }

    fn before(&self, op: DiskOp, path: &Path) -> io::Result<Verdict> {
        let mut state = self.lock();
        if state.dead {
            return Err(io::Error::other("the process is dead"));
        }
        state.log.push((op, path.to_path_buf()));
        let here = (op, target(op, path));
        let mut verdict = Verdict::Proceed;
        let mut index = 0;
        while index < state.rules.len() {
            let rule = &mut state.rules[index];
            if rule.on.is_some_and(|on| on != here) {
                index += 1;
                continue;
            }
            if rule.skip > 0 {
                rule.skip -= 1;
                index += 1;
                continue;
            }
            let action = rule.action;
            if rule.repeat {
                index += 1;
            } else {
                state.rules.remove(index);
            }
            state.fired = true;
            match action {
                Action::Fail => return Err(io::Error::other("injected disk failure")),
                Action::CrashBefore => {
                    state.dead = true;
                    return Err(io::Error::other("the process died here"));
                }
                Action::Short(keep) => verdict = Verdict::Short(keep),
                Action::CrashAfterWriting(keep) => verdict = Verdict::WriteThenDie(keep),
                Action::CrashAfter => verdict = Verdict::ProceedThenDie,
            }
        }
        Ok(verdict)
    }

    fn run(
        &self,
        op: DiskOp,
        path: &Path,
        call: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        match self.before(op, path)? {
            Verdict::ProceedThenDie => {
                call()?;
                self.kill();
                Ok(())
            }
            _ => call(),
        }
    }
}

struct FaultDisk(Arc<Faults>);

struct FaultFile {
    faults: Arc<Faults>,
    path: PathBuf,
    inner: Arc<dyn DiskFile>,
    /// Appended files are tracked for the power cut.
    appended: bool,
}

impl Disk for FaultDisk {
    fn open_append(&self, path: &Path) -> io::Result<Arc<dyn DiskFile>> {
        let mut opened = None;
        let existed = path.symlink_metadata().is_ok();
        self.0.run(DiskOp::Open, path, || {
            opened = Some(RealDisk.open_append(path)?);
            Ok(())
        })?;
        let length = std::fs::metadata(path)?.len();
        let mut state = self.0.lock();
        if !existed {
            state.entries.push(EntryChange::Created(path.to_path_buf()));
        }
        // What an earlier process left is taken as flushed: the journal flushes at opening.
        state
            .lengths
            .entry(path.to_path_buf())
            .or_insert((length, length));
        drop(state);
        Ok(Arc::new(FaultFile {
            faults: Arc::clone(&self.0),
            path: path.to_path_buf(),
            inner: opened.expect("opened above"),
            appended: true,
        }))
    }

    fn create_new(&self, path: &Path) -> io::Result<Arc<dyn DiskFile>> {
        let mut created = None;
        self.0.run(DiskOp::Create, path, || {
            created = Some(RealDisk.create_new(path)?);
            Ok(())
        })?;
        let created_entry = EntryChange::Created(path.to_path_buf());
        self.0.lock().entries.push(created_entry);
        Ok(Arc::new(FaultFile {
            faults: Arc::clone(&self.0),
            path: path.to_path_buf(),
            inner: created.expect("created above"),
            appended: false,
        }))
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let replaced = std::fs::read(to).ok();
        self.0
            .run(DiskOp::Rename, to, || RealDisk.rename(from, to))?;
        self.0.lock().entries.push(EntryChange::Renamed {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            replaced,
        });
        Ok(())
    }

    fn link(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.0.run(DiskOp::Link, to, || RealDisk.link(from, to))?;
        let linked = EntryChange::Created(to.to_path_buf());
        self.0.lock().entries.push(linked);
        Ok(())
    }

    fn unlink(&self, path: &Path) -> io::Result<()> {
        let held = std::fs::read(path).ok();
        self.0.run(DiskOp::Unlink, path, || RealDisk.unlink(path))?;
        let mut state = self.0.lock();
        let tracked = state.lengths.remove(path);
        if let Some(mut held) = held {
            // What comes back if the unlink is lost is what the drive had: an appended
            // file's bytes after its last full flush go with the same cut.
            if let Some((_, flushed)) = tracked {
                held.truncate(usize::try_from(flushed).unwrap_or(usize::MAX));
            }
            state
                .entries
                .push(EntryChange::Unlinked(path.to_path_buf(), held));
        }
        Ok(())
    }

    fn create_dir(&self, path: &Path) -> io::Result<()> {
        self.0
            .run(DiskOp::CreateDir, path, || RealDisk.create_dir(path))
    }

    fn sync_dir(&self, path: &Path) -> io::Result<()> {
        self.0
            .run(DiskOp::SyncDir, path, || RealDisk.sync_dir(path))?;
        let mut state = self.0.lock();
        state
            .entries
            .retain(|change| change.directory() != Some(path));
        Ok(())
    }
}

impl FaultFile {
    fn wrote(&self, n: usize) {
        if self.appended {
            if let Some((length, _)) = self.faults.lock().lengths.get_mut(&self.path) {
                *length += n as u64;
            }
        }
    }
}

impl DiskFile for FaultFile {
    fn write(&self, bytes: &[u8]) -> io::Result<usize> {
        let (keep, die) = match self.faults.before(DiskOp::Write, &self.path)? {
            Verdict::Proceed => (bytes.len(), false),
            Verdict::ProceedThenDie => (bytes.len(), true),
            Verdict::Short(keep) => (keep.min(bytes.len()), false),
            Verdict::WriteThenDie(keep) => (keep.min(bytes.len()), true),
        };
        let mut written = 0;
        while written < keep {
            match self.inner.write(&bytes[written..keep])? {
                0 => break,
                n => written += n,
            }
        }
        self.wrote(written);
        if die {
            self.faults.kill();
            return Err(io::Error::other("the process died here"));
        }
        Ok(written)
    }

    fn flush_full(&self) -> io::Result<()> {
        let segment = target(DiskOp::Flush, &self.path) == Target::Segment;
        if segment {
            let mut state = self.faults.lock();
            if state.hold_flushes && !state.dead {
                state.blocked_flushes += 1;
                self.faults.changed.notify_all();
                while state.hold_flushes && !state.dead {
                    state = self
                        .faults
                        .changed
                        .wait(state)
                        .unwrap_or_else(|e| e.into_inner());
                }
                state.blocked_flushes -= 1;
            }
        }
        // The length this flush is known to cover: bytes written while it runs are not.
        let covered = self.faults.lock().lengths.get(&self.path).map(|l| l.0);
        self.faults
            .run(DiskOp::Flush, &self.path, || self.inner.flush_full())?;
        if let (Some(covered), Some(entry)) =
            (covered, self.faults.lock().lengths.get_mut(&self.path))
        {
            entry.1 = entry.1.max(covered).min(entry.0);
        }
        Ok(())
    }

    fn truncate(&self, len: u64) -> io::Result<()> {
        self.faults
            .run(DiskOp::Truncate, &self.path, || self.inner.truncate(len))?;
        if let Some(entry) = self.faults.lock().lengths.get_mut(&self.path) {
            entry.0 = len;
            entry.1 = entry.1.min(len);
        }
        Ok(())
    }
}

impl Hooks for Faults {
    fn crash_at(&self, site: Site) -> bool {
        let mut state = self.lock();
        if state.dead {
            return true;
        }
        let Some(index) = state.sites.iter().position(|(armed, _)| *armed == site) else {
            return false;
        };
        if state.sites[index].1 > 0 {
            state.sites[index].1 -= 1;
            return false;
        }
        state.sites.remove(index);
        state.fired = true;
        state.dead = true;
        let act = state.on_site_crash.clone();
        drop(state);
        self.changed.notify_all();
        if let Some(act) = act {
            act();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An unlink the cut loses gives the file back as the drive had it: without the
    /// bytes appended after its last full flush.
    #[test]
    fn an_unlinked_file_restored_by_a_cut_has_only_its_flushed_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("appended.log");
        let faults = Faults::new();
        let disk = faults.disk();
        let file = disk.open_append(&path).unwrap();
        crate::disk::write_all(&*file, b"flushed").unwrap();
        file.flush_full().unwrap();
        // Its name is on the drive: the cut cannot lose the file itself.
        disk.sync_dir(path.parent().unwrap()).unwrap();
        crate::disk::write_all(&*file, b" and not").unwrap();
        disk.unlink(&path).unwrap();
        assert!(!path.exists());

        faults.power_cut_losing_entries().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"flushed");
    }

    /// A hold that no flush reaches (its thread died first) fails the wait, not the run.
    #[test]
    #[should_panic(expected = "no segment flush reached the hold within")]
    fn waiting_for_a_held_flush_that_never_arrives_fails_at_its_bound() {
        let faults = Faults::new();
        faults.hold_flushes();
        faults.wait_for_held_flush_within(Duration::from_millis(50));
    }

    /// The failed wait lifts the hold, so a store dropped while the panic unwinds never
    /// joins a flusher that reached the hold in the meantime.
    #[test]
    fn a_failed_wait_lifts_the_hold() {
        let faults = Faults::new();
        faults.hold_flushes();
        let waited = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            faults.wait_for_held_flush_within(Duration::from_millis(50))
        }));
        assert!(waited.is_err());
        assert!(!faults.lock().hold_flushes);
    }

    #[test]
    fn a_flush_reaching_the_hold_ends_the_wait() {
        let dir = tempfile::tempdir().unwrap();
        let segments = dir.path().canonicalize().unwrap().join("segments");
        std::fs::create_dir(&segments).unwrap();
        let path = segments.join("00000001.log");
        let faults = Faults::new();
        faults.hold_flushes();
        let flusher = {
            let faults = Arc::clone(&faults);
            std::thread::spawn(move || {
                let file = faults.disk().open_append(&path).unwrap();
                file.flush_full()
            })
        };
        faults.wait_for_held_flush_within(HELD_BOUND);
        faults.release_flushes();
        flusher.join().unwrap().unwrap();
    }
}
