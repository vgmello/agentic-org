//! A small fixed pool of worker threads. Connections hand requests to it, so one
//! connection can have several requests in flight and a slow one doesn't hold up the rest.

use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

type Task = Box<dyn FnOnce() + Send>;

/// Tasks waiting for a worker. Beyond this a connection's reader blocks, which pushes back
/// on that one client.
const QUEUE: usize = 1024;

pub(crate) struct Pool {
    tasks: Option<SyncSender<Task>>,
    workers: Vec<JoinHandle<()>>,
}

/// A cheap handle for submitting work.
#[derive(Clone)]
pub(crate) struct Submitter(SyncSender<Task>);

impl Submitter {
    /// Queues `task`; `false` if the pool has shut down.
    pub(crate) fn submit(&self, task: impl FnOnce() + Send + 'static) -> bool {
        self.0.send(Box::new(task)).is_ok()
    }
}

impl Pool {
    pub(crate) fn new(threads: usize) -> std::io::Result<Self> {
        let (tasks, queue) = mpsc::sync_channel::<Task>(QUEUE);
        let queue: Arc<Mutex<Receiver<Task>>> = Arc::new(Mutex::new(queue));
        let workers = (0..threads.max(1))
            .map(|n| {
                let queue = Arc::clone(&queue);
                std::thread::Builder::new()
                    .name(format!("org-office-worker-{n}"))
                    .spawn(move || loop {
                        // Hold the lock only while taking a task, not while running it.
                        let task = queue
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .recv();
                        match task {
                            Ok(task) => task(),
                            Err(_) => return,
                        }
                    })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            tasks: Some(tasks),
            workers,
        })
    }

    pub(crate) fn submitter(&self) -> Submitter {
        Submitter(self.tasks.clone().expect("pool is running"))
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        // Close the queue; workers finish their current task and exit.
        self.tasks.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}
