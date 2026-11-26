//! Composition of the socket's office ports: reads, submits, hiring, messaging and the per-domain
//! RPC dispatchers.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::failure::{logged, unsupported};
use crate::federation_runtime::HqFederationSlot;
use crate::session_proxy::build_session_hooks;
use crate::{
    chat_calls, cost_calls, directory, federation_calls, follow, git_calls, live, mailbox_store,
    org_feed, process_calls, provider_calls, serve, task_calls, wake_bridge, watcher,
};
use org_office::{Office, OfficeError};

/// Preserve the organization's retryable availability failure across the read port.
fn snapshot_failure(error: &OfficeError) -> serve::SnapshotFailure {
    logged("snapshot", error);
    match error {
        OfficeError::SnapshotPending => serve::SnapshotFailure::Unavailable,
        _ => serve::SnapshotFailure::Storage,
    }
}

pub(crate) struct MessagingPorts {
    directory: Arc<dyn org_mailbox::MemberDirectory + Send + Sync>,
    pub(crate) mailbox: Arc<org_mailbox::Mailbox>,
    pub(crate) chat_svc: Arc<chat_calls::ChatService>,
    chat: serve::ChatFn,
    feed: Box<dyn follow::OrgFeed>,
}

pub(crate) fn build_messaging_ports(
    office: &Arc<Office>,
    live: &Arc<live::LiveEvents>,
    wake: &Arc<org_feed::FeedWake>,
    session_manager: &Arc<org_office_sessions::SessionManager>,
    channel_watcher: &Arc<watcher::ChannelWatcher>,
    sync_wake: Arc<dyn Fn() + Send + Sync>,
) -> MessagingPorts {
    let directory: Arc<dyn org_mailbox::MemberDirectory + Send + Sync> =
        Arc::new(directory::OfficeDirectory::of(Arc::clone(office)));
    let make_id = || org_mailbox::OfficeIdentity {
        office_id: office.info().office_id.clone(),
        incarnation: office.journal_incarnation().to_owned(),
        is_hq: matches!(office.is_hq(), Ok(true)),
    };
    let store = Arc::new(mailbox_store::OfficeMailboxStore::new(Arc::clone(office)));
    let mailbox = match org_mailbox::Mailbox::with_triage(
        Arc::clone(&store) as Arc<dyn org_mailbox::MessageStore + Send + Sync>,
        store as Arc<dyn org_mailbox::TriageStore + Send + Sync>,
        Arc::clone(&directory),
        make_id(),
        Arc::new(org_mailbox::SystemClock),
        Arc::clone(live) as Arc<dyn org_mailbox::MessageEvents>,
    ) {
        Ok(mb) => Arc::new(mb),
        Err(error) => {
            eprintln!("agentic-org-office: snooze timer seed failed: {error}");
            Arc::new(org_mailbox::Mailbox::new(
                Arc::new(mailbox_store::OfficeMailboxStore::new(Arc::clone(office))),
                Arc::clone(&directory),
                make_id(),
                Arc::new(org_mailbox::SystemClock),
                Arc::clone(live) as Arc<dyn org_mailbox::MessageEvents>,
            ))
        }
    };
    let chat_svc = Arc::new(
        chat_calls::ChatService::with_watcher(
            Arc::clone(office),
            Arc::clone(live),
            Arc::clone(&mailbox),
            Some(Arc::clone(session_manager)),
            Arc::clone(channel_watcher),
        )
        .with_sat_sync_wake(sync_wake)
        .with_feed_wake(Arc::clone(wake)),
    );
    let chat_for_dispatch = Arc::clone(&chat_svc);
    let chat: serve::ChatFn = Box::new(move |member, via_grant, stopping, request| {
        chat_for_dispatch.dispatch(member, via_grant, stopping, request)
    });
    let feed: Box<dyn follow::OrgFeed> = Box::new(org_feed::OfficeOrgFeed::new(
        Arc::clone(office),
        Arc::clone(wake),
    ));
    MessagingPorts {
        directory,
        mailbox,
        chat_svc,
        chat,
        feed,
    }
}

fn snapshot_pending(office: &Office) -> bool {
    office
        .sync_status()
        .is_ok_and(|s| s.initial_snapshot_required)
}

fn snapshot_unavailable_api_error() -> org_proto::ApiError {
    org_proto::ApiError::new(
        org_proto::ErrorCode::Unavailable,
        "organization snapshot is not available yet",
    )
}

fn gate_rpc_port<F>(office: &Arc<Office>, inner: F) -> serve::HiringFn
where
    F: Fn(Option<&str>, &org_proto::Request) -> Option<org_proto::Response> + Send + Sync + 'static,
{
    let office = Arc::clone(office);
    Box::new(move |member, request| {
        if snapshot_pending(&office) {
            return Some(org_proto::Response::error(
                request.id,
                snapshot_unavailable_api_error(),
            ));
        }
        inner(member, request)
    })
}

fn build_hiring_port(
    office: &Arc<Office>,
    wake: &Arc<org_feed::FeedWake>,
    session_manager: &Arc<org_office_sessions::SessionManager>,
    channel_watcher: &Arc<watcher::ChannelWatcher>,
    sync_wake: Arc<dyn Fn() + Send + Sync>,
) -> serve::HiringFn {
    let office = Arc::clone(office);
    let wake = Arc::clone(wake);
    let sessions = Arc::clone(session_manager);
    let watcher = Arc::clone(channel_watcher);
    Box::new(move |member, request| {
        if snapshot_pending(&office) {
            return Some(org_proto::Response::error(
                request.id,
                snapshot_unavailable_api_error(),
            ));
        }
        let response = office.dispatch_hiring_rpc(member, request);
        if matches!(
            response.as_ref().map(|r| &r.outcome),
            Some(org_proto::Outcome::Result(_))
        ) {
            wake.notify();
            sync_wake();
            notify_agent_lifecycle(&sessions, &watcher, request);
        }
        response
    })
}

fn notify_agent_lifecycle(
    sessions: &org_office_sessions::SessionManager,
    watcher: &watcher::ChannelWatcher,
    request: &org_proto::Request,
) {
    if request.is::<org_proto::hiring::AgentSetStatus>() {
        if let Ok(params) = request.params::<org_proto::hiring::AgentSetStatus>() {
            if !params.paused {
                sessions.on_agent_resumed(&params.member_id);
            }
        }
    } else if request.is::<org_proto::hiring::AgentOffboard>() {
        if let Ok(params) = request.params::<org_proto::hiring::AgentOffboard>() {
            sessions.on_agent_offboarded(&params.member_id);
            watcher.on_agent_offboarded(&params.member_id);
        }
    } else if request.is::<org_proto::hiring::AgentMove>() {
        if let Ok(params) = request.params::<org_proto::hiring::AgentMove>() {
            watcher.on_agent_team_moved(&params.member_id);
        }
    }
}

fn notify_org_command_lifecycle(
    sessions: &org_office_sessions::SessionManager,
    watcher: &watcher::ChannelWatcher,
    command: &org_office::rules::OrgCommand,
) {
    match command {
        org_office::rules::OrgCommand::MemberSetAgentStatus(params)
            if params.status == org_proto::AgentStatus::Active =>
        {
            sessions.on_agent_resumed(&params.member_id);
        }
        org_office::rules::OrgCommand::MemberOffboard(params) => {
            sessions.on_agent_offboarded(&params.member_id);
            watcher.on_agent_offboarded(&params.member_id);
        }
        _ => {}
    }
}

fn build_submit_port(
    office: &Arc<Office>,
    wake: &Arc<org_feed::FeedWake>,
    session_manager: &Arc<org_office_sessions::SessionManager>,
    channel_watcher: &Arc<watcher::ChannelWatcher>,
    sync_wake: Arc<dyn Fn() + Send + Sync>,
) -> serve::SubmitFn {
    let office = Arc::clone(office);
    let wake = Arc::clone(wake);
    let sessions = Arc::clone(session_manager);
    let watcher = Arc::clone(channel_watcher);
    Box::new(move |member, command, dry_run| {
        let answer = if dry_run {
            office.check_org(member, command)
        } else {
            let lifecycle_cmd = match &command {
                org_office::rules::OrgCommand::MemberSetAgentStatus(p) => Some(
                    org_office::rules::OrgCommand::MemberSetAgentStatus(p.clone()),
                ),
                org_office::rules::OrgCommand::MemberOffboard(p) => {
                    Some(org_office::rules::OrgCommand::MemberOffboard(p.clone()))
                }
                _ => None,
            };
            let res = office.submit_org(member, command);
            if matches!(res, Ok(Ok(_))) {
                wake.notify();
                sync_wake();
                if let Some(cmd) = lifecycle_cmd.as_ref() {
                    notify_org_command_lifecycle(&sessions, &watcher, cmd);
                }
            }
            res
        };
        answer.map_err(|e| logged("command", &e))
    })
}

struct ReadPorts {
    snapshot: serve::SnapshotFn,
    projects_list: serve::ProjectsListFn,
    project_inspect: serve::ProjectInspectFn,
    who: serve::WhoFn,
    status: serve::StatusFn,
}

fn build_read_ports(
    office: &Arc<Office>,
    session_manager: &Arc<org_office_sessions::SessionManager>,
    live: &Arc<live::LiveEvents>,
) -> ReadPorts {
    let o_snap = Arc::clone(office);
    let o_list = Arc::clone(office);
    let o_insp = Arc::clone(office);
    let o_who = Arc::clone(office);
    let s_who = Arc::clone(session_manager);
    let o_stat = Arc::clone(office);
    let l_stat = Arc::clone(live);
    ReadPorts {
        snapshot: Box::new(move || {
            if snapshot_pending(&o_snap) {
                return Err(serve::SnapshotFailure::Unavailable);
            }
            o_snap.org_snapshot().map_err(|e| snapshot_failure(&e))
        }),
        projects_list: Box::new(move |params| {
            if snapshot_pending(&o_list) {
                return Ok(Err(snapshot_unavailable_api_error()));
            }
            o_list
                .list_projects(params.include_archived)
                .map(Ok)
                .map_err(|e| logged("project.list", &e))
        }),
        project_inspect: Box::new(move |params| {
            if snapshot_pending(&o_insp) {
                return Ok(Err(snapshot_unavailable_api_error()));
            }
            o_insp
                .inspect_project(&params.project)
                .map_err(|e| logged("project.inspect", &e))
        }),
        who: Box::new(move |params| {
            if snapshot_pending(&o_who) {
                return Ok(Err(snapshot_unavailable_api_error()));
            }
            let overlay = wake_bridge::SessionRuntimeOverlay::new(Arc::clone(&s_who));
            o_who
                .who_with_runtime(params, &overlay)
                .map_err(|e| logged("who", &e))
        }),
        status: Box::new(
            move |member, params| match o_stat.set_status(member, params) {
                Ok((result, changed)) => {
                    l_stat.publish_presence(changed);
                    Ok(Ok(result))
                }
                Err(refusal) => Ok(Err(refusal)),
            },
        ),
    }
}

/// Composes the office RPC ports passed to `serve::Shared`.
#[expect(
    clippy::too_many_arguments,
    reason = "composes the office's domain ports, federation handle, and sync wakeups"
)]
pub(crate) fn build_office_ports(
    office: Arc<Office>,
    session_manager: Arc<org_office_sessions::SessionManager>,
    channel_watcher: Arc<watcher::ChannelWatcher>,
    live: Arc<live::LiveEvents>,
    wake: &Arc<org_feed::FeedWake>,
    messaging: MessagingPorts,
    hq_federation_listening: Arc<AtomicBool>,
    hq_fed_slot: HqFederationSlot,
    sync_wake: Arc<dyn Fn() + Send + Sync>,
) -> serve::OfficePorts {
    let session_hooks = build_session_hooks(&office, &session_manager, &hq_fed_slot);
    let hiring = build_hiring_port(
        &office,
        wake,
        &session_manager,
        &channel_watcher,
        Arc::clone(&sync_wake),
    );
    let submit = build_submit_port(
        &office,
        wake,
        &session_manager,
        &channel_watcher,
        Arc::clone(&sync_wake),
    );
    let reads = build_read_ports(&office, &session_manager, &live);
    let task_svc = Arc::new(
        task_calls::TaskService::new(
            Arc::clone(&office),
            Arc::clone(&live),
            Some(Arc::clone(&session_manager)),
        )
        .with_sync_wake(sync_wake),
    );
    let git_svc = Arc::new(git_calls::GitService::new(
        Arc::clone(&office),
        Arc::clone(&live),
    ));
    serve::OfficePorts {
        work: Box::new(|_, _| Ok(Err(unsupported("work", "B4")))),
        operation_get: {
            let office = Arc::clone(&office);
            Box::new(move |member, id| {
                office
                    .operation(member, id)
                    .map_err(|e| logged("operation.get", &e))
            })
        },
        snapshot: reads.snapshot,
        projects_list: reads.projects_list,
        project_inspect: reads.project_inspect,
        hiring: Some(hiring),
        sessions: Some(session_hooks.sessions),
        run_binding_verify: Some(session_hooks.run_binding_verify),
        run_binding_active: Some(session_hooks.run_binding_active),
        delegation: Some(crate::session_proxy::chief_of_staff_delegation(
            &office,
            &session_manager,
        )),
        external_auth: Some(Arc::new(crate::session_proxy::ExternalAuth(Arc::clone(
            &session_manager,
        )))),
        record_agent_command: Some(session_hooks.record_agent_command),
        waiting_messages_counter: {
            let sessions = Arc::clone(&session_manager);
            Some(Box::new(move |member| {
                sessions.waiting_messages_counter(member)
            }))
        },
        on_agent_stop: Some(session_hooks.on_agent_stop),
        hr_message_guard: {
            let office = Arc::clone(&office);
            Some(Box::new(move |sender, target| {
                office
                    .check_hr_message_allowed(sender, target)
                    .map_err(|e| logged("msg.send", &e))
            }))
        },
        who: reads.who,
        status: reads.status,
        live: Arc::clone(&live),
        feed: Some(messaging.feed),
        mailbox: Some(messaging.mailbox),
        chat: Some(messaging.chat),
        tasks: Some(Box::new(move |member, request| {
            task_svc.dispatch(member, request)
        })),
        git: Some(gate_rpc_port(&office, move |member, request| {
            git_svc.dispatch(member, request)
        })),
        processes: {
            let o = Arc::clone(&office);
            Some(gate_rpc_port(&office, move |member, request| {
                process_calls::dispatch_process_rpc(&o, member, request)
            }))
        },
        costs: {
            let o = Arc::clone(&office);
            let sessions = Arc::clone(&session_manager);
            let live = Arc::clone(&live);
            Some(gate_rpc_port(&office, move |member, request| {
                cost_calls::dispatch_cost_budget_rpc(&o, Some(&sessions), &live, member, request)
            }))
        },
        providers: {
            let o = Arc::clone(&office);
            let sessions = Arc::clone(&session_manager);
            let wake = Arc::clone(wake);
            Some(gate_rpc_port(&office, move |member, request| {
                provider_calls::dispatch_provider_pool_rpc(
                    &o,
                    Some(&sessions),
                    Some(&wake),
                    member,
                    request,
                )
            }))
        },
        federation: {
            let office = Arc::clone(&office);
            let hq_listening = Arc::clone(&hq_federation_listening);
            Some(Box::new(move |member, request| {
                federation_calls::dispatch_federation_rpc(
                    &office,
                    member,
                    request,
                    hq_listening.load(Ordering::Acquire),
                )
            }))
        },
        directory: messaging.directory,
        submit,
    }
}
