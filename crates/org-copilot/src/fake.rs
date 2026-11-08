//! A scripted runtime for tests, here and in the office: no process, no network.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use crate::runtime::{CopilotRuntime, RunOutput, RunRequest, RuntimeError, Usage};

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
}

/// Plays its replies in order. With none left, a run fails with [`RuntimeError::Failed`].
#[derive(Debug)]
pub struct FakeRuntime {
    available: bool,
    replies: Mutex<VecDeque<Reply>>,
    prompts: Mutex<Vec<String>>,
    started: AtomicUsize,
}

impl FakeRuntime {
    /// A runtime that is set up and will play `replies`.
    pub fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        FakeRuntime {
            available: true,
            replies: Mutex::new(replies.into_iter().collect()),
            prompts: Mutex::new(Vec::new()),
            started: AtomicUsize::new(0),
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

    /// How many runs have started (a run counts from the moment it is called).
    pub fn started(&self) -> usize {
        self.started.load(Ordering::SeqCst)
    }
}

impl CopilotRuntime for FakeRuntime {
    fn available(&self) -> bool {
        self.available
    }

    fn run(&self, request: &RunRequest) -> Result<RunOutput, RuntimeError> {
        self.prompts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request.prompt.clone());
        self.started.fetch_add(1, Ordering::SeqCst);
        let reply = self
            .replies
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front();
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
