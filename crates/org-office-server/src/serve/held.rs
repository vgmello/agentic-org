//! Per-connection control state: held `msg.wait` calls, their places, and bound registrations.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use org_mailbox::CancelHandle;

use super::MAX_HELD;

/// What other threads may do to a connection: end its held calls and its subscription. The
/// watcher does it when the connection's member stops being bindable, without waiting for the
/// connection's next call.
#[derive(Default)]
pub(super) struct Control {
    pub(super) held: Held,
    /// Moves on whenever the connection's binding changes or the reader ends, which stops the
    /// events of the binding before it.
    pub(super) generation: AtomicU64,
    /// Set by the watcher when it has ended this connection's held calls and subscription
    /// because its member can no longer be bound: the connection thread refuses the next call
    /// with `not_bound`, then closes. If it remains idle, the next reader tick ends it and
    /// removes its registration.
    pub(super) revoked: std::sync::atomic::AtomicBool,
}

/// The member-bound connections, by connection number: who each acts as, and how to reach it.
pub(super) type Bound = Mutex<HashMap<u64, (String, Arc<Control>)>>;

/// Keeps `conn`'s entry in [`Bound`] equal to the member it acts as. Called whenever its binding
/// may have changed; removes the entry when the connection is not bound as a member, and when
/// the connection ends.
pub(super) struct Registration<'a> {
    pub(super) bound: &'a Bound,
    pub(super) conn: u64,
    pub(super) control: Arc<Control>,
    pub(super) member: Option<String>,
}

impl Registration<'_> {
    /// `true` when the connection is newly registered as `member`: it may have become unbindable
    /// before the watcher could see it, so the caller looks once.
    pub(super) fn sync(&mut self, member: Option<&str>) -> bool {
        if self.member.as_deref() == member {
            return false;
        }
        let mut bound = self.bound.lock().unwrap_or_else(|p| p.into_inner());
        // Under the lock the watcher acts under: a decision about the member this connection
        // had must not unbind the one it has now.
        match member {
            Some(_) if self.control.revoked.load(Ordering::SeqCst) => return false,
            Some(member) => {
                bound.insert(self.conn, (member.to_owned(), Arc::clone(&self.control)));
            }
            None => {
                // A lookup-failure unbind must not erase a watcher revocation that raced
                // the call's liveness check. The reader still needs to observe that flag.
                bound.remove(&self.conn);
            }
        }
        self.member = member.map(str::to_owned);
        member.is_some()
    }
}

impl Drop for Registration<'_> {
    fn drop(&mut self) {
        self.bound
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.conn);
    }
}

/// The held calls of a connection, so a re-bind or the end of the connection can end them.
#[derive(Default)]
pub(super) struct Held {
    pub(super) next: Mutex<(u64, HashMap<u64, CancelHandle>)>,
}

impl Held {
    /// Room for one more: its number and a way to cancel it, or `None` when `MAX_HELD` are held.
    pub(super) fn start(&self, cancel: CancelHandle) -> Option<u64> {
        let mut state = self.next.lock().unwrap_or_else(|p| p.into_inner());
        if state.1.len() >= MAX_HELD {
            return None;
        }
        state.0 += 1;
        let number = state.0;
        state.1.insert(number, cancel);
        Some(number)
    }

    pub(super) fn finish(&self, number: u64) {
        self.next
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .1
            .remove(&number);
    }

    /// Ends every held call (each answers `not_bound`; one that had already chosen a message may
    /// still answer with it, under its own id, before or after the bind's answer: the contract
    /// fixes no order) and gives their places back at once, so
    /// a client that binds again can wait again. The cancelled calls are already ending; their
    /// threads are bounded by the write deadline of the answer they still send.
    pub(super) fn cancel_all(&self) {
        let ended = std::mem::take(&mut self.next.lock().unwrap_or_else(|p| p.into_inner()).1);
        for handle in ended.values() {
            handle.cancel();
        }
    }
}

/// Ends the held calls when the connection's thread stops for any reason, a panic included:
/// `thread::scope` joins the held calls' threads first, and they would otherwise wait out their
/// timeouts.
pub(super) struct CancelOnDrop<'a>(pub(super) &'a Held);

impl Drop for CancelOnDrop<'_> {
    fn drop(&mut self) {
        self.0.cancel_all();
    }
}

/// A held call's place, given back however its thread ends (a panic included) and only after its
/// answer is written, so a client that does not read cannot make threads pile up past the cap.
pub(super) struct Place<'a>(pub(super) &'a Held, pub(super) u64);

impl Drop for Place<'_> {
    fn drop(&mut self) {
        self.0.finish(self.1);
    }
}
