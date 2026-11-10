//! A scripted runtime for tests, here and in the office: no process, no network.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use crate::runtime::{CancelToken, CopilotRuntime, RunOutput, RunRequest, RuntimeError, Usage};

/// What the fake does on one run.
#[derive(Debug, Clone)]
pub enum Reply {
    /// Answer with this text.
    Text(String),
    /// Fail like this.
    Error(RuntimeError),
    /// Wait this long (a real sleep: keep it short in tests), then answer with the text.
    Slow(Duration, String),
    /// Wait this long, then fail.
    SlowFail(Duration),
    /// Never answer, like a hung adapter whose child doesn't die: block until
    /// [`FakeRuntime::release`], whatever the cancel token says.
    Hang,
    /// Block until [`FakeRuntime::release`], then answer with the text: a run that is still going
    /// for exactly as long as the test needs, with no sleep to outlast.
    Held(String),
    /// Never answer until cancelled, like a well-behaved adapter that kills its child at the
    /// deadline: then fail with [`RuntimeError::TimedOut`].
    HangUntilCancelled,
    /// Block until cancelled, then answer with the text anyway: an answer that provably arrives
    /// after the deadline, with no race against the clock.
    AfterCancel(String),
}

/// Plays its replies in order. With none left, a run fails with [`RuntimeError::Failed`].
#[derive(Debug)]
pub struct FakeRuntime {
    available: bool,
    replies: Mutex<VecDeque<Reply>>,
    prompts: Mutex<Vec<String>>,
    started: AtomicUsize,
    cancelled: AtomicUsize,
    released: AtomicBool,
}

impl FakeRuntime {
    /// A runtime that is set up and will play `replies`.
    pub fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        FakeRuntime {
            available: true,
            replies: Mutex::new(replies.into_iter().collect()),
            prompts: Mutex::new(Vec::new()),
            started: AtomicUsize::new(0),
            cancelled: AtomicUsize::new(0),
            released: AtomicBool::new(false),
        }
    }

    /// A runtime that answers with `text` once.
    pub fn answering(text: &str) -> Self {
        Self::new([Reply::Text(text.to_owned())])
    }

    /// A machine with no runtime set up.
    pub fn not_set_up() -> Self {
        FakeRuntime {
            available: false,
            ..Self::new([])
        }
    }

    /// The prompts it was given, oldest first.
    pub fn prompts(&self) -> Vec<String> {
        self.prompts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// How many runs saw their cancel token set.
    pub fn cancelled(&self) -> usize {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Let every [`Reply::Hang`] and [`Reply::Held`] run go (so a test doesn't leave threads
    /// behind).
    pub fn release(&self) {
        self.released.store(true, Ordering::SeqCst);
    }

    /// How many runs have started (a run counts once it has taken its reply, before it waits).
    pub fn started(&self) -> usize {
        self.started.load(Ordering::SeqCst)
    }

    fn wait_for_release(&self) {
        while !self.released.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Block until the run's token is set, and count it in [`FakeRuntime::cancelled`].
    fn wait_for_cancel(&self, cancel: &CancelToken) {
        while !cancel.is_cancelled() {
            thread::sleep(Duration::from_millis(5));
        }
        self.cancelled.fetch_add(1, Ordering::SeqCst);
    }
}

impl CopilotRuntime for FakeRuntime {
    fn available(&self) -> bool {
        self.available
    }

    fn run(&self, request: &RunRequest, cancel: &CancelToken) -> Result<RunOutput, RuntimeError> {
        self.prompts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request.prompt.clone());
        let reply = self
            .replies
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front();
        // Counted after the reply is taken, so a test that sees a run started knows which reply
        // it has.
        self.started.fetch_add(1, Ordering::SeqCst);
        let text = match reply {
            None | Some(Reply::Error(RuntimeError::Failed)) => return Err(RuntimeError::Failed),
            Some(Reply::Error(e)) => return Err(e),
            Some(Reply::Text(text)) => text,
            Some(Reply::Slow(wait, text)) => {
                thread::sleep(wait);
                text
            }
            Some(Reply::SlowFail(wait)) => {
                thread::sleep(wait);
                return Err(RuntimeError::Failed);
            }
            Some(Reply::Hang) => {
                self.wait_for_release();
                return Err(RuntimeError::Failed);
            }
            Some(Reply::Held(text)) => {
                self.wait_for_release();
                text
            }
            Some(Reply::HangUntilCancelled) => {
                self.wait_for_cancel(cancel);
                return Err(RuntimeError::TimedOut);
            }
            Some(Reply::AfterCancel(text)) => {
                self.wait_for_cancel(cancel);
                text
            }
        };
        Ok(RunOutput {
            text,
            usage: Some(Usage {
                input_tokens: Some(10),
                output_tokens: Some(5),
            }),
        })
    }
}
