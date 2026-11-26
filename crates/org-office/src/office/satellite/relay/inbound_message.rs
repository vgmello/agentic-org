//! Applying an inbound relayed message: sender and conversation checks, local recipient
//! states and wakes, and conversation-only deltas.

use org_store::{Actor, Id};

use crate::office::satellite::placement::PlacementContext;

use super::inbound_ask::attach_relayed_ask_answer;
use super::types::{
    message_expires_at_ms, RelayedMessageOutcome, RelayedRecipientCtx, RelayedRecipientWake,
};

pub(super) struct ApplyRelayedMessageOp {
    pub(super) operation_id: Id,
    pub(super) here: org_store::OfficeIdentity,
    pub(super) owner: Id,
    pub(super) origin_office: String,
    pub(super) now_ms: u64,
    pub(super) ctx: PlacementContext,
    pub(super) incoming_conv: Option<crate::messaging::records::ConversationRecord>,
    pub(super) incoming_answer:
        Option<(Id, String, crate::messaging::records::ParticipantAnswerFile)>,
    pub(super) msg: crate::messaging::records::MessageRecord,
}

impl crate::store::open::StoreOp for ApplyRelayedMessageOp {
    type Out = Result<org_store::Reply, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        let op = org_store::Operation {
            id: self.operation_id.clone(),
            kind: "replication.apply_message",
            actor: Actor::System,
            fingerprint: org_store::Sha256::of_parts(&[
                self.msg.conversation_id.as_str().as_bytes(),
                b"\0",
                self.msg.id.as_bytes(),
                b"\0",
                self.msg.content_sha256.as_bytes(),
            ]),
        };
        store.submit(Some(op), false, move |uow| {
            decide_apply_relayed_message(uow, &self)
        })
    }
}

fn validate_relayed_sender(
    employees: &[crate::organization::records::Employee],
    placement: &PlacementContext,
    origin_office: &str,
    msg: &crate::messaging::records::MessageRecord,
) -> Result<(String, String, bool), org_store::Refusal> {
    use crate::organization::records::{EmployeeKind, EmployeeStatus};
    if msg.sender_kind == crate::messaging::records::SenderKind::Office {
        // Bind office-system sender identity strictly to the authenticated origin office (`TASK-52` P5-#5).
        if msg.sender_employee_id.is_some()
            || msg.sender_office_id.as_ref().map(Id::as_str) != Some(origin_office)
        {
            return Err(org_store::Refusal::invalid(
                "office-system sender_office_id does not match authenticated origin office",
            ));
        }
        return Ok((
            origin_office.to_owned(),
            msg.sender_display_name.clone(),
            false,
        ));
    }
    let Some(sid) = msg.sender_employee_id.as_ref() else {
        return Err(org_store::Refusal::invalid(
            "employee message missing sender_employee_id",
        ));
    };
    let Some(emp) = employees
        .iter()
        .find(|e| e.id == *sid && e.status == EmployeeStatus::Active)
    else {
        return Err(org_store::Refusal::invalid(
            "relayed message sender is not an active employee",
        ));
    };
    // Bind agent authors strictly to their placed office so satellite A cannot spoof an agent
    // placed on satellite B or HQ (`TASK-52` P4-#1).
    if emp.kind == EmployeeKind::Agent {
        let placed = placement
            .employee_offices
            .get(emp.id.as_str())
            .map(String::as_str)
            .unwrap_or(placement.hq_office_id.as_str());
        if placed != origin_office {
            return Err(org_store::Refusal::invalid(
                "relayed agent sender is not placed at authenticated origin office",
            ));
        }
    }
    Ok((
        emp.id.as_str().to_owned(),
        emp.name.clone(),
        emp.kind == EmployeeKind::Person,
    ))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConvWriteMode {
    None,
    Create,
    Update,
}

/// Validates the immutable identity of a deterministic automatic channel (`#general`, `#team-*`,
/// `#proj-*`) against the receiver's committed organization snapshot and derives its mutable
/// channel name from the receiver's current org/team/project record so a team rename while a
/// satellite is offline never strands its first automatic-channel message (`TASK-52` P12-#3 / P13-#5).
pub(super) fn canonicalize_deterministic_auto_channel(
    view: &impl crate::messaging::ports::MessagingRead,
    conv: &crate::messaging::records::ConversationRecord,
) -> Option<crate::messaging::records::ConversationRecord> {
    use crate::messaging::records::derive_conversation_id;
    use crate::organization::records::TeamStatus;
    use org_proto::chat::{AutoScope, ConversationKind};
    if conv.kind != ConversationKind::Channel
        || conv.archived
        || !conv.memberships.is_empty()
        || !conv.pins.is_empty()
    {
        return None;
    }
    let org = view.organization().ok()??;
    if conv.organization_id != org.id {
        return None;
    }
    let canonical_name = match conv.auto_scope {
        Some(AutoScope::Organization)
            if conv.id == derive_conversation_id("auto:org", org.id.as_str())
                && conv.team_id.is_none()
                && conv.project_id.is_none() =>
        {
            "general".to_owned()
        }
        Some(AutoScope::Team) => {
            let tid = conv.team_id.as_ref()?;
            if conv.project_id.is_some()
                || conv.id != derive_conversation_id("auto:team", tid.as_str())
            {
                return None;
            }
            let teams = view.teams().ok()?;
            let team = teams
                .iter()
                .find(|t| t.id == *tid && t.status == TeamStatus::Active)?;
            crate::messaging::features::slugify_channel(&team.name)
        }
        Some(AutoScope::Project) => {
            let pid = conv.project_id.as_ref()?;
            if conv.team_id.is_some()
                || conv.id != derive_conversation_id("auto:project", pid.as_str())
            {
                return None;
            }
            let projects = view.projects().ok()?;
            let project = projects.iter().find(|p| p.id == *pid && !p.archived)?;
            project.slug.clone()
        }
        _ => return None,
    };
    let mut canonical = conv.clone();
    canonical.name = Some(canonical_name);
    Some(canonical)
}

fn resolve_relayed_conversation(
    view: &impl crate::messaging::ports::MessagingRead,
    owner: &Id,
    origin_office: &str,
    hq_office_id: &str,
    incoming: Option<&crate::messaging::records::ConversationRecord>,
    msg: &crate::messaging::records::MessageRecord,
) -> Result<
    (
        crate::messaging::records::ConversationRecord,
        ConvWriteMode,
        Vec<Id>,
    ),
    org_store::Refusal,
> {
    let existing = view
        .conversation(&msg.conversation_id)
        .map_err(|_| org_store::Refusal::invalid("read conversation"))?;
    let (conv, write_mode) = match (existing, incoming) {
        (Some(local), Some(remote)) if local.kind != remote.kind => {
            return Err(org_store::Refusal::invalid(
                "relayed conversation kind mismatch",
            ));
        }
        // Adopt HQ-authoritative newer conversation revisions (e.g. channel membership changes,
        // `TASK-52` P5-#4) while preventing non-HQ peers from overwriting existing conversations.
        (Some(local), Some(remote))
            if origin_office == hq_office_id && remote.revision > local.revision =>
        {
            (remote.clone(), ConvWriteMode::Update)
        }
        (Some(local), _) => (local, ConvWriteMode::None),
        (None, Some(remote)) => {
            // Enforce single HQ channel ownership for custom channels while allowing satellites
            // to materialize deterministic auto-channels with canonical receiver-derived names
            // (`TASK-52` P8-#6 / P12-#3 / P13-#5).
            if remote.kind == org_proto::chat::ConversationKind::Channel
                && origin_office != hq_office_id
            {
                let Some(canonical) = canonicalize_deterministic_auto_channel(view, remote) else {
                    return Err(org_store::Refusal::invalid(
                        "only HQ may create relayed custom channels",
                    ));
                };
                (canonical, ConvWriteMode::Create)
            } else {
                (remote.clone(), ConvWriteMode::Create)
            }
        }
        (None, None) => {
            return Err(org_store::Refusal::invalid(
                "conversation missing for relayed message",
            ));
        }
    };
    // Reuse native team/project/org/explicit membership rules (`TASK-52` P4-#2).
    let active_members = crate::messaging::features::effective_auto_members(view, &conv, owner)
        .map_err(|_| org_store::Refusal::invalid("resolve conversation members"))?;
    if msg
        .sender_employee_id
        .as_ref()
        .is_some_and(|sid| !active_members.contains(sid))
    {
        return Err(org_store::Refusal::invalid(
            "relayed message sender is not an active conversation member",
        ));
    }
    Ok((conv, write_mode, active_members))
}

struct RelayedRecipientDeliveries {
    local_agent_recipients: Vec<String>,
    local_recipient_wakes: Vec<RelayedRecipientWake>,
    local_delivered_headers: Vec<org_proto::msg::MessageHeader>,
}

fn relayed_wake_kind_for_message(
    conv_kind: org_proto::chat::ConversationKind,
    msg: &crate::messaging::records::MessageRecord,
    target_id: &str,
    delivered_because: org_proto::chat::DeliveredBecause,
    is_mentioned: bool,
    open_question_asker_id: Option<&str>,
) -> Option<&'static str> {
    use org_proto::chat::{ConversationKind, DeliveredBecause};
    if msg.kind == "announcement" {
        return None;
    }
    if msg.kind == org_proto::msg::KIND_REQUEST {
        return Some("ask");
    }
    if msg.kind == "handoff"
        && (delivered_because == DeliveredBecause::Addressed
            || matches!(conv_kind, ConversationKind::Dm | ConversationKind::GroupDm))
    {
        return Some("handoff");
    }
    if open_question_asker_id == Some(target_id) {
        return Some("ask_answer");
    }
    if delivered_because == DeliveredBecause::Mention || is_mentioned {
        return Some("mention");
    }
    if matches!(conv_kind, ConversationKind::Dm | ConversationKind::GroupDm) {
        return Some("direct_message");
    }
    None
}

fn attach_relayed_recipient_states(
    view: &impl crate::messaging::ports::MessagingRead,
    ctx: &RelayedRecipientCtx<'_>,
    mut change: org_store::Change,
) -> Result<(org_store::Change, RelayedRecipientDeliveries), org_store::Refusal> {
    use crate::messaging::features::{
        build_mention_candidates, collect_recipient_reasons, to_wire_header,
        upsert_participant_snapshot,
    };
    use crate::messaging::mentions::extract_mentions;
    use crate::messaging::records::RecipientStateRecord;
    use crate::organization::records::EmployeeKind;

    let sender_id = ctx.msg.sender_employee_id.as_ref().unwrap_or(ctx.owner);
    let candidates = build_mention_candidates(ctx.employees, ctx.active_members, ctx.owner);
    let scan = extract_mentions(&ctx.msg.body_markdown, &candidates);
    let reasons = collect_recipient_reasons(
        view,
        ctx.conv,
        sender_id,
        ctx.active_members,
        &scan.resolved,
        ctx.msg.thread_root_id.as_deref(),
        &ctx.msg.kind,
        &ctx.msg.metadata,
    )?;
    let open_question_asker_id = ctx
        .msg
        .thread_root_id
        .as_deref()
        .and_then(|root_id| view.message(root_id).ok().flatten())
        .filter(|root| {
            root.kind == org_proto::msg::KIND_REQUEST
                && crate::messaging::asks::is_open_ask_card(view, root)
        })
        .and_then(|root| root.sender_employee_id.map(|id| id.as_str().to_owned()));
    let local_seq = view
        .message_local_seq(&ctx.msg.id)
        .ok()
        .flatten()
        .unwrap_or_else(|| {
            view.max_local_seq()
                .ok()
                .map_or(ctx.msg.origin_sequence, |s| s.saturating_add(1))
        });
    let mut local_agent_recipients = Vec::new();
    let mut local_recipient_wakes = Vec::new();
    let mut local_delivered_headers = Vec::new();
    for (emp_id, because) in reasons {
        // Commit inbox recipient state ONLY at the single office owning that recipient so multiple
        // offices never create diverging inbox copies for the same employee (`TASK-52` P4-#3).
        let owner_office = ctx
            .placement
            .employee_offices
            .get(emp_id.as_str())
            .map(String::as_str)
            .unwrap_or(ctx.placement.hq_office_id.as_str());
        if owner_office != ctx.here.office_id.as_str() {
            continue;
        }
        let emp = ctx.employees.iter().find(|e| e.id == emp_id);
        let display_name = emp.map(|e| e.name.clone());
        if emp.is_some_and(|e| e.kind == EmployeeKind::Agent) {
            let is_mentioned = scan
                .resolved
                .iter()
                .any(|m| m.employee_id == emp_id.as_str());
            if let Some(wake_kind) = relayed_wake_kind_for_message(
                ctx.conv.kind,
                ctx.msg,
                emp_id.as_str(),
                because,
                is_mentioned,
                open_question_asker_id.as_deref(),
            ) {
                local_agent_recipients.push(emp_id.as_str().to_owned());
                local_recipient_wakes.push(RelayedRecipientWake {
                    employee_id: emp_id.as_str().to_owned(),
                    kind: wake_kind.to_owned(),
                });
            }
        }
        let state = RecipientStateRecord {
            organization_id: ctx.here.organization_id.clone(),
            message_id: ctx.msg.id.clone(),
            conversation_id: ctx.conv.id.clone(),
            employee_id: emp_id.clone(),
            recipient_display_name: display_name,
            state: org_proto::msg::MessageLifecycleState::Unread,
            triage_generation: 0,
            snoozed_until: None,
            snooze_after_task: false,
            snooze_note: None,
            triaged_at: None,
            read_at: None,
            updated_at: ctx.msg.created_at.clone(),
            reason: None,
            delivered_because: because,
            related_message_ids: Vec::new(),
            expired_waiting_person: false,
            owner_office_id: ctx.here.office_id.clone(),
            origin_incarnation: ctx.msg.origin_incarnation.clone(),
            origin_sequence: ctx.msg.origin_sequence,
            org_epoch: ctx.msg.org_epoch,
            org_sequence: ctx.msg.org_sequence,
        };
        local_delivered_headers.push(to_wire_header(ctx.msg, Some(&state), local_seq));
        change = upsert_participant_snapshot(
            view,
            &ctx.here.organization_id,
            &ctx.conv.id,
            &emp_id,
            state,
            change,
        )?;
    }
    Ok((
        change,
        RelayedRecipientDeliveries {
            local_agent_recipients,
            local_recipient_wakes,
            local_delivered_headers,
        },
    ))
}

fn decide_apply_relayed_message<E: crate::store::open::OfficeEngine>(
    uow: &mut org_store::UnitOfWork<'_, E>,
    req: &ApplyRelayedMessageOp,
) -> Result<org_store::Change, org_store::Refusal> {
    use crate::messaging::ports::MessagingRead;
    use crate::messaging::records::{conversation_path, encode_json, message_path};
    use crate::organization::ports::OrganizationRead;
    use org_store::{Ack, Change, Refusal};

    if message_expires_at_ms(&req.msg).is_some_and(|exp| exp <= req.now_ms) {
        return Err(Refusal::invalid("relayed message already expired"));
    }
    let view = uow.view();
    let owner = crate::organization::owner::effective_owner_id(view, &req.owner)
        .map_err(|_| Refusal::invalid("resolve organization owner"))?;
    let employees = view
        .employees()
        .map_err(|_| Refusal::invalid("read employees"))?;
    let (sender_id, sender_name, sender_is_person) =
        validate_relayed_sender(&employees, &req.ctx, &req.origin_office, &req.msg)?;
    let (conv, write_mode, active_members) = resolve_relayed_conversation(
        view,
        &owner,
        &req.origin_office,
        &req.ctx.hq_office_id,
        req.incoming_conv.as_ref(),
        &req.msg,
    )?;
    let rctx = RelayedRecipientCtx {
        here: &req.here,
        owner: &owner,
        placement: &req.ctx,
        employees: &employees,
        conv: &conv,
        active_members: &active_members,
        msg: &req.msg,
    };
    if let Some(existing_msg) = view
        .message(&req.msg.id)
        .map_err(|_| Refusal::invalid("read existing message"))?
    {
        if existing_msg.content_sha256 != req.msg.content_sha256 {
            return Err(Refusal::invalid("relayed message content_sha256 conflict"));
        }
        let (_, answered_ask, closed_ask_id) = attach_relayed_ask_answer(
            view,
            &rctx,
            req.incoming_answer.as_ref(),
            Change::new(Ack::Durable),
        )?;
        let (_, delivered) =
            attach_relayed_recipient_states(view, &rctx, Change::new(Ack::Durable))?;
        let outcome = RelayedMessageOutcome {
            message_id: req.msg.id.clone(),
            conversation_id: conv.id.as_str().to_owned(),
            sender_id,
            sender_name,
            sender_is_person,
            is_dm: conv.kind == org_proto::chat::ConversationKind::Dm,
            inserted: false,
            conversation_kind: Some(conv.kind),
            summary: relayed_wake_summary(&req.msg.body_markdown, &req.msg.id),
            local_agent_recipients: delivered.local_agent_recipients,
            local_recipient_wakes: delivered.local_recipient_wakes,
            local_delivered_headers: delivered.local_delivered_headers,
            answered_ask,
            withdrawn_ask: None,
            closed_ask_reason: closed_ask_id.as_ref().map(|_| "answered".to_owned()),
            closed_ask_id,
            handoff_accepted: false,
            delivered_message: None,
        };
        let reply = serde_json::to_vec(&outcome).unwrap_or_default();
        return Ok(Change::new(Ack::Durable).reply(reply));
    }
    let mut change = Change::new(Ack::Durable);
    match write_mode {
        ConvWriteMode::Create => {
            change = change.create(conversation_path(&conv.id), encode_json(&conv));
        }
        ConvWriteMode::Update => {
            change = change.put(conversation_path(&conv.id), encode_json(&conv));
        }
        ConvWriteMode::None => {}
    }
    change = change.create(message_path(&conv.id, &req.msg.id), encode_json(&req.msg));
    let (change, answered_ask, closed_ask_id) =
        attach_relayed_ask_answer(view, &rctx, req.incoming_answer.as_ref(), change)?;
    let (change, delivered) = attach_relayed_recipient_states(view, &rctx, change)?;
    let outcome = RelayedMessageOutcome {
        message_id: req.msg.id.clone(),
        conversation_id: conv.id.as_str().to_owned(),
        sender_id,
        sender_name,
        sender_is_person,
        is_dm: conv.kind == org_proto::chat::ConversationKind::Dm,
        inserted: true,
        conversation_kind: Some(conv.kind),
        summary: relayed_wake_summary(&req.msg.body_markdown, &req.msg.id),
        local_agent_recipients: delivered.local_agent_recipients,
        local_recipient_wakes: delivered.local_recipient_wakes,
        local_delivered_headers: delivered.local_delivered_headers,
        answered_ask,
        withdrawn_ask: None,
        closed_ask_reason: closed_ask_id.as_ref().map(|_| "answered".to_owned()),
        closed_ask_id,
        handoff_accepted: false,
        delivered_message: None,
    };
    let reply = serde_json::to_vec(&outcome).unwrap_or_default();
    Ok(change.reply(reply))
}

pub(super) const MAX_RELAYED_WAKE_SUMMARY_CHARS: usize = 4096;

pub(super) fn relayed_wake_summary(body: &str, message_id: &str) -> String {
    let mut chars = body.chars();
    let prefix: String = chars
        .by_ref()
        .take(MAX_RELAYED_WAKE_SUMMARY_CHARS)
        .collect();
    if chars.next().is_some() {
        format!(
            "{prefix}\n...[truncated; run `agent-os msg read --id {message_id}` for the full body]"
        )
    } else {
        prefix
    }
}

pub(super) struct ApplyRelayedConversationOnlyOp {
    pub(super) operation_id: Id,
    pub(super) origin_office: String,
    pub(super) ctx: PlacementContext,
    pub(super) conv: crate::messaging::records::ConversationRecord,
}

impl crate::store::open::StoreOp for ApplyRelayedConversationOnlyOp {
    type Out = Result<org_store::Reply, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        use crate::messaging::ports::MessagingRead;
        use crate::messaging::records::{conversation_path, encode_json};
        use org_store::{Ack, Change, Refusal};
        let op = org_store::Operation {
            id: self.operation_id,
            kind: "replication.apply_conversation",
            actor: Actor::System,
            fingerprint: org_store::Sha256::of_parts(&[
                self.conv.id.as_str().as_bytes(),
                b"\0",
                &self.conv.revision.to_le_bytes(),
            ]),
        };
        let origin = self.origin_office;
        let ctx = self.ctx;
        let conv = self.conv;
        store.submit(Some(op), false, move |uow| {
            let existing = uow
                .view()
                .conversation(&conv.id)
                .map_err(|_| Refusal::invalid("read conversation"))?;
            let is_channel = conv.kind == org_proto::chat::ConversationKind::Channel;
            let origin_authorized_dm = !is_channel
                && conv.memberships.iter().any(|p| {
                    p.left_at.is_none()
                        && ctx
                            .employee_offices
                            .get(p.employee_id.as_str())
                            .map(String::as_str)
                            .unwrap_or(ctx.hq_office_id.as_str())
                            == origin.as_str()
                });
            let change = match existing {
                Some(local) if local.kind != conv.kind => {
                    return Err(Refusal::invalid("conversation kind mismatch"));
                }
                Some(local)
                    if is_channel
                        && origin == ctx.hq_office_id
                        && conv.revision > local.revision =>
                {
                    Change::new(Ack::Durable).put(conversation_path(&conv.id), encode_json(&conv))
                }
                Some(_) => Change::new(Ack::Durable),
                None if is_channel && origin == ctx.hq_office_id => Change::new(Ack::Durable)
                    .create(conversation_path(&conv.id), encode_json(&conv)),
                None if is_channel => {
                    let Some(canonical) =
                        canonicalize_deterministic_auto_channel(uow.view(), &conv)
                    else {
                        return Err(Refusal::invalid(
                            "origin office lacks authority to create relayed custom channel",
                        ));
                    };
                    Change::new(Ack::Durable)
                        .create(conversation_path(&canonical.id), encode_json(&canonical))
                }
                None if origin_authorized_dm => Change::new(Ack::Durable)
                    .create(conversation_path(&conv.id), encode_json(&conv)),
                None => {
                    return Err(Refusal::invalid(
                        "origin office lacks authority to create relayed conversation",
                    ));
                }
            };
            Ok(change)
        })
    }
}
