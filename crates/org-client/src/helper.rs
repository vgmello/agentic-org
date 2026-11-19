//! Own a helper and its completion signal without changing normal detachment on drop.
use std::io;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;
use std::thread::{self, JoinHandle};

pub(super) struct Helper {
    thread: JoinHandle<()>,
    // Retained even in builds that never explicitly join helpers; the underscore
    // avoids a dead-code warning when testkit and unit tests are both disabled.
    _exited: Mutex<Receiver<()>>,
}

struct Signal(Sender<()>);
impl Drop for Signal {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

impl Helper {
    pub(super) fn spawn(name: &str, work: impl FnOnce() + Send + 'static) -> io::Result<Self> {
        let (finished, exited) = mpsc::channel();
        let thread = thread::Builder::new().name(name.into()).spawn(move || {
            let _signal = Signal(finished);
            work();
        })?;
        Ok(Self {
            thread,
            _exited: Mutex::new(exited),
        })
    }

    pub(super) fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }

    pub(super) fn join(self) -> thread::Result<()> {
        self.thread.join()
    }

    #[cfg(any(test, feature = "testkit"))]
    pub(super) fn join_before(
        slot: &mut Option<Self>,
        deadline: std::time::Instant,
    ) -> io::Result<()> {
        let Some(helper) = slot.as_mut() else {
            return Ok(());
        };
        let name = helper.thread.thread().name().unwrap_or("helper").to_owned();
        if matches!(
            helper
                ._exited
                .get_mut()
                .unwrap_or_else(|p| p.into_inner())
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())),
            Err(mpsc::RecvTimeoutError::Timeout)
        ) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("{name} did not finish before deadline"),
            ));
        }
        slot.take()
            .expect("retained helper")
            .join()
            .map_err(|_| io::Error::other(format!("{name} panicked")))
    }
}
