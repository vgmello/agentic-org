//! The copilot: one call, `draft`, from a request to a validated draft or a fixed-message error.

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::contract::{DraftRequest, DraftResponse, FeatureInfo};
use crate::feature::{Feature, Registry};
use crate::limits::{Limits, RunGate};
use crate::prompt;
use crate::runtime::{CancelToken, CopilotRuntime, RunOutput, RunRequest, RuntimeError, Usage};
use crate::secrets::contains_secret;

/// Why a request was refused before any run. Maps to the wire error `invalid_request`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidReason {
    UnknownFeature,
    /// The input breaks the feature's schema (a wrong shape, a missing or extra field).
    BadInput,
    /// Over a cap: a field over its length, the data over the byte cap, or too many context
    /// items.
    TooLarge,
    ContextNotAllowed,
    /// The text looks like a secret (a key, a token). Never echoed.
    LooksLikeSecret,
}

/// Why a run failed. Maps to the wire error `copilot_failed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailReason {
    /// The runtime failed to run.
    Runtime,
    /// The answer was over the output cap.
    AnswerTooLarge,
    /// The answer was not one JSON document.
    NotJson,
    /// The answer was JSON, but not the shape the feature asks for.
    SchemaMismatch,
    /// The answer holds something shaped like a secret.
    SecretInAnswer,
}

/// What `draft` can return as an error. The messages are fixed text: they never include the
/// input, the context, the prompt or the runtime's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopilotError {
    /// No runtime is set up. Wire: `copilot_unavailable`.
    Unavailable,
    /// Wire: `invalid_request`; over a cap, `too_large`; an unknown feature, `copilot_unavailable`.
    Invalid(InvalidReason),
    /// A run of this person's for this feature is still going. Wire: `copilot_busy`.
    Busy,
    /// Too many adapter calls are in flight (stuck ones count until they return). Wire:
    /// `copilot_busy` too, with its own message and log line.
    Overloaded,
    /// Wire: `timeout`.
    Timeout,
    /// Wire: `copilot_failed`.
    Failed(FailReason),
}

impl CopilotError {
    /// The wire error code.
    pub fn code(&self) -> &'static str {
        match self {
            // A feature this office doesn't offer is as good as unavailable.
            CopilotError::Unavailable | CopilotError::Invalid(InvalidReason::UnknownFeature) => {
                "copilot_unavailable"
            }
            CopilotError::Invalid(InvalidReason::TooLarge) => "too_large",
            CopilotError::Invalid(_) => "invalid_request",
            CopilotError::Busy | CopilotError::Overloaded => "copilot_busy",
            CopilotError::Timeout => "timeout",
            CopilotError::Failed(_) => "copilot_failed",
        }
    }

    /// How the run is counted in the run log.
    pub fn outcome(&self) -> Outcome {
        match self {
            CopilotError::Unavailable | CopilotError::Invalid(InvalidReason::UnknownFeature) => {
                Outcome::Unavailable
            }
            CopilotError::Invalid(_) => Outcome::InvalidInput,
            CopilotError::Busy | CopilotError::Overloaded => Outcome::Busy,
            CopilotError::Timeout => Outcome::Timeout,
            CopilotError::Failed(_) => Outcome::Failed,
        }
    }
}

impl fmt::Display for CopilotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            CopilotError::Unavailable => "no copilot runtime is set up",
            CopilotError::Invalid(InvalidReason::UnknownFeature) => "unknown copilot feature",
            CopilotError::Invalid(InvalidReason::BadInput) => "the input doesn't fit the feature",
            CopilotError::Invalid(InvalidReason::TooLarge) => "the input is too large",
            CopilotError::Invalid(InvalidReason::ContextNotAllowed) => {
                "this feature doesn't take that context"
            }
            CopilotError::Invalid(InvalidReason::LooksLikeSecret) => {
                "the input looks like it holds a secret"
            }
            CopilotError::Busy => "a run of this feature is still going",
            CopilotError::Overloaded => "too many copilot runs are in flight",
            CopilotError::Timeout => "the copilot took too long",
            CopilotError::Failed(FailReason::Runtime) => "the runtime failed",
            CopilotError::Failed(FailReason::AnswerTooLarge) => "the answer was too large",
            CopilotError::Failed(FailReason::NotJson) => "the answer wasn't one JSON document",
            CopilotError::Failed(FailReason::SchemaMismatch) => {
                "the answer didn't match the feature's schema"
            }
            CopilotError::Failed(FailReason::SecretInAnswer) => {
                "the answer looked like it held a secret"
            }
        })
    }
}

impl std::error::Error for CopilotError {}

/// How a run ended, for the run log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    InvalidInput,
    Busy,
    Unavailable,
    Timeout,
    Failed,
}

/// What the office logs per run. There is no field that could hold a body, on purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    pub user: String,
    pub feature: String,
    pub version: u32,
    pub outcome: Outcome,
    pub duration: Duration,
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub usage: Option<Usage>,
}

/// Where run records go. The office implements it with its own logger.
pub trait RunLog: Send + Sync {
    fn record(&self, record: &RunRecord);
}

/// The copilot, over a registry and a runtime.
pub struct Copilot<R> {
    registry: Registry,
    pub(crate) runtime: Arc<R>,
    limits: Limits,
    gate: RunGate,
    /// Adapter calls still running, including ones that timed out and haven't returned.
    in_flight: Arc<AtomicUsize>,
    log: Option<Arc<dyn RunLog>>,
}

/// One adapter call in flight; dropping it (the worker ends, also by panic) frees the count.
struct InFlight(Arc<AtomicUsize>);

impl InFlight {
    fn start(count: &Arc<AtomicUsize>, max: usize) -> Option<InFlight> {
        if count.fetch_add(1, Ordering::SeqCst) >= max {
            count.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(InFlight(count.clone()))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl<R: CopilotRuntime> Copilot<R> {
    pub fn new(registry: Registry, runtime: R, limits: Limits) -> Self {
        Copilot {
            registry,
            runtime: Arc::new(runtime),
            limits,
            gate: RunGate::new(),
            in_flight: Arc::new(AtomicUsize::new(0)),
            log: None,
        }
    }

    /// Send a [`RunRecord`] to `log` after every call.
    pub fn with_log(mut self, log: Arc<dyn RunLog>) -> Self {
        self.log = Some(log);
        self
    }

    /// `copilot.features`: empty when no runtime is set up, which is how the app knows to hide
    /// its smart buttons.
    pub fn features(&self) -> Vec<FeatureInfo> {
        if !self.runtime.available() {
            return Vec::new();
        }
        self.registry.iter().map(Feature::info).collect()
    }

    /// `copilot.draft`. `user` is the caller's id: one run per user per feature at a time.
    pub fn draft(&self, user: &str, request: &DraftRequest) -> Result<DraftResponse, CopilotError> {
        let started = Instant::now();
        let feature = self.registry.get(&request.feature);
        let mut record = RunRecord {
            user: user.to_owned(),
            // A name the registry doesn't know came from the caller and could be anything, so it is
            // never logged.
            feature: feature.map_or_else(|| "unknown".to_owned(), |f| f.id().to_owned()),
            version: feature.map_or(0, Feature::version),
            outcome: Outcome::Ok,
            duration: Duration::ZERO,
            input_bytes: 0,
            output_bytes: 0,
            usage: None,
        };
        let result = self.run(user, request, feature, &mut record);
        record.duration = started.elapsed();
        if let Err(e) = &result {
            record.outcome = e.outcome();
        }
        if let Some(log) = &self.log {
            log.record(&record);
        }
        result
    }

    fn run(
        &self,
        user: &str,
        request: &DraftRequest,
        feature: Option<&Feature>,
        record: &mut RunRecord,
    ) -> Result<DraftResponse, CopilotError> {
        use CopilotError::{Failed, Invalid};
        if !self.runtime.available() {
            return Err(CopilotError::Unavailable);
        }
        let feature = feature.ok_or(Invalid(InvalidReason::UnknownFeature))?;
        self.check_input(feature, request, record)?;

        let _slot = self
            .gate
            .try_acquire(user, feature.id())
            .ok_or(CopilotError::Busy)?;

        let prompt = prompt::render(feature, &request.input, &request.context)
            .map_err(|_| Invalid(InvalidReason::BadInput))?;
        let run = RunRequest {
            prompt,
            output_schema: feature.output_schema().document().clone(),
            timeout: self.limits.timeout,
            max_output_bytes: self.limits.max_output_bytes,
        };
        let output = self.run_with_deadline(run)?;
        record.output_bytes = output.text.len();
        record.usage = output.usage;
        if output.text.len() > self.limits.max_output_bytes {
            return Err(Failed(FailReason::AnswerTooLarge));
        }
        let answer: Value =
            serde_json::from_str(&output.text).map_err(|_| Failed(FailReason::NotJson))?;
        if !feature.output_schema().violations(&answer).is_empty() {
            return Err(Failed(FailReason::SchemaMismatch));
        }
        // Defence in depth: a draft that carries something shaped like a secret is not shown.
        if contains_secret(&answer) {
            return Err(Failed(FailReason::SecretInAnswer));
        }
        let mut answer = answer;
        for (field, kind) in feature.context_refs() {
            let offered = answer["draft"][field].as_str().is_some_and(|value| {
                request
                    .context
                    .iter()
                    .any(|c| c.kind == *kind && c.text == value)
            });
            if !offered {
                if let Some(draft) = answer["draft"].as_object_mut() {
                    draft.remove(field);
                }
            }
        }
        let notes = answer["notes"]
            .as_array()
            .map(|n| {
                n.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        Ok(DraftResponse {
            draft: answer["draft"].clone(),
            notes,
            usage: output.usage,
        })
    }

    /// Run the adapter on a worker thread and wait at most the timeout. At the deadline the run's
    /// [`CancelToken`] is set and `timeout` is the answer; the worker is left to wind down, and
    /// counts against `max_concurrent_runs` until it does. The caller's slot is free as soon as
    /// this returns.
    fn run_with_deadline(&self, run: RunRequest) -> Result<RunOutput, CopilotError> {
        let flight = InFlight::start(&self.in_flight, self.limits.max_concurrent_runs)
            .ok_or(CopilotError::Overloaded)?;
        let timeout = self.limits.timeout;
        let cancel = CancelToken::new();
        let (done, answer) = mpsc::channel();
        let runtime = Arc::clone(&self.runtime);
        let worker_cancel = cancel.clone();
        thread::Builder::new()
            .name("copilot-run".into())
            .spawn(move || {
                let _flight = flight;
                // The receiver may be gone (we gave up): then the answer is thrown away.
                let _ = done.send(runtime.run(&run, &worker_cancel));
            })
            .map_err(|_| CopilotError::Failed(FailReason::Runtime))?;
        match answer.recv_timeout(timeout) {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(RuntimeError::TimedOut)) => Err(CopilotError::Timeout),
            Ok(Err(RuntimeError::NotSetUp)) => Err(CopilotError::Unavailable),
            Ok(Err(RuntimeError::Failed)) => Err(CopilotError::Failed(FailReason::Runtime)),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                cancel.cancel();
                Err(CopilotError::Timeout)
            }
            // The worker ended without an answer: the adapter panicked.
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(CopilotError::Failed(FailReason::Runtime))
            }
        }
    }

    fn check_input(
        &self,
        feature: &Feature,
        request: &DraftRequest,
        record: &mut RunRecord,
    ) -> Result<(), CopilotError> {
        use CopilotError::Invalid;
        if request.context.len() > self.limits.max_context_items {
            return Err(Invalid(InvalidReason::TooLarge));
        }
        if request
            .context
            .iter()
            .any(|c| !feature.accepts_context(&c.kind))
        {
            return Err(Invalid(InvalidReason::ContextNotAllowed));
        }
        // The cap is on what goes into the prompt: the escaped data block. Field caps in the
        // schema are in characters, this outer one in bytes.
        let input_bytes = prompt::data_json(&request.input, &request.context).len();
        record.input_bytes = input_bytes;
        if input_bytes > self.limits.max_input_bytes {
            return Err(Invalid(InvalidReason::TooLarge));
        }
        let violations = feature.input_schema().violations(&request.input);
        if !violations.is_empty() {
            // A field over its length cap is `too_large`; any other break is a bad shape.
            let only_caps = violations
                .iter()
                .all(|v| matches!(v.rule, "maxLength" | "maxItems"));
            return Err(Invalid(if only_caps {
                InvalidReason::TooLarge
            } else {
                InvalidReason::BadInput
            }));
        }
        let context_has_secret = request.context.iter().any(|c| {
            [&c.kind, &c.label, &c.text]
                .into_iter()
                .any(|t| crate::secrets::text_has_secret(t))
        });
        if contains_secret(&request.input) || context_has_secret {
            return Err(Invalid(InvalidReason::LooksLikeSecret));
        }
        Ok(())
    }
}
