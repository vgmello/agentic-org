//! The flusher: group flush, the host confirmation, durable replies, and the files.
//! A committed operation never waits for anything here. A durable one is answered only
//! after the flush that covers it and the host's confirmation of that flushed head.

use std::{sync::Arc, time::Instant};

use crate::{
    engine::Engine,
    error::{FilesError, JournalError, StoreError},
    hooks::Site,
    rebuild::materialize,
    state::Core,
};

pub(crate) fn run<E: Engine>(core: Arc<Core<E>>) {
    // A flusher that panics would leave every durable request, a sync and a close waiting
    // for good: the store stops instead, and they are answered with the reason.
    struct Panicked<'a, E: Engine>(&'a Core<E>);
    impl<E: Engine> Drop for Panicked<'_, E> {
        fn drop(&mut self) {
            if std::thread::panicking() {
                self.0
                    .stop(StoreError::Internal("the flusher panicked".into()));
            }
        }
    }
    let _panicked = Panicked(&core);
    let mut deadline: Option<Instant> = None;
    loop {
        {
            let mut flow = core.flow();
            loop {
                if flow.dead || flow.fatal.is_some() {
                    return;
                }
                let unflushed = !flow.unflushed.is_empty();
                let behind = !flow.unmaterialized.is_empty();
                let urgent = !flow.waiters.is_empty()
                    || flow.closing
                    || flow.sync_wanted
                    || flow.flush_now
                    || flow.holding_back;
                if (unflushed && urgent) || behind {
                    flow.flush_now = false;
                    break;
                }
                if !unflushed {
                    deadline = None;
                    // Nothing left to flush: an immediate-flush request is satisfied.
                    flow.flush_now = false;
                    if flow.sync_wanted {
                        flow.sync_wanted = false;
                        core.wake.notify_all();
                    }
                    if flow.closing && flow.writer_done {
                        return;
                    }
                    flow = core.wake.wait(flow).unwrap_or_else(|e| e.into_inner());
                    continue;
                }
                // Unflushed entries and nobody waiting: flush when the interval has passed.
                let due = *deadline.get_or_insert_with(|| Instant::now() + core.flush_interval);
                let now = Instant::now();
                if now >= due {
                    break;
                }
                let waited = core.wake.wait_timeout(flow, due - now);
                flow = waited.unwrap_or_else(|e| e.into_inner()).0;
            }
        }
        deadline = None;
        if let Err(fatal) = cycle(&core) {
            core.stop(fatal);
            return;
        }
    }
}

fn stopped_at(path: String, reason: String) -> StoreError {
    StoreError::Stopped {
        path: Some(path),
        reason,
    }
}

/// One round: flush, confirm, answer, write files, mark, engine upkeep, release. Any
/// error is fatal: a failed flush is never retried in this process, and a head the host
/// did not confirm answers no durable request (the caller stops the store, which fails
/// them).
fn cycle<E: Engine>(core: &Core<E>) -> Result<(), StoreError> {
    if core.flow().dead {
        return Ok(());
    }
    if core.journal.flushed() < core.journal.head().ordinal {
        let head = core.journal.flush().map_err(|e| match e {
            JournalError::Flush { path, message } => {
                stopped_at(path, format!("a journal flush failed: {message}"))
            }
            other => StoreError::Journal(other),
        })?;
        let confirmed = {
            let mut flow = core.flow();
            if flow.dead {
                return Ok(());
            }
            flow.flushed_through(head.ordinal);
            core.wake.notify_all();
            flow.confirmed.clone()
        };
        let head = head.head();
        if confirmed.as_ref() != Some(&head) {
            core.custody
                .confirm(core.journal.identity(), confirmed.as_ref(), &head)
                .map_err(|e| StoreError::Custody(e.0))?;
        }
        let mut flow = core.flow();
        if flow.dead {
            return Ok(());
        }
        flow.confirmed_through(head);
        core.wake.notify_all();
    }
    if core.flow().dead {
        return Ok(());
    }
    let _mat_guard = core
        .materialization_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let through = materialize(&core.files, &core.journal).map_err(|e| match e {
        StoreError::Files(FilesError::Foreign { path }) => stopped_at(
            path.to_string(),
            "the file holds bytes this office did not write".into(),
        ),
        StoreError::Files(error) => stopped_at(error.path(), error.to_string()),
        other => other,
    })?;
    let Some(through) = through else {
        return Ok(());
    };
    if core.hooks.crash_at(Site::BeforeMark) {
        core.die();
        return Ok(());
    }
    // Serialize materialization cap + segment release with consumer registration/advancement
    // (`ARCH-03`) so concurrent `register_consumer` / `advance_consumer` never races with release.
    let _release_gate = core.release_gate.lock().unwrap_or_else(|e| e.into_inner());
    let release_cap = core.flow().release_cap(through);
    core.journal
        .set_materialized_with_release_cap(through, release_cap)?;
    // The engine's upkeep, on this thread: the writer never waits for it.
    core.engine.after_round();
    let release_through = {
        let mut flow = core.flow();
        flow.materialized_through(through);
        core.wake.notify_all();
        flow.release_cap(through)
    };
    core.journal.release_through(release_through)?;
    Ok(())
}
