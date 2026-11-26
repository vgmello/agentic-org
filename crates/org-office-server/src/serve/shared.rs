//! The state every connection shares (ports, bound connections, live-event delivery) and the live
//! owner and person checks made against it.

use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::follow::{self, Timing};
use org_mailbox::Mailbox;
use org_proto::{HelloResult, MemberKind, OfficeHello, OfficeInfo, OfficeRole, OfficeStatus};

use super::auth::{Binding, LaunchAuth};
use super::held::Bound;
use super::OfficePorts;

/// What every connection shares.
pub struct Shared {
    pub hello: HelloResult,
    pub(super) office_id: String,
    pub(super) ports: OfficePorts,
    /// Typed launch authority, retained and zeroed when `Shared` drops.
    pub(super) auth: LaunchAuth,
    pub(super) timing: Timing,
    /// The connections bound as a member, for the watcher.
    pub(super) bound: Bound,
    pub(super) directory_change_token: Option<Box<dyn Fn() -> u64 + Send + Sync>>,
    pub(crate) inbox: Option<crate::inbox::Hq>,
    pub(super) hint: Option<crate::inbox::Hint>,
    pub(crate) stopping: std::sync::atomic::AtomicBool,
    /// The office-wide budgets of the unbound sign-in calls (`auth.*`).
    pub(super) auth_limits: super::auth_limits::AuthRateLimits,
}

impl Shared {
    /// The same, with other timings (tests).
    #[cfg(test)]
    pub(super) fn with_timing(mut self, timing: Timing) -> Self {
        self.timing = timing;
        self
    }

    /// The `hello` this office answers with.
    pub fn new(
        info: &org_office::OfficeInfo,
        socket: &Path,
        ports: OfficePorts,
        auth: impl Into<LaunchAuth>,
    ) -> Self {
        Self {
            office_id: info.office_id.clone(),
            ports,
            auth: auth.into(),
            timing: Timing::default(),
            bound: Mutex::default(),
            directory_change_token: None,
            inbox: None,
            hint: None,
            stopping: std::sync::atomic::AtomicBool::new(false),
            auth_limits: super::auth_limits::AuthRateLimits::default(),
            // Part 1 implements only what every server of this api_version has: no capabilities.
            hello: HelloResult::office(
                env!("CARGO_PKG_VERSION"),
                "This Mac",
                OfficeHello {
                    organization_name: info.org_name.clone(),
                    office: OfficeInfo {
                        id: info.office_id.clone(),
                        organization_id: info.org_id.clone(),
                        name: info.office_name.clone(),
                        role: match info.role {
                            org_office::OfficeRole::Hq => OfficeRole::Hq,
                            org_office::OfficeRole::Satellite => OfficeRole::Satellite,
                        },
                        host: "This Mac".to_owned(),
                        endpoint: Some(socket.display().to_string()),
                        status: OfficeStatus::Active,
                        status_reason: None,
                        next_restart_at: None,
                    },
                },
            ),
        }
    }

    /// Returns the office's mailbox port when messaging is enabled.
    pub(crate) fn mailbox(&self) -> Option<Arc<Mailbox>> {
        self.ports.mailbox.clone()
    }

    /// Track organization changes for revocation even when no subscription feed is served.
    pub(crate) fn watch_directory_changes(
        &mut self,
        token: impl Fn() -> u64 + Send + Sync + 'static,
    ) {
        self.directory_change_token = Some(Box::new(token));
    }

    /// Only a known HQ composition registers real sources; other offices stay dark.
    pub(crate) fn register_inbox(
        &mut self,
        inbox: crate::inbox::Hq,
        authority: Result<bool, org_office::OfficeError>,
    ) {
        match authority {
            Ok(true) => {
                self.inbox = Some(inbox);
                self.hello.capabilities.push("inbox.list".into());
            }
            Ok(false) => {}
            Err(_) => eprintln!("agentic-org-office: inbox disabled (authority read unavailable)"),
        }
    }

    pub(crate) fn enable_inbox_hints(&mut self) -> Result<(), String> {
        // Hints reach the owner only through `subscribe`: no feed, no hints.
        let Some(feed) = &self.ports.feed else {
            return Ok(());
        };
        if self.inbox.is_some() && self.hint.is_none() {
            self.hint = Some(crate::inbox::Hint::new(
                feed.head()?,
                self.ports.live.mailbox_head(),
            ));
            self.hello.capabilities.push("inbox.changed".into());
        }
        Ok(())
    }
}

pub(super) struct Subscriber<'a> {
    pub(super) binding: &'a Binding,
    pub(super) wire: &'a follow::Wire<'a>,
}

pub(super) fn deliver_hint(
    shared: &Shared,
    subscriber: &Subscriber<'_>,
    data: &org_proto::inbox::InboxChangedData,
) -> bool {
    if shared.hint.is_some() && matches!(live_owner(shared, subscriber.binding), Ok(true)) {
        subscriber.wire.emit::<org_proto::inbox::InboxChanged>(data)
    } else {
        true
    }
}

pub(crate) type LookupError = org_mailbox::StoreError;

/// One live owner check for list authorization and a2's per-delivery hint filtering.
pub(crate) fn live_owner(shared: &Shared, binding: &Binding) -> Result<bool, LookupError> {
    let Binding::Member { id, .. } = binding else {
        return Ok(false);
    };
    shared.ports.directory.resolve_member(id).map(|found| {
        found.is_some_and(|(member, _)| {
            member.id == *id && member.kind == MemberKind::Person && member.owner
        })
    })
}

/// One live person-member check for multi-person host-channel reads and member actions (`TASK-38`).
pub(crate) fn live_person(shared: &Shared, binding: &Binding) -> Result<bool, LookupError> {
    let Binding::Member { id, .. } = binding else {
        return Ok(false);
    };
    shared.ports.directory.resolve_member(id).map(|found| {
        found.is_some_and(|(member, _)| member.id == *id && member.kind == MemberKind::Person)
    })
}
