//! A record family that needs no tables: it owns `local/offices/<office-id>/probe/`, its
//! projector writes no rows, and its state is read through `CoreRead::source_hash`. It
//! has committed-class and durable-class operations, so the store's own guarantees are
//! tested on every engine without a real family.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use crate::{
    change::{Ack, Actor, Change, DurablePath, Id, Operation, Refusal, Sha256},
    engine::{CoreRead, Engine, Projector},
    error::ProjectionError,
    sequences::Stream,
    writer::UnitOfWork,
};

/// Bytes that start with this are refused by the probe's projector.
pub const REFUSED: &[u8] = b"!refused";

#[derive(Clone)]
pub struct Probe {
    root: DurablePath,
    decisions: Arc<AtomicUsize>,
}

/// A boxed decision, as `Store::submit` takes it.
pub type Decision<E> = Box<dyn FnMut(&mut UnitOfWork<'_, E>) -> Result<Change, Refusal> + Send>;

impl Probe {
    pub fn new(office: &Id) -> Self {
        let root = DurablePath::parse(&format!("local/offices/{office}/probe"));
        Self {
            root: root.expect("an ID is one safe component"),
            decisions: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn projector<E: Engine>(&self) -> Arc<dyn Projector<E>> {
        Arc::new(self.clone())
    }

    pub fn path(&self, name: &str) -> DurablePath {
        self.root.join(name).expect("a valid probe name")
    }

    /// How many times any decision of this probe has run.
    pub fn decisions(&self) -> usize {
        self.decisions.load(Ordering::SeqCst)
    }

    /// An operation identity for `kind` on `name`; the fingerprint covers `bytes`.
    pub fn operation(&self, id: Id, kind: &'static str, name: &str, bytes: &[u8]) -> Operation {
        Operation {
            id,
            kind,
            actor: Actor::System,
            fingerprint: Sha256::of_parts(&[kind.as_bytes(), b"\0", name.as_bytes(), b"\0", bytes]),
        }
    }

    fn counted<E: Engine>(
        &self,
        mut decide: impl FnMut(&mut UnitOfWork<'_, E>) -> Result<Change, Refusal> + Send + 'static,
    ) -> Decision<E> {
        let decisions = Arc::clone(&self.decisions);
        Box::new(move |uow| {
            decisions.fetch_add(1, Ordering::SeqCst);
            decide(uow)
        })
    }

    /// Replaces `name` with `bytes` and takes one work position. The reply names it.
    pub fn put<E: Engine>(&self, name: &str, bytes: &[u8], ack: Ack) -> Decision<E> {
        let (path, bytes, name) = (self.path(name), bytes.to_vec(), name.to_owned());
        self.counted(move |uow: &mut UnitOfWork<'_, E>| {
            let position = uow.next(Stream::Work)?;
            let reply = format!("put {name} at work {position}");
            Ok(Change::new(ack)
                .put(path.clone(), bytes.clone())
                .reply(reply.into_bytes()))
        })
    }

    /// Creates the immutable `name`. One that exists is a recorded rejection, and the
    /// organization position the decision took is given back.
    pub fn create<E: Engine>(&self, name: &str, bytes: &[u8]) -> Decision<E> {
        let (path, bytes, name) = (self.path(name), bytes.to_vec(), name.to_owned());
        self.counted(move |uow: &mut UnitOfWork<'_, E>| {
            let generation = uow.next(Stream::Organization)?;
            if uow.view().source_hash(&path)?.is_some() {
                return Err(Refusal::rejected(format!("{name} exists").into_bytes()));
            }
            let reply = format!("created {name} at generation {generation}");
            Ok(Change::new(Ack::Durable)
                .create(path.clone(), bytes.clone())
                .reply(reply.into_bytes()))
        })
    }

    /// Removes `name`. A missing one is refused before admission.
    pub fn remove<E: Engine>(&self, name: &str) -> Decision<E> {
        let (path, name) = (self.path(name), name.to_owned());
        self.counted(move |uow: &mut UnitOfWork<'_, E>| {
            if uow.view().source_hash(&path)?.is_none() {
                return Err(Refusal::invalid(format!("no {name}")));
            }
            let reply = format!("removed {name}");
            Ok(Change::new(Ack::Committed)
                .remove(path.clone())
                .reply(reply.into_bytes()))
        })
    }

    /// Appends `suffix` to the current bytes of `name`, read through the unit of work.
    pub fn append<E: Engine>(&self, name: &str, suffix: &[u8]) -> Decision<E> {
        let (path, suffix) = (self.path(name), suffix.to_vec());
        self.counted(move |uow: &mut UnitOfWork<'_, E>| {
            let mut bytes = uow.source(&path)?.map(|b| b.to_vec()).unwrap_or_default();
            bytes.extend_from_slice(&suffix);
            Ok(Change::new(Ack::Committed)
                .put(path.clone(), bytes.clone())
                .reply(bytes))
        })
    }
}

impl<E: Engine> Projector<E> for Probe {
    fn owns(&self, path: &DurablePath) -> bool {
        path.is_under(&self.root) && *path != self.root
    }

    fn apply(
        &self,
        _tx: &mut E::Tx,
        path: &DurablePath,
        bytes: Option<&[u8]>,
    ) -> Result<(), ProjectionError> {
        if bytes.is_some_and(|bytes| bytes.starts_with(REFUSED)) {
            return Err(ProjectionError::skippable(
                path,
                "the probe refuses these bytes",
            ));
        }
        Ok(())
    }
}
