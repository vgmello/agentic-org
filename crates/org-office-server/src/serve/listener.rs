//! The socket listener: binding the path, the accept loop, and the revocation watcher.

use std::collections::HashMap;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use super::binding::{lookup, lookup_binding, Lookup};
use super::connection::serve_connection;
use super::shared::Shared;
use super::MAX_CONNECTIONS;

/// Binds `path` (owner-only), replacing a stale socket file a crashed server left: the caller
/// holds the office lock, so no other server of this office is alive.
pub fn bind(path: &Path) -> std::io::Result<UnixListener> {
    match std::fs::symlink_metadata(path) {
        // Only a stale socket (a crashed server's) is replaced; anything else is not ours.
        Ok(metadata) if metadata.file_type().is_socket() => {
            // A socket someone still answers on is a live server, not a stale file.
            if UnixStream::connect(path).is_ok() {
                return Err(std::io::Error::other("a server is already on this socket"));
            }
            std::fs::remove_file(path)?
        }
        Ok(_) => return Err(std::io::Error::other("the socket path is not a socket")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

/// Accepts connections until `stop` says to quit, then closes every connection and waits for
/// its thread, so nothing is still reading the office when the caller closes it.
pub fn run(listener: &UnixListener, shared: &Arc<Shared>, stop: &dyn Fn() -> bool) {
    run_for_uid(listener, shared, stop, org_socket::effective_uid());
}

pub(super) fn run_for_uid(
    listener: &UnixListener,
    shared: &Arc<Shared>,
    stop: &dyn Fn() -> bool,
    expected_uid: u32,
) {
    let stopping = Arc::new(std::sync::atomic::AtomicBool::new(false));
    // Ends the held calls of a connection whose member stops being bindable, at once.
    let watching = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let watcher = {
        let (shared, watching) = (Arc::clone(shared), Arc::clone(&watching));
        std::thread::Builder::new()
            .name("org-office-watch".into())
            .spawn(move || watch(&shared, &watching))
            .ok()
    };
    let live = Arc::new(AtomicUsize::new(0));
    // A handle on each connection, to close it at shutdown.
    let open: Arc<Mutex<HashMap<u64, UnixStream>>> = Arc::default();
    let mut threads: Vec<JoinHandle<()>> = Vec::new();
    let mut next_id = 0u64;
    while !stop() {
        match listener.accept() {
            Ok((stream, _)) => {
                if !org_socket::peer_allowed(&stream, expected_uid) {
                    continue; // no response, registration or connection slot for a foreign UID
                }
                if live.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    live.fetch_sub(1, Ordering::SeqCst);
                    continue; // dropped: closed at once
                }
                next_id += 1;
                let id = next_id;
                // A connection that can't be registered can't be closed at shutdown: refuse it.
                let Ok(handle) = stream.try_clone() else {
                    live.fetch_sub(1, Ordering::SeqCst);
                    continue;
                };
                open.lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(id, handle);
                let shared = Arc::clone(shared);
                let stopping = Arc::clone(&stopping);
                let slot = Slot(Arc::clone(&live));
                let registered = Registered {
                    registry: Arc::clone(&open),
                    id,
                };
                // If the thread cannot start, the closure (and so the slot and the registry
                // entry) is dropped with the error: both are given back, the connection closes.
                if let Ok(thread) = std::thread::Builder::new()
                    .name("org-office-conn".into())
                    .spawn(move || {
                        // Both given back however the thread ends, panics included: the slot,
                        // and the handle that would otherwise keep the socket open.
                        let _slot = slot;
                        let _registered = registered;
                        let _ = stream.set_nonblocking(false);
                        serve_connection(stream, &shared, id, &stopping);
                    })
                {
                    threads.retain(|t| !t.is_finished());
                    threads.push(thread);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            // A connection that died before accept: carry on.
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    // The tick also observes stop if a platform shutdown misses its reader wakeup.
    stopping.store(true, Ordering::SeqCst);
    shared.stopping.store(true, Ordering::SeqCst);
    // Stop: close every connection, then wait for the threads.
    for stream in open.lock().unwrap_or_else(|p| p.into_inner()).values() {
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }
    for thread in threads {
        let _ = thread.join();
    }
    watching.store(false, Ordering::SeqCst);
    if let Some(watcher) = watcher {
        let _ = watcher.join();
    }
}

/// Whenever the organization has changed (and the first time), looks at each member-bound
/// connection's member: one that is no longer bindable (offboarded, made office-run, removed) has
/// its held `msg.wait` calls ended with `not_bound` and its subscription ended, without waiting
/// for the connection's next call. An idle revoked connection closes at its next reader tick;
/// a call that arrives first receives `not_bound`, then the connection closes. A lookup that
/// fails ends nothing. With no subscription feed, check the directory's change token each tick.
pub(super) fn watch(shared: &Shared, watching: &std::sync::atomic::AtomicBool) {
    let feed = shared.ports.feed.as_deref();
    let mut checked = (None, None);
    while watching.load(Ordering::SeqCst) {
        // Sample before looking so a change during the pass gets another pass.
        let token = feed.map(|feed| feed.token());
        let position = match feed {
            Some(feed) => (feed.head().ok(), None),
            None => (
                None,
                shared.directory_change_token.as_ref().map(|token| token()),
            ),
        };
        if position == (None, None) || position != checked {
            // A panic in a lookup must not end the watching for good.
            let pass = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| revoke(shared)));
            match pass {
                Ok(false) => checked = position,
                // A lookup could not answer: look again on the next tick.
                Ok(true) => {}
                Err(_) => {
                    eprintln!("agentic-org-office: the member watcher failed once; it carries on");
                }
            }
        }
        let sleep = match (&shared.inbox, &shared.hint, feed) {
            (Some(hq), Some(hint), Some(feed)) => hint
                .poll(hq, feed, &shared.ports.live, std::time::Instant::now)
                .min(shared.timing.idle),
            _ => shared.timing.idle,
        };
        match (feed, token) {
            (Some(feed), Some(token)) => {
                feed.wait(token, sleep);
            }
            _ => std::thread::sleep(sleep),
        }
    }
}

/// One pass of the watcher. `true` when a lookup could not answer, so the pass must be repeated.
fn revoke(shared: &Shared) -> bool {
    let mut undecided = false;
    let members: std::collections::HashSet<String> = shared
        .bound
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .values()
        .map(|(member, _)| member.clone())
        .collect();
    // One lookup per member, however many connections it has.
    for member in members {
        match lookup(shared, &member) {
            Lookup::Failed => {
                undecided = true;
                continue;
            }
            Lookup::Bindable(found) if found.id == member => continue,
            Lookup::Bindable(_) | Lookup::Not => {}
        }
        // Confirmed under the registry's lock, on the connections that STILL act as this member:
        // one that bound as someone else, or bound again after the member was restored, is left
        // alone.
        let bound = shared.bound.lock().unwrap_or_else(|p| p.into_inner());
        match lookup_binding(shared, &member) {
            Lookup::Failed => {
                undecided = true;
                continue;
            }
            Lookup::Bindable(_) => continue,
            Lookup::Not => {}
        }
        for (_, control) in bound.values().filter(|(m, _)| *m == member) {
            // `revoked` first: a wait that starts after the cancel below sees it.
            control.revoked.store(true, Ordering::SeqCst);
            control.generation.fetch_add(1, Ordering::SeqCst);
            control.held.cancel_all();
        }
    }
    undecided
}

/// A connection's entry in the registry; dropping it removes the entry (and so the extra handle
/// on the socket), even if the connection's thread panics.
struct Registered {
    registry: Arc<Mutex<HashMap<u64, UnixStream>>>,
    id: u64,
}

impl Drop for Registered {
    fn drop(&mut self) {
        self.registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.id);
    }
}

/// One of the connection slots; dropping it gives the slot back, even if the thread panics.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
