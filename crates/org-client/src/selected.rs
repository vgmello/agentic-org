//! Human selected-task transport on an actual host-approved descriptor. No
//! launcher, legacy app token or agent binding confers this authority.
use crate::{Client, ClientError, DEFAULT_TIMEOUT};
use org_proto::{
    selected_tasks::{AssignmentResult, NativeAssignment, SetAssignment, CAPABILITY},
    HelloResult, Principal,
};
use std::{os::fd::OwnedFd, time::Duration};

pub struct ApprovedOffice {
    client: Client,
    instance: String,
    organization: String,
    office: String,
    channel: String,
    hello: HelloResult,
}
/// Preserve the original operation ID on any possible submission. A transport
/// failure says nothing about rollback; the caller must reconcile by that ID.
#[derive(Debug)]
pub struct SubmissionFailure {
    pub may_have_submitted: bool,
    pub error: ClientError,
}
impl ApprovedOffice {
    /// The host descriptor receipt is the caller's responsibility; this takes
    /// ownership and verifies negotiation/scope before any business operation.
    pub fn from_descriptor(
        fd: OwnedFd,
        instance: String,
        organization: String,
        office: String,
        channel: String,
    ) -> Result<Self, ClientError> {
        if [&instance, &organization, &office, &channel]
            .iter()
            .any(|id| id.is_empty())
        {
            return Err(ClientError::Protocol(
                "missing approved channel scope".into(),
            ));
        }
        let stream = org_socket::rights::connected_unix_stream(fd).map_err(ClientError::Io)?;
        stream.set_nonblocking(false).map_err(ClientError::Io)?;
        let client = Client::from_stream(stream)?;
        let hello = client.hello_offering(
            org_proto::ApiRange { min: 2, max: 2 },
            &instance,
            DEFAULT_TIMEOUT,
        )?;
        if hello.instance_id.as_deref() != Some(&instance)
            || !hello
                .office
                .as_ref()
                .is_some_and(|o| o.office.id == office && o.office.organization_id == organization)
        {
            return Err(ClientError::Protocol(
                "approved office scope mismatch".into(),
            ));
        }
        Ok(Self {
            client,
            instance,
            organization,
            office,
            channel,
            hello,
        })
    }
    pub fn hello(&self) -> &HelloResult {
        &self.hello
    }
    /// Opaque sources are retained only in protected staging. This call never
    /// means the task or its history was adopted. Retry the same stage/chunk.
    pub fn stage_sources(
        &self,
        principal: &Principal,
        params: &org_proto::selected_tasks::staging::Params,
        timeout: Duration,
    ) -> Result<org_proto::selected_tasks::staging::Status, SubmissionFailure> {
        let claims = &principal.grant.claims;
        if claims.channel_id != self.channel
            || claims.org_id != self.organization
            || claims.office_id != self.office
            || !self
                .hello
                .supports(org_proto::selected_tasks::staging::CAPABILITY)
        {
            return Err(SubmissionFailure {
                may_have_submitted: false,
                error: ClientError::Protocol(
                    "selected staging channel unavailable or scope mismatch".into(),
                ),
            });
        }
        let mut sent = false;
        self.client
            .call_tracked_principal::<org_proto::selected_tasks::staging::Stage>(
                params,
                Some(principal),
                timeout,
                &mut sent,
            )
            .map_err(|error| SubmissionFailure {
                may_have_submitted: sent,
                error,
            })
    }
    pub fn adopt_task(
        &self,
        principal: &Principal,
        action: &org_proto::selected_tasks::adoption::Action,
        timeout: Duration,
    ) -> Result<org_proto::selected_tasks::adoption::Outcome, SubmissionFailure> {
        let claims = &principal.grant.claims;
        if claims.channel_id != self.channel
            || claims.org_id != self.organization
            || claims.office_id != self.office
            || !self
                .hello
                .supports(org_proto::selected_tasks::adoption::CAPABILITY)
        {
            return Err(SubmissionFailure {
                may_have_submitted: false,
                error: ClientError::Protocol(
                    "selected adoption channel unavailable or scope mismatch".into(),
                ),
            });
        }
        let mut sent = false;
        self.client
            .call_tracked_principal::<org_proto::selected_tasks::adoption::Adopt>(
                action,
                Some(principal),
                timeout,
                &mut sent,
            )
            .map_err(|error| SubmissionFailure {
                may_have_submitted: sent,
                error,
            })
    }
    pub fn instance(&self) -> &str {
        &self.instance
    }
    pub fn read(
        &self,
        principal: &Principal,
        query: &org_proto::selected_tasks::read::Query,
        timeout: Duration,
    ) -> Result<org_proto::selected_tasks::read::Result<i64>, ClientError> {
        let claims = &principal.grant.claims;
        if claims.channel_id != self.channel
            || claims.org_id != self.organization
            || claims.office_id != self.office
        {
            return Err(ClientError::Protocol("selected task scope mismatch".into()));
        }
        self.client
            .call_tracked_principal::<org_proto::selected_tasks::read::Read>(
                query,
                Some(principal),
                timeout,
                &mut false,
            )
    }
    pub fn set_assignment(
        &self,
        principal: &Principal,
        params: &NativeAssignment,
        timeout: Duration,
    ) -> Result<AssignmentResult<i64>, SubmissionFailure> {
        let claims = &principal.grant.claims;
        if claims.channel_id != self.channel
            || claims.org_id != self.organization
            || claims.office_id != self.office
            || !self.hello.supports(CAPABILITY)
        {
            return Err(SubmissionFailure {
                may_have_submitted: false,
                error: ClientError::Protocol(
                    "selected task channel unavailable or scope mismatch".into(),
                ),
            });
        }
        let mut sent = false;
        self.client
            .call_tracked_principal::<SetAssignment>(params, Some(principal), timeout, &mut sent)
            .map_err(|error| SubmissionFailure {
                may_have_submitted: sent,
                error,
            })
    }
    pub fn lookup(
        &self,
        principal: &Principal,
        operation_id: &str,
        timeout: Duration,
    ) -> Result<org_proto::selected_tasks::LookupOutcome<i64>, ClientError> {
        let claims = &principal.grant.claims;
        if claims.channel_id != self.channel
            || claims.org_id != self.organization
            || claims.office_id != self.office
            || org_proto::ulid_time_ms(operation_id).is_none()
        {
            return Err(ClientError::Protocol(
                "selected lookup scope mismatch".into(),
            ));
        }
        self.client
            .call_tracked_principal::<org_proto::selected_tasks::Lookup>(
                &org_proto::OperationGetParams {
                    kind: org_proto::OperationKind::Work,
                    id: operation_id.into(),
                },
                Some(principal),
                timeout,
                &mut false,
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use org_proto::{
        frame, Grant, GrantClaims, GrantMac, Hello, HelloResult, OfficeHello, OfficeInfo,
        OfficeRole, OfficeStatus, Request, Response,
    };
    use std::{io::BufReader, os::unix::net::UnixStream};
    const INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    fn principal() -> Principal {
        Principal {
            grant: Grant::new(
                GrantClaims {
                    account: "account".into(),
                    member: INSTANCE.into(),
                    org_id: "org".into(),
                    office_id: "office".into(),
                    channel_id: "channel".into(),
                    session_id: "session".into(),
                    exp: u64::MAX,
                },
                GrantMac::from_bytes([11; 32]),
            ),
        }
    }
    fn hello() -> HelloResult {
        HelloResult::office(
            "test",
            "This Mac",
            OfficeHello {
                organization_name: "Org".into(),
                office: OfficeInfo {
                    id: "office".into(),
                    name: "HQ".into(),
                    role: OfficeRole::Hq,
                    organization_id: "org".into(),
                    host: "This Mac".into(),
                    status: OfficeStatus::Active,
                    status_reason: None,
                    next_restart_at: None,
                    endpoint: None,
                },
            },
        )
        .with_instance_id(INSTANCE)
        .with_capabilities(vec![CAPABILITY.into()])
    }
    #[test]
    fn approved_descriptor_sends_complete_authenticated_pair_and_retains_uncertainty() {
        for lost in [false, true] {
            let (client, server) = UnixStream::pair().unwrap();
            let worker = std::thread::spawn(move || {
                let mut reader = BufReader::new(server);
                let request: Request = frame::read_message(&mut reader).unwrap().unwrap();
                assert!(request.is::<Hello>());
                let mut greeting = hello();
                greeting.api_version = 2;
                frame::write_message(
                    reader.get_mut(),
                    &Response::ok::<Hello>(request.id, &greeting).unwrap(),
                )
                .unwrap();
                let raw = frame::read_line(&mut reader).unwrap().unwrap();
                let request = org_proto::selected_tasks::decode_native_assignment(&raw).unwrap();
                assert_eq!(request.params.command.expected_revision.get(), i64::MAX);
                assert_eq!(
                    request.params.command.assigned_team_id.0.as_deref(),
                    Some("cross-home-team")
                );
                assert!(request.params.command.assignee_id.0.is_none());
                let principal: Principal =
                    serde_json::from_str(request.principal.raw().unwrap().get()).unwrap();
                assert_eq!(principal.grant.claims.channel_id, "channel");
                if !lost {
                    let result = AssignmentResult::Completed {
                        organization_id: "org".into(),
                        office_id: "office".into(),
                        task_id: "opaque-task".into(),
                        revision: i64::MAX,
                        assigned_team_id: Some("cross-home-team".into()),
                        assignee_id: None,
                        event_sha256: "a".repeat(64),
                    };
                    frame::write_message(
                        reader.get_mut(),
                        &Response::ok::<SetAssignment>(request.id, &result).unwrap(),
                    )
                    .unwrap();
                }
            });
            let approved = ApprovedOffice::from_descriptor(
                client.into(),
                INSTANCE.into(),
                "org".into(),
                "office".into(),
                "channel".into(),
            )
            .unwrap();
            let params:NativeAssignment=serde_json::from_value(serde_json::json!({"change_id":INSTANCE,"task_id":"opaque-task","command":{"expected_revision":i64::MAX,"assigned_team_id":"cross-home-team","assignee_id":null}})).unwrap();
            let result = approved.set_assignment(&principal(), &params, Duration::from_secs(3));
            if lost {
                assert!(result.unwrap_err().may_have_submitted);
            } else {
                assert!(matches!(
                    result.unwrap(),
                    AssignmentResult::Completed {
                        revision: i64::MAX,
                        ..
                    }
                ));
            }
            drop(approved);
            worker.join().unwrap();
        }
    }
}
