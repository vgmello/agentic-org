//! The protocol's typed calls: `hello` and its version checks, `msg.bind`, the host's
//! lists, and `subscribe`.

use std::sync::mpsc;

use org_proto::msg::{MsgBind, MsgBindParams, MsgBindResult};
use org_proto::{
    ApiRange, Call, Empty, Hello, HelloParams, HelloResult, ListOfficesResult,
    ListOrganizationsResult, OfficeInfo, OfficeList, Organization, OrganizationList, Position,
    Subscribe, SubscribeParams, API_VERSION,
};

use super::*;

impl Client {
    /// `hello`, then checks the protocol version. A server of another version is refused:
    /// use this first on every new connection.
    pub fn hello(&self) -> Result<HelloResult, ClientError> {
        self.hello_with_timeout(DEFAULT_TIMEOUT)
    }

    /// [`Client::hello`] with a timeout of your own.
    pub fn hello_with_timeout(&self, timeout: Duration) -> Result<HelloResult, ClientError> {
        let hello = self.call_with_timeout::<Hello>(&HelloParams::default(), timeout)?;
        if hello.api_version != API_VERSION {
            return Err(ClientError::VersionMismatch {
                client: API_VERSION,
                server: hello.api_version,
            });
        }
        Ok(hello)
    }

    /// Offers API versions and verifies the selected instance (X-02 §3).
    ///
    /// The offer is the caller's contract; it is not clamped to `SUPPORTED_API`.
    /// A malformed range or noncanonical expected ULID returns a fixed protocol
    /// error without sending anything or closing this connection.
    ///
    /// After valid arguments, checks the decoded answer's version, then its
    /// instance. Version 1 cannot prove an instance. Transport and API errors keep
    /// their variants; any handshake error closes this connection and later calls
    /// fail with [`ClientError::Disconnected`]. Use this before sharing or using a
    /// new connection. Each repeated hello checks its own offer and expected ID.
    /// The legacy [`Client::hello`] wrappers retain their version-1 behavior.
    pub fn hello_offering(
        &self,
        offered: ApiRange,
        expected_instance_id: &str,
        timeout: Duration,
    ) -> Result<HelloResult, ClientError> {
        if offered.min == 0 || offered.min > offered.max {
            return Err(ClientError::Protocol(
                "the offered API range is invalid".into(),
            ));
        }
        if !org_proto::is_uuid_v7(expected_instance_id) {
            return Err(ClientError::Protocol(
                "the expected instance id is invalid".into(),
            ));
        }
        let outcome = (|| {
            let mut params = HelloParams::default();
            params.api = Some(offered);
            let hello = self.call_with_timeout::<Hello>(&params, timeout)?;
            if !offered.contains(hello.api_version) {
                return Err(ClientError::Protocol(
                    "the server chose an API version outside the offered range".into(),
                ));
            }
            if hello.api_version == 1 {
                return Err(ClientError::Protocol(
                    "the instance cannot be verified with API version 1".into(),
                ));
            }
            let instance = match hello.instance_id.as_deref() {
                Some(id) if org_proto::is_uuid_v7(id) => id,
                _ => {
                    return Err(ClientError::Protocol(
                        "the server's instance id is missing or malformed".into(),
                    ))
                }
            };
            if instance != expected_instance_id {
                return Err(ClientError::InstanceMismatch);
            }
            Ok(hello)
        })();
        if outcome.is_err() {
            self.shared.close();
        }
        outcome
    }

    /// `msg.bind` as a member, with no token: how an agent run by hand binds (ADR-0003 M-A).
    /// `member` is a handle or member id; the result names the member it resolved to. The
    /// office refuses (`not_authorized`) a member that isn't an active or paused agent placed
    /// in it, and a person or the owner (they act through a grant on a host channel) or an
    /// office-run agent. A
    /// binding belongs to the connection, and a failed bind leaves it unbound.
    pub fn bind_member(&self, member: &str) -> Result<MsgBindResult, ClientError> {
        self.bind_member_with_timeout(member, DEFAULT_TIMEOUT)
    }

    /// [`Client::bind_member`] with a timeout of your own.
    pub fn bind_member_with_timeout(
        &self,
        member: &str,
        timeout: Duration,
    ) -> Result<MsgBindResult, ClientError> {
        self.call_with_timeout::<MsgBind>(&MsgBindParams::member(member), timeout)
    }

    /// [`Client::call_with_timeout`] that gives up early once `stop()` returns true, for a call
    /// the server holds (`msg.wait`) that a person may want to interrupt. `stop` is checked
    /// every 50 ms. Giving up closes the connection, which also ends the held call on
    /// the server; the result is then `None`, and every later call on this client fails with
    /// [`ClientError::Disconnected`].
    pub fn call_unless<C: Call>(
        &self,
        params: &C::Params,
        timeout: Duration,
        stop: impl Fn() -> bool,
    ) -> Option<Result<C::Result, ClientError>>
    where
        C::Params: Sync,
        C::Result: Send,
    {
        std::thread::scope(|scope| {
            let call = scope.spawn(|| self.call_with_timeout::<C>(params, timeout));
            loop {
                if call.is_finished() {
                    return Some(join_scoped(call));
                }
                if stop() {
                    self.shared.close();
                    let _ = join_scoped(call);
                    return None;
                }
                std::thread::sleep(STOP_POLL);
            }
        })
    }

    /// `organization.list` (host): every organization with an office on this machine.
    pub fn list_organizations(&self) -> Result<Vec<Organization>, ClientError> {
        let ListOrganizationsResult { organizations } = self.call::<OrganizationList>(&Empty {})?;
        Ok(organizations)
    }

    /// `office.list` (host): every office on this machine, with how to reach it.
    pub fn list_offices(&self) -> Result<Vec<OfficeInfo>, ClientError> {
        let ListOfficesResult { offices } = self.call::<OfficeList>(&Empty {})?;
        Ok(offices)
    }

    /// Subscribes to server events. The [`Subscription`] ends when the connection closes, and
    /// holds at most 256 events: a reader that falls further behind loses the connection
    /// (calls then fail with `Disconnected`). So do not leave it unread for long, and note
    /// that more than 256 events arriving before the server's answer to `subscribe` also
    /// closes it.
    ///
    /// Once at a time: while a subscription is alive, a second call fails with
    /// [`ClientError::AlreadySubscribed`] and leaves the first as it was. Dropping the
    /// subscription releases it. If the call times out the server may or may not have
    /// subscribed, so the connection is closed: reconnect. (A timeout while the request was
    /// still waiting its turn behind another call's write is different: nothing was sent, so
    /// the connection is left alone.)
    ///
    /// This follows the live stream from now on. To resume after a snapshot without missing
    /// a change, use [`Client::subscribe_from`].
    pub fn subscribe(&self) -> Result<Subscription, ClientError> {
        self.subscribe_from(None)
    }

    /// Like [`Client::subscribe`], but resumes from `from_position` (the position of an
    /// `org.snapshot`): the server first sends every change after it, in order, then follows
    /// live. If the server can no longer resume from that position it answers
    /// `snapshot_required` (a [`ClientError::Api`] with that code): take a fresh snapshot and
    /// subscribe from its position. `None` follows live only.
    pub fn subscribe_from(
        &self,
        from_position: Option<Position>,
    ) -> Result<Subscription, ClientError> {
        self.subscribe_from_with_timeout(from_position, DEFAULT_TIMEOUT)
    }

    /// [`Client::subscribe_from`] with a timeout of your own. If the timeout passes after the
    /// request was sent, the server may or may not have subscribed, so the connection is closed;
    /// if it passes while the request was still waiting its turn (nothing sent), the connection
    /// is left alone.
    pub fn subscribe_from_with_timeout(
        &self,
        from_position: Option<Position>,
        timeout: Duration,
    ) -> Result<Subscription, ClientError> {
        let (events, received) = mpsc::sync_channel(EVENT_QUEUE);
        let id = self.shared.next_subscription.fetch_add(1, Ordering::SeqCst);
        {
            // Install the channel first: an event may arrive before the subscribe answer.
            let mut installed = lock(&self.shared.events);
            if installed.is_some() {
                return Err(ClientError::AlreadySubscribed);
            }
            *installed = Some((id, events));
        }
        // From here the `Subscription` owns the slot, and releases it when dropped.
        let subscription = Subscription {
            events: received,
            id,
            slot: Arc::clone(&self.shared.events),
        };
        let mut sent = false;
        let params = SubscribeParams {
            from_position,
            from_work_position: None,
        };
        match self.call_tracked::<Subscribe>(&params, timeout, &mut sent) {
            Ok(Empty {}) => Ok(subscription),
            Err(error) => {
                if sent && matches!(error, ClientError::Timeout) {
                    self.shared.close();
                }
                // `subscription` drops here and releases the slot.
                Err(error)
            }
        }
    }
}
