//! Opening a store: journal incarnation, replay, materialization and the store threads.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use super::consumers::load_consumer_watermarks;
use super::{Opening, StoreParts};
use crate::changes::Changes;
use crate::custody::{OfficeIdentity, WriterAuthority};
use crate::disk::Disk;
use crate::engine::{CoreRead, Cursor, Engine};
use crate::error::StoreError;
use crate::files::Files;
use crate::journal::{Journal, JournalPoint, Recovered};
use crate::rebuild::{materialize, rebuild_with, Projectors};
use crate::sequences::Sequences;
use crate::state::{Core, Flow};

pub(super) fn spawn_store_thread(
    name: &str,
    body: impl FnOnce() + Send + 'static,
) -> Result<JoinHandle<()>, StoreError> {
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(body)
        .map_err(|e| StoreError::Internal(format!("cannot start the {name} thread: {e}")))
}

pub(super) fn open_store_core<E: Engine>(
    parts: StoreParts<E>,
) -> Result<(Arc<Core<E>>, Sequences), StoreError> {
    let StoreParts {
        durable,
        identity,
        custody,
        engine,
        projectors,
        excluded_prefixes,
        clock,
        flush_interval,
        opening,
        limits,
        disk,
        hooks,
    } = parts;
    if limits.change_ring == 0 {
        return Err(StoreError::Invalid(
            "the change ring must keep at least one entry".into(),
        ));
    }
    let files = Files::new(&durable, Arc::clone(&disk))?;
    let consumer_watermarks = match &opening {
        Opening::Create(_) => std::collections::BTreeMap::new(),
        Opening::Existing => {
            load_consumer_watermarks(&crate::journal::journal_dir(&durable, &identity))?
        }
    };
    let min_consumer_wm = consumer_watermarks.values().copied().min();
    let (journal, recovered) = open_or_create_journal(
        opening,
        disk,
        &durable,
        &identity,
        &custody,
        min_consumer_wm,
    )?;
    journal.set_segment_bytes(limits.segment_bytes);
    let projectors =
        Projectors::new_with_exclusions(projectors, &identity.office_id, excluded_prefixes);
    materialize_with_consumer_cap(&files, &journal, &consumer_watermarks)?;

    let head = journal.head();
    let base = journal.base();
    let cursor = engine.snapshot()?.cursor()?;
    let on_chain = cursor_on_chain(cursor.as_ref(), &base, &head, &recovered);
    if !on_chain {
        rebuild_with(
            &files,
            &journal,
            engine.as_ref(),
            &projectors,
            hooks.as_ref(),
        )?;
    }
    let core = Core {
        engine,
        projectors,
        files,
        journal,
        custody,
        clock,
        hooks,
        limits,
        flush_interval,
        changes: Changes::new(head.feed, limits.change_ring),
        release_gate: Mutex::new(()),
        materialization_lock: Mutex::new(()),
        flow: Mutex::new(Flow {
            fatal: None,
            dead: false,
            closing: false,
            writer_done: false,
            sync_wanted: false,
            holding_back: false,
            queued: 0,
            flushed: head.ordinal,
            materialized: head.ordinal,
            confirmed: recovered.confirmed.clone(),
            waiters: Vec::new(),
            unflushed: VecDeque::new(),
            unflushed_bytes: 0,
            unmaterialized: VecDeque::new(),
            unmaterialized_bytes: 0,
            consumer_watermarks,
        }),
        wake: Condvar::new(),
    };
    replay_and_confirm(&core, cursor, on_chain, &head, &recovered)?;
    let sequences = Sequences {
        high_waters: head.high_waters,
        feed: head.feed,
    };
    Ok((Arc::new(core), sequences))
}

pub(super) fn open_or_create_journal(
    opening: Opening,
    disk: Arc<dyn Disk>,
    durable: &std::path::Path,
    identity: &OfficeIdentity,
    custody: &Arc<dyn WriterAuthority>,
    min_consumer_watermark: Option<u64>,
) -> Result<(Journal, Recovered), StoreError> {
    match opening {
        Opening::Create(facts) => {
            let confirmed = custody
                .checkpoint(identity)
                .map_err(|e| StoreError::Custody(e.0))?;
            let journal = Journal::create_on(disk, durable, identity, &facts)?;
            let recovered = Recovered {
                confirmed,
                entries: Vec::new(),
                truncated: None,
            };
            Ok((journal, recovered))
        }
        Opening::Existing => Ok(Journal::open_on(
            disk,
            durable,
            identity,
            custody.as_ref(),
            min_consumer_watermark,
        )?),
    }
}

pub(super) fn materialize_with_consumer_cap(
    files: &Files,
    journal: &Journal,
    consumer_watermarks: &std::collections::BTreeMap<String, u64>,
) -> Result<(), StoreError> {
    if let Some(through) = materialize(files, journal)? {
        let release_cap = consumer_watermarks
            .values()
            .copied()
            .min()
            .map_or(through, |min| through.min(min));
        journal.set_materialized_with_release_cap(through, release_cap)?;
    }
    Ok(())
}

pub(super) fn cursor_on_chain(
    cursor: Option<&Cursor>,
    base: &JournalPoint,
    head: &JournalPoint,
    recovered: &Recovered,
) -> bool {
    cursor.is_some_and(|cursor| {
        let held = if cursor.ordinal == base.ordinal {
            Some(base.sha256)
        } else {
            recovered
                .entries
                .iter()
                .find(|e| e.ordinal() == cursor.ordinal)
                .map(|e| e.sha256)
        };
        cursor.ordinal <= head.ordinal && held == Some(cursor.sha256)
    })
}

pub(super) fn replay_and_confirm<E: Engine>(
    core: &Core<E>,
    cursor: Option<Cursor>,
    on_chain: bool,
    head: &JournalPoint,
    recovered: &Recovered,
) -> Result<(), StoreError> {
    if let Some(cursor) = cursor.filter(|cursor| on_chain && cursor.ordinal < head.ordinal) {
        let after: Vec<_> = recovered
            .entries
            .iter()
            .filter(|e| e.ordinal() > cursor.ordinal)
            .cloned()
            .collect();
        core.replay(&after, &head.cursor())?;
    }
    let flushed = head.head();
    if recovered.confirmed.as_ref() != Some(&flushed) {
        core.custody
            .confirm(
                core.journal.identity(),
                recovered.confirmed.as_ref(),
                &flushed,
            )
            .map_err(|e| StoreError::Custody(e.0))?;
        core.flow().confirmed = Some(flushed);
    }
    Ok(())
}
