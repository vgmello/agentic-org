//! Calls: sending a request and waiting for its answer within the call's timeout.

use std::sync::mpsc;

use org_proto::{Call, CallError, Request};

use super::*;
use crate::transport::{await_response, encode_request, enqueue_request, finish_outcome};

/// Per-response metadata carried alongside an RPC result (`TASK-115` / `doc-101`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CallMeta {
    /// Non-zero pending wakes/messages hint reported on this response, if any.
    pub waiting_messages: Option<u32>,
}

impl CallMeta {
    fn from_response(response: &Response) -> Self {
        Self {
            waiting_messages: response.waiting_messages.filter(|&n| n > 0),
        }
    }
}

/// An RPC call's decoded value paired with the metadata from that exact response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallOutcome<T> {
    pub value: T,
    pub meta: CallMeta,
}

impl Client {
    /// Calls `C` and waits for the answer up to [`default_timeout`] for its method
    /// ([`DEFAULT_TIMEOUT`], or longer for the calls that are slow by nature).
    pub fn call<C: Call>(&self, params: &C::Params) -> Result<C::Result, ClientError> {
        self.call_with_meta::<C>(params)
            .map(|outcome| outcome.value)
    }

    /// Calls `C` and returns both the decoded result and per-response [`CallMeta`].
    pub fn call_with_meta<C: Call>(
        &self,
        params: &C::Params,
    ) -> Result<CallOutcome<C::Result>, ClientError> {
        self.call_with_timeout_and_meta::<C>(params, default_timeout(C::METHOD))
    }

    /// Calls `C`. `timeout` bounds the whole call, sending the request and then waiting for
    /// the answer. A request that can't be fully sent by then closes the connection, because
    /// part of it may be on the wire.
    pub fn call_with_timeout<C: Call>(
        &self,
        params: &C::Params,
        timeout: Duration,
    ) -> Result<C::Result, ClientError> {
        self.call_with_timeout_and_meta::<C>(params, timeout)
            .map(|outcome| outcome.value)
    }

    /// Calls `C` with an explicit timeout and returns both the decoded result and per-response
    /// [`CallMeta`].
    pub fn call_with_timeout_and_meta<C: Call>(
        &self,
        params: &C::Params,
        timeout: Duration,
    ) -> Result<CallOutcome<C::Result>, ClientError> {
        self.call_tracked::<C>(params, timeout, &mut false)
    }

    /// Sends one raw JSON-RPC method and params object (for example organization writes that
    /// carry `dry_run: true` without a `command_id`) and returns the JSON `result` value.
    pub fn call_raw(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        self.call_raw_with_meta(method, params)
            .map(|outcome| outcome.value)
    }

    /// Like [`Client::call_raw`], returning the JSON `result` alongside per-response [`CallMeta`].
    pub fn call_raw_with_meta(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<CallOutcome<serde_json::Value>, ClientError> {
        self.call_raw_with_timeout_and_meta(method, params, default_timeout(method))
    }

    /// Sends one raw JSON-RPC method and params object with an explicit timeout budget (used for
    /// bounded short-hold calls such as `chat.ask`, `chat.wait_ask`, and `msg.wait`).
    pub fn call_raw_with_timeout(
        &self,
        method: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value, ClientError> {
        self.call_raw_with_timeout_and_meta(method, params, timeout)
            .map(|outcome| outcome.value)
    }

    /// Like [`Client::call_raw_with_timeout`], returning the JSON `result` alongside per-response
    /// [`CallMeta`].
    pub fn call_raw_with_timeout_and_meta(
        &self,
        method: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> Result<CallOutcome<serde_json::Value>, ClientError> {
        let id = self.next_request_id()?;
        let request = Request {
            id,
            method: method.to_owned(),
            params,
            principal: None,
        };
        let response = self.send_request(request, timeout, &mut false)?;
        let meta = CallMeta::from_response(&response);
        match response.outcome {
            org_proto::Outcome::Result(value) => Ok(CallOutcome { value, meta }),
            org_proto::Outcome::Error(error) => Err(ClientError::Api(error)),
        }
    }

    fn next_request_id(&self) -> Result<u64, ClientError> {
        let id = self.shared.next_id.fetch_add(1, Ordering::SeqCst);
        if id > MAX_REQUEST_ID {
            return Err(ClientError::Protocol(
                "this connection has used up its request ids; reconnect".into(),
            ));
        }
        Ok(id)
    }

    /// [`Client::call_with_timeout_and_meta`], also saying whether the request was handed to the
    /// writer and not withdrawn again (`sent`), i.e. whether the server may have seen any of it.
    pub(crate) fn call_tracked<C: Call>(
        &self,
        params: &C::Params,
        timeout: Duration,
        sent: &mut bool,
    ) -> Result<CallOutcome<C::Result>, ClientError> {
        let id = self.next_request_id()?;
        let request =
            Request::new::<C>(id, params).map_err(|e| ClientError::Protocol(e.to_string()))?;
        let response = self.send_request(request, timeout, sent)?;
        let meta = CallMeta::from_response(&response);
        let value = response.into_result::<C>().map_err(|error| match error {
            CallError::Api(error) => ClientError::Api(error),
            CallError::Decode(_) => {
                ClientError::undecodable(&format!("the result of `{}`", C::METHOD))
            }
        })?;
        Ok(CallOutcome { value, meta })
    }

    fn send_request(
        &self,
        request: Request,
        timeout: Duration,
        sent: &mut bool,
    ) -> Result<Response, ClientError> {
        let started = Instant::now();
        let deadline = started
            .checked_add(timeout)
            .unwrap_or_else(|| started + Duration::from_secs(60 * 60 * 24 * 365));
        let id = request.id;
        let line = encode_request(&request)?;

        let (answer_to, answer) = mpsc::sync_channel(1);
        lock(&self.shared.pending).insert(id, answer_to);
        if self.shared.closed.load(Ordering::SeqCst) {
            lock(&self.shared.pending).remove(&id);
            return Err(ClientError::Disconnected);
        }

        let state = Arc::new(AtomicU8::new(QUEUED));
        enqueue_request(
            &self.shared,
            self.outbound.as_ref(),
            id,
            Outgoing {
                line,
                deadline,
                state: Arc::clone(&state),
            },
            deadline,
            sent,
        )?;

        let outcome = await_response(&self.shared, id, &answer, deadline);
        if !matches!(outcome, Ok(Ok(_))) && self.settle(&state) {
            *sent = false;
        }
        finish_outcome(outcome, deadline)
    }

    /// This call is out of time (or the connection is going away). What happens to its request
    /// depends on how far the writer got:
    /// - not started: withdraw it, so it is never sent (nothing is on the wire);
    /// - being written: part of it may be on the wire. The writer decides: it finishes, or it
    ///   notices the deadline and gives the request up itself, usually within milliseconds, so
    ///   a healthy connection is not closed just because the deadline fell while the writer was
    ///   busy. Only if the write is still stuck after [`WRITE_GRACE`] does this call close the
    ///   connection, which also wakes the blocked write (a peer that takes a few bytes at a
    ///   time keeps one `write()` going well past any deadline).
    ///
    /// Returns whether the request was withdrawn without any of it being written.
    ///
    /// Known limit: the writer thread is in `WRITING` from the moment it picks a request up
    /// until the whole request is out. A write that is still going `WRITE_GRACE` after the
    /// deadline (a peer that takes a large request slowly, or a writer that is not scheduled
    /// for that long under heavy load or a stopped process) looks the same as a stuck write,
    /// and the caller then closes the connection, healthy or not. Callers must be ready to
    /// reconnect.
    fn settle(&self, state: &AtomicU8) -> bool {
        self.settle_with(state, Instant::now, || {
            std::thread::sleep(Duration::from_millis(1));
        })
    }

    pub(crate) fn settle_with(
        &self,
        state: &AtomicU8,
        mut now: impl FnMut() -> Instant,
        mut wait_for_writer: impl FnMut(),
    ) -> bool {
        match state.compare_exchange(QUEUED, ABANDONED, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) | Err(ABANDONED) => return true,
            Err(WRITING) => {}
            Err(_) => return false, // written
        }
        let give_up_at = now() + WRITE_GRACE;
        while now() < give_up_at {
            let progress = state.load(Ordering::SeqCst);
            if progress != WRITING || self.shared.closed.load(Ordering::SeqCst) {
                return progress == ABANDONED;
            }
            wait_for_writer();
        }
        if state.load(Ordering::SeqCst) == WRITING {
            self.shared.close();
        }
        false
    }
}
