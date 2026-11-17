//! Fixed native workers with bounded waiting jobs and input bytes.
//! Constructed inside service start, after the verified host handoff.

use std::io;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use tokio::sync::oneshot;

type Job = Box<dyn FnOnce() + Send + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitError {
    QueueFull,
    BytesFull,
    Stopped,
}

/// Receiving a ticket means admission, not execution or completion. Dropping it
/// never cancels native work. HTTP timeout classification must retain that fact.
pub struct Ticket<T> {
    receiver: oneshot::Receiver<T>,
}

impl<T> Ticket<T> {
    pub async fn wait(self) -> Result<T, oneshot::error::RecvError> {
        self.receiver.await
    }
}

struct QueuedBytes {
    used: AtomicUsize,
    limit: usize,
}

struct Reservation {
    budget: Arc<QueuedBytes>,
    bytes: usize,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

/// Dedicated blocking calls, not Tokio's spawn_blocking pool. Work must use the
/// native client's finite deadlines: shutdown joins all admitted work. Only
/// application code submits typed native operations; no browser RPC name enters
/// this boundary. `input_bytes` accounts for all request-owned bytes in the job.
pub struct BlockingPool {
    sender: Option<SyncSender<Job>>,
    budget: Arc<QueuedBytes>,
    workers: Vec<JoinHandle<()>>,
}

impl BlockingPool {
    pub fn new(workers: usize, waiting_jobs: usize, waiting_bytes: usize) -> io::Result<Self> {
        if workers == 0 || waiting_jobs == 0 || waiting_bytes == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "blocking worker limits must be nonzero",
            ));
        }
        let (sender, receiver) = mpsc::sync_channel::<Job>(waiting_jobs);
        let receiver = Arc::new(Mutex::new(receiver));
        let mut pool = Self {
            sender: Some(sender),
            budget: Arc::new(QueuedBytes {
                used: AtomicUsize::new(0),
                limit: waiting_bytes,
            }),
            workers: Vec::new(),
        };
        for index in 0..workers {
            let receiver = Arc::clone(&receiver);
            // On a partial spawn failure, Pool::drop closes the queue and joins
            // the already-created workers before returning the failure.
            let worker = thread::Builder::new()
                .name(format!("gateway-native-{index}"))
                .spawn(move || loop {
                    let job = receiver.lock().ok().and_then(|queue| queue.recv().ok());
                    let Some(job) = job else { break };
                    // A native panic loses that reply; it must not silently
                    // shrink worker capacity for all following requests.
                    let _ = catch_unwind(AssertUnwindSafe(job));
                })?;
            pool.workers.push(worker);
        }
        Ok(pool)
    }

    /// Nonblocking admission. Every Err proves this work was never enqueued.
    /// Running calls no longer consume waiting bytes; their request sizes are
    /// separately bounded at ingress, and their count is the fixed worker count.
    pub fn submit<T: Send + 'static>(
        &self,
        input_bytes: usize,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<Ticket<T>, SubmitError> {
        let sender = self.sender.as_ref().ok_or(SubmitError::Stopped)?;
        self.budget
            .used
            .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(input_bytes)
                    .filter(|next| *next <= self.budget.limit)
            })
            .map_err(|_| SubmitError::BytesFull)?;
        let reservation = Reservation {
            budget: Arc::clone(&self.budget),
            bytes: input_bytes,
        };
        let (reply, receiver) = oneshot::channel();
        let job = Box::new(move || {
            drop(reservation);
            let result = work();
            let _ = reply.send(result);
        });
        sender.try_send(job).map_err(|error| match error {
            TrySendError::Full(_) => SubmitError::QueueFull,
            TrySendError::Disconnected(_) => SubmitError::Stopped,
        })?;
        Ok(Ticket { receiver })
    }
}

impl Drop for BlockingPool {
    fn drop(&mut self) {
        self.sender.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const GUARD: Duration = Duration::from_secs(10);

    fn wait_started(receiver: &mpsc::Receiver<()>) {
        receiver.recv_timeout(GUARD).unwrap();
    }

    fn reply<T>(ticket: Ticket<T>) -> Result<T, oneshot::error::RecvError> {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(async { tokio::time::timeout(GUARD, ticket.wait()).await.unwrap() })
    }

    #[test]
    fn two_workers_run_in_parallel_and_the_waiting_queue_is_bounded() {
        let pool = BlockingPool::new(2, 1, 32).unwrap();
        let (started, observed) = mpsc::channel();
        let (release_a, gate_a) = mpsc::channel();
        let (release_b, gate_b) = mpsc::channel();
        let started_a = started.clone();
        let a = pool
            .submit(8, move || {
                started_a.send(()).unwrap();
                gate_a.recv_timeout(GUARD).unwrap();
                1
            })
            .unwrap();
        // Admission is intentionally nonblocking: wait for the first dequeue
        // before admitting the second into a one-slot queue.
        wait_started(&observed);
        let b = pool
            .submit(8, move || {
                started.send(()).unwrap();
                gate_b.recv_timeout(GUARD).unwrap();
                2
            })
            .unwrap();
        wait_started(&observed);
        let c = pool.submit(8, || 3).unwrap();
        assert!(matches!(
            pool.submit(8, || panic!("not admitted")),
            Err(SubmitError::QueueFull)
        ));
        assert_eq!(pool.budget.used.load(Ordering::Acquire), 8);
        release_a.send(()).unwrap();
        release_b.send(()).unwrap();
        assert_eq!(reply(a).unwrap(), 1);
        assert_eq!(reply(b).unwrap(), 2);
        assert_eq!(reply(c).unwrap(), 3);
        assert_eq!(pool.budget.used.load(Ordering::Acquire), 0);
    }

    #[test]
    fn byte_budget_is_independent_of_job_count_and_rejected_work_never_runs() {
        let pool = BlockingPool::new(1, 4, 16).unwrap();
        let (started, observed) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let running = pool
            .submit(16, move || {
                started.send(()).unwrap();
                gate.recv_timeout(GUARD).unwrap();
            })
            .unwrap();
        wait_started(&observed);
        let queued = pool.submit(16, || 7).unwrap();
        assert!(matches!(
            pool.submit(1, || panic!("not admitted")),
            Err(SubmitError::BytesFull)
        ));
        assert!(matches!(
            pool.submit(usize::MAX, || panic!("not admitted")),
            Err(SubmitError::BytesFull)
        ));
        release.send(()).unwrap();
        reply(running).unwrap();
        assert_eq!(reply(queued).unwrap(), 7);
        assert_eq!(pool.budget.used.load(Ordering::Acquire), 0);
    }

    #[test]
    fn lost_waiter_does_not_cancel_admitted_work() {
        let pool = BlockingPool::new(1, 2, 32).unwrap();
        let (started, observed) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let running = pool
            .submit(8, move || {
                started.send(()).unwrap();
                gate.recv_timeout(GUARD).unwrap();
            })
            .unwrap();
        wait_started(&observed);
        let (applied, observed_applied) = mpsc::channel();
        let abandoned_waiter = pool.submit(8, move || applied.send(()).unwrap()).unwrap();
        drop(abandoned_waiter);
        release.send(()).unwrap();
        observed_applied.recv_timeout(GUARD).unwrap();
        reply(running).unwrap();
    }

    #[test]
    fn a_panicking_job_loses_its_reply_without_losing_the_worker() {
        let pool = BlockingPool::new(1, 1, 16).unwrap();
        let panic_reply = pool
            .submit(8, || panic!("synthetic backend panic"))
            .unwrap();
        assert!(reply(panic_reply).is_err());
        let next = pool.submit(8, || 9).unwrap();
        assert_eq!(reply(next).unwrap(), 9);
    }

    #[test]
    fn zero_limits_are_refused_before_threads_are_started() {
        for (workers, jobs, bytes) in [(0, 1, 1), (1, 0, 1), (1, 1, 0)] {
            assert!(
                matches!(BlockingPool::new(workers, jobs, bytes), Err(error) if error.kind() == io::ErrorKind::InvalidInput)
            );
        }
    }
}
