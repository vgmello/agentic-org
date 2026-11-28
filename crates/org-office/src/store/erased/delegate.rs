//! Delegating read and write port implementations for [`BoxedTx`] and [`BoxedSnapshot`].

use std::collections::{BTreeMap, BTreeSet};

use org_docs::delivery::records::{
    DeploymentRecord, EntityLinkRecord, FeatureFlagRecord, FeatureRecord, LinkEntityKind,
    MilestoneRecord, NumberCounterRecord, ReleaseRecord, ScopedChangelogRecord,
};
use org_proto::attachments::AttachmentTargetKind;
use org_store::{CorePort, CoreRead, Cursor, DurablePath, EngineError, Id, Mark, Receipt, Sha256};

use crate::attachments::{
    ports::{AttachmentsRead, AttachmentsWrite},
    records::AttachmentRecord,
};
use crate::delivery::ports::{DeliveryRead, DeliveryWrite};
use crate::messaging::{
    ports::{BoundMessageFile, MessagingRead, MessagingWrite},
    records::{
        ConversationPreferenceRecord, ConversationReadMarkerRecord, ConversationRecord,
        MessageReactionRecord, MessageRecord, ParticipantAnswerFile, RecipientStateRecord,
    },
};
use crate::organization::{
    ports::{OrganizationRead, OrganizationWrite, RecordRef, TeamHomedMove},
    records::{
        AgentExecutionConfig, Employee, HireRequest, ModelProfile, Office, OfficeCode,
        Organization, ProviderConnection, Repository, Role, Team, TeamAllocation, TeamCounter,
        TeamLimitRequest, WatchingModelSetting,
    },
};
use crate::processes::{
    ports::{ProcessesRead, ProcessesWrite},
    records::StructureDocument,
};
use crate::project_docs::{
    ports::{ProjectDocsRead, ProjectDocsWrite},
    records::{DocEntry, DocMedia, ProjectDoc},
};
use crate::projects::{
    ports::{ProjectsRead, ProjectsWrite},
    records::{Project, ProjectAllocation, ProjectCounter},
};
use crate::reviews::{
    ports::{ReviewListQuery, ReviewsRead, ReviewsWrite},
    records::{
        OfficeReviewCounterRecord, ReviewDocument, ReviewEventRecord, ReviewFindingRecord,
        ReviewFindingReplyRecord, ReviewRecord, ReviewReviewerRecord, ReviewRoundRecord,
    },
};
use crate::search::{
    plan::SearchPlan,
    ports::{MessageScope, SearchRead},
    rank::SourcePage,
};
use crate::tasks::{
    ports::{TasksRead, TasksWrite},
    query::{TaskHistoryOutline, TaskListPage, TaskListQuery, TaskRollup},
    records::{
        ChecklistStepRecord, OfficeTaskCounterRecord, TaskCommentRecord, TaskEventRecord,
        TaskLinkRecord, TaskRecord, TaskWatcherRecord,
    },
};

use super::{BoxedSnapshot, BoxedTx};

macro_rules! fwd_read_one {
    ($trait:ident for $ty:ty { $($name:ident($($arg:ident: $arg_ty:ty),*) -> $ret:ty;)* }) => {
        impl $trait for $ty {
            $(fn $name(&self $(, $arg: $arg_ty)*) -> Result<$ret, EngineError> {
                self.0.$name($($arg),*)
            })*
        }
    };
}

macro_rules! fwd_read {
    ($trait:ident for $ty1:ty, $ty2:ty { $($body:tt)* }) => {
        fwd_read_one!($trait for $ty1 { $($body)* });
        fwd_read_one!($trait for $ty2 { $($body)* });
    };
    ($trait:ident for $ty:ty { $($body:tt)* }) => {
        fwd_read_one!($trait for $ty { $($body)* });
    };
}

macro_rules! fwd_write {
    ($trait:ident { $($name:ident($($arg:ident: $arg_ty:ty),*) -> $ret:ty;)* }) => {
        impl $trait for BoxedTx {
            $(fn $name(&mut self $(, $arg: $arg_ty)*) -> Result<$ret, EngineError> {
                self.0.$name($($arg),*)
            })*
        }
    };
}

fwd_read!(CoreRead for BoxedTx, BoxedSnapshot {
    cursor() -> Option<Cursor>;
    source_hash(path: &DurablePath) -> Option<Sha256>;
    sources_under(prefix: &DurablePath) -> Vec<DurablePath>;
    receipt(id: &Id) -> Option<Receipt>;
    list_receipts_by_prefix(prefix: &str) -> Vec<Receipt>;
});

fwd_read!(OrganizationRead for BoxedTx, BoxedSnapshot {
    generation() -> u64;
    organization() -> Option<Organization>;
    repositories() -> Vec<Repository>;
    office(id: &Id) -> Option<Office>;
    offices() -> Vec<Office>;
    office_codes() -> Vec<OfficeCode>;
    office_code(office: &Id) -> Option<OfficeCode>;
    role(id: &Id) -> Option<Role>;
    roles() -> Vec<Role>;
    team(id: &Id) -> Option<Team>;
    team_by_number(number: u64) -> Option<Team>;
    teams() -> Vec<Team>;
    team_chain(id: &Id) -> Option<Vec<Id>>;
    employee(id: &Id) -> Option<Employee>;
    employees() -> Vec<Employee>;
    handle_in_use(handle: &str) -> bool;
    hire_request(id: &Id) -> Option<HireRequest>;
    hire_requests() -> Vec<HireRequest>;
    team_limit_request(id: &Id) -> Option<TeamLimitRequest>;
    team_limit_requests() -> Vec<TeamLimitRequest>;
    agent_execution(employee_id: &Id) -> Option<AgentExecutionConfig>;
    agent_executions() -> Vec<AgentExecutionConfig>;
    provider_connection(id: &Id) -> Option<ProviderConnection>;
    provider_connections() -> Vec<ProviderConnection>;
    model_profile(id: &Id) -> Option<ModelProfile>;
    model_profiles() -> Vec<ModelProfile>;
    watching_model() -> Option<WatchingModelSetting>;
    team_counter() -> Option<TeamCounter>;
    team_allocations() -> Vec<TeamAllocation>;
    record_refs() -> Vec<RecordRef>;
});

fwd_read!(ProcessesRead for BoxedTx, BoxedSnapshot {
    structure_document(id: &Id) -> Option<StructureDocument>;
    structure_documents() -> Vec<StructureDocument>;
});

fwd_read!(ProjectsRead for BoxedTx, BoxedSnapshot {
    project(id: &Id) -> Option<Project>;
    project_by_number(number: u64) -> Option<Project>;
    project_by_slug(slug: &str) -> Option<Project>;
    resolve_project(reference: &str) -> Option<Project>;
    projects() -> Vec<Project>;
    project_counter() -> Option<ProjectCounter>;
    project_allocations() -> Vec<ProjectAllocation>;
    team_task_moves(chains: &BTreeMap<Id, (Vec<Id>, Vec<Id>)>) -> Vec<TeamHomedMove>;
});

fwd_read!(MessagingRead for BoxedTx, BoxedSnapshot {
    conversation(id: &Id) -> Option<ConversationRecord>;
    conversations() -> Vec<ConversationRecord>;
    message(id: &str) -> Option<MessageRecord>;
    messages_in_conversation(conversation_id: &Id) -> Vec<MessageRecord>;
    all_messages() -> Vec<MessageRecord>;
    recipient_state(message_id: &str, employee_id: &Id) -> Option<RecipientStateRecord>;
    recipient_states_for_employee(employee_id: &Id) -> Vec<RecipientStateRecord>;
    all_recipient_states() -> Vec<RecipientStateRecord>;
    read_marker(conversation_id: &Id, viewer_employee_id: &Id) -> Option<ConversationReadMarkerRecord>;
    preferences(conversation_id: &Id, viewer_employee_id: &Id) -> Vec<ConversationPreferenceRecord>;
    reactions_for_message(message_id: &str) -> Vec<MessageReactionRecord>;
    answer_for_message(message_id: &str) -> Option<ParticipantAnswerFile>;
    max_origin_sequence(origin_office_id: &Id, origin_incarnation: &str) -> u64;
    max_state_sequence(owner_office_id: &Id, origin_incarnation: &str) -> u64;
    max_local_seq() -> u64;
    message_local_seq(message_id: &str) -> Option<u64>;
    message_file(attachment_id: &str) -> Option<BoundMessageFile>;
    unread_inbox_summary(employee_id: &Id, limit: usize) -> org_proto::resume::ResumeInboxSummary;
    open_asks_for_employee(employee_id: &Id, now_utc: &str, caller_is_person: bool, limit: usize) -> Vec<(MessageRecord, bool)>;
});

fwd_read!(AttachmentsRead for BoxedTx, BoxedSnapshot {
    attachment(id: &Id) -> Option<AttachmentRecord>;
    attachments_for_target(kind: AttachmentTargetKind, target_id: Option<&Id>, after: Option<&Id>, limit: usize) -> Vec<AttachmentRecord>;
    attachments() -> Vec<AttachmentRecord>;
});

fwd_read!(TasksRead for BoxedTx, BoxedSnapshot {
    task(id: &Id) -> Option<TaskRecord>;
    task_by_key(office_code: &str, number: u64) -> Option<TaskRecord>;
    resolve_task(reference: &str) -> Option<TaskRecord>;
    tasks() -> Vec<TaskRecord>;
    max_task_number(office_code: &str) -> u64;
    task_ids_by_label(label: &str) -> Vec<Id>;
    task_label_counts() -> Vec<(String, u64)>;
    office_task_counter(office_code: &str) -> Option<OfficeTaskCounterRecord>;
    office_task_counters() -> Vec<OfficeTaskCounterRecord>;
    checklist_steps(task_id: &Id) -> Vec<ChecklistStepRecord>;
    task_comments(task_id: &Id) -> Vec<TaskCommentRecord>;
    task_comment_by_id(comment_id: &Id) -> Option<TaskCommentRecord>;
    task_watchers(task_id: &Id) -> Vec<TaskWatcherRecord>;
    task_links_from(task_id: &Id) -> Vec<TaskLinkRecord>;
    task_dependents(task_id: &Id) -> Vec<Id>;
    all_task_links() -> Vec<TaskLinkRecord>;
    task_events(task_id: &Id) -> Vec<TaskEventRecord>;
    task_page(query: &TaskListQuery) -> TaskListPage;
    task_rollups(ids: &[Id]) -> BTreeMap<Id, TaskRollup>;
    task_history_outlines() -> BTreeMap<Id, TaskHistoryOutline>;
    feature_in_project(project_id: &Id, feature_id: &Id) -> bool;
    milestone_in_project(project_id: &Id, milestone_id: &Id) -> bool;
});

fwd_read!(ProjectDocsRead for BoxedTx, BoxedSnapshot {
    project_doc_entries(project: &Id) -> Vec<DocEntry>;
    project_doc(id: &Id) -> Option<ProjectDoc>;
    project_doc_media(project: &Id) -> Vec<DocMedia>;
    projects_with_docs() -> Vec<Id>;
    document_link_ids(doc_id: &Id) -> Vec<String>;
});

fwd_read!(DeliveryRead for BoxedTx, BoxedSnapshot {
    project_features(project: &Id) -> Vec<FeatureRecord>;
    feature(id: &Id) -> Option<FeatureRecord>;
    all_features() -> Vec<FeatureRecord>;
    project_releases(project: &Id) -> Vec<ReleaseRecord>;
    release(id: &Id) -> Option<ReleaseRecord>;
    release_milestones(release: &Id) -> Vec<MilestoneRecord>;
    project_milestones(project: &Id) -> Vec<MilestoneRecord>;
    milestone(id: &Id) -> Option<MilestoneRecord>;
    project_deployments(project: &Id) -> Vec<DeploymentRecord>;
    deployment(id: &Id) -> Option<DeploymentRecord>;
    project_flags(project: &Id) -> Vec<FeatureFlagRecord>;
    flag(id: &Id) -> Option<FeatureFlagRecord>;
    used_flag_ids(project: &Id) -> BTreeSet<String>;
    release_changelog(release: &Id) -> Option<ScopedChangelogRecord>;
    deployment_changelog(deployment: &Id) -> Option<ScopedChangelogRecord>;
    project_changelogs(project: &Id) -> Vec<ScopedChangelogRecord>;
    project_links(project: &Id) -> Vec<EntityLinkRecord>;
    delivery_counter(scope_kind: &str, scope_id: &Id, entity_kind: &str) -> Option<NumberCounterRecord>;
    projects_with_delivery() -> Vec<Id>;
});

fwd_read!(ReviewsRead for BoxedTx, BoxedSnapshot {
    review(id: &Id) -> Option<ReviewRecord>;
    review_by_key(office_code: &str, number: u64) -> Option<ReviewRecord>;
    resolve_review(reference: &str) -> Option<ReviewRecord>;
    list_reviews(query: &ReviewListQuery) -> Vec<ReviewRecord>;
    max_review_number(office_code: &str) -> u64;
    office_review_counter(office_code: &str) -> Option<OfficeReviewCounterRecord>;
    office_review_counters() -> Vec<OfficeReviewCounterRecord>;
    review_rounds(review_id: &Id) -> Vec<ReviewRoundRecord>;
    review_reviewers(review_id: &Id) -> Vec<ReviewReviewerRecord>;
    review_findings(review_id: &Id) -> Vec<ReviewFindingRecord>;
    review_finding_by_id(id: &Id) -> Option<ReviewFindingRecord>;
    review_finding_replies(review_id: &Id) -> Vec<ReviewFindingReplyRecord>;
    review_finding_reply_by_id(id: &Id) -> Option<ReviewFindingReplyRecord>;
    review_events(review_id: &Id) -> Vec<ReviewEventRecord>;
    review_child_parent_ids() -> Vec<Id>;
});

fwd_read!(SearchRead for BoxedSnapshot {
    search_shared(plan: &SearchPlan) -> SourcePage;
    search_messages(plan: &SearchPlan, scope: &MessageScope) -> SourcePage;
});

fwd_write!(CorePort {
    mark() -> Mark;
    undo_to(mark: Mark) -> ();
    set_source(path: &DurablePath, hash: Option<Sha256>) -> ();
    put_receipt(receipt: &Receipt) -> ();
    check() -> ();
});

fwd_write!(OrganizationWrite {
    put_organization(record: &Organization) -> ();
    remove_organization() -> ();
    put_repositories(records: &[Repository]) -> ();
    put_office(record: &Office) -> ();
    remove_office(id: &Id) -> ();
    put_office_codes(records: &[OfficeCode]) -> ();
    put_role(record: &Role) -> ();
    remove_role(id: &Id) -> ();
    put_team(record: &Team) -> ();
    remove_team(id: &Id) -> ();
    put_employee(record: &Employee) -> ();
    remove_employee(id: &Id) -> ();
    put_hire_request(record: &HireRequest) -> ();
    remove_hire_request(id: &Id) -> ();
    put_team_limit_request(record: &TeamLimitRequest) -> ();
    remove_team_limit_request(id: &Id) -> ();
    put_agent_execution(record: &AgentExecutionConfig) -> ();
    remove_agent_execution(employee_id: &Id) -> ();
    put_provider_connection(record: &ProviderConnection) -> ();
    remove_provider_connection(id: &Id) -> ();
    put_model_profile(record: &ModelProfile) -> ();
    remove_model_profile(id: &Id) -> ();
    put_watching_model(record: &WatchingModelSetting) -> ();
    remove_watching_model() -> ();
    put_team_counter(record: &TeamCounter) -> ();
    remove_team_counter() -> ();
    put_team_allocation(record: &TeamAllocation) -> ();
    remove_team_allocation(number: u64) -> ();
});

fwd_write!(ProcessesWrite {
    put_structure_document(record: &StructureDocument) -> ();
    remove_structure_document(id: &Id) -> ();
});

fwd_write!(ProjectsWrite {
    put_project(record: &Project) -> ();
    remove_project(id: &Id) -> ();
    put_project_counter(record: &ProjectCounter) -> ();
    remove_project_counter() -> ();
    put_project_allocation(record: &ProjectAllocation) -> ();
    remove_project_allocation(number: u64) -> ();
});

fwd_write!(MessagingWrite {
    put_conversation(record: &ConversationRecord) -> ();
    remove_conversation(id: &Id) -> ();
    put_message(record: &MessageRecord, local_seq: u64) -> ();
    remove_message(id: &str) -> ();
    put_recipient_state(record: &RecipientStateRecord) -> ();
    remove_recipient_states_for_participant(conversation_id: &Id, employee_id: &Id) -> ();
    put_read_marker(record: &ConversationReadMarkerRecord) -> ();
    put_preferences(conversation_id: &Id, viewer_employee_id: &Id, prefs: &[ConversationPreferenceRecord]) -> ();
    put_participant_reactions(message_id: &str, employee_id: &Id, reactions: &[MessageReactionRecord]) -> ();
    put_participant_answer(answer: &ParticipantAnswerFile) -> ();
    remove_participant_answer(message_id: &str, employee_id: &Id) -> ();
});

fwd_write!(AttachmentsWrite {
    put_attachment(record: &AttachmentRecord) -> ();
    remove_attachment(id: &Id) -> ();
});

fwd_write!(TasksWrite {
    put_task(record: &TaskRecord) -> ();
    remove_task(id: &Id) -> ();
    put_office_task_counter(record: &OfficeTaskCounterRecord) -> ();
    remove_office_task_counter(office_code: &str) -> ();
    put_checklist_steps(task_id: &Id, steps: &[ChecklistStepRecord]) -> ();
    put_task_comment(record: &TaskCommentRecord) -> ();
    remove_task_comment(task_id: &Id, comment_id: &Id) -> ();
    put_task_watchers(task_id: &Id, watchers: &[TaskWatcherRecord]) -> ();
    put_task_links(task_id: &Id, links: &[TaskLinkRecord]) -> ();
    put_task_event(record: &TaskEventRecord) -> ();
});

fwd_write!(ProjectDocsWrite {
    put_project_doc(doc: &ProjectDoc) -> ();
    remove_project_doc_at(project: &Id, path: &str) -> ();
    put_doc_media(media: &DocMedia) -> ();
    remove_doc_media_at(project: &Id, path: &str) -> ();
});

fwd_write!(DeliveryWrite {
    put_feature(record: &FeatureRecord) -> ();
    remove_feature(id: &Id) -> ();
    put_release(record: &ReleaseRecord) -> ();
    remove_release(id: &Id) -> ();
    put_milestone(record: &MilestoneRecord) -> ();
    remove_milestone(id: &Id) -> ();
    put_deployment(record: &DeploymentRecord) -> ();
    remove_deployment(id: &Id) -> ();
    put_flag(record: &FeatureFlagRecord) -> ();
    remove_flag(id: &Id) -> ();
    put_scoped_changelog(record: &ScopedChangelogRecord) -> ();
    remove_release_changelog(release: &Id) -> ();
    remove_deployment_changelog(deployment: &Id) -> ();
    put_entity_link(record: &EntityLinkRecord) -> ();
    remove_entity_link(source_kind: LinkEntityKind, source_id: &Id, link_id: &Id) -> ();
    put_delivery_counter(scope_kind: &str, scope_id: &Id, entity_kind: &str, counter: &NumberCounterRecord) -> ();
    remove_delivery_counter(scope_kind: &str, scope_id: &Id, entity_kind: &str) -> ();
});

fwd_write!(ReviewsWrite {
    put_review_document(doc: &ReviewDocument) -> ();
    put_review_round(record: &ReviewRoundRecord) -> ();
    put_review_event(record: &ReviewEventRecord) -> ();
    put_office_review_counter(record: &OfficeReviewCounterRecord) -> ();
    remove_office_review_counter(office_code: &str) -> ();
});
