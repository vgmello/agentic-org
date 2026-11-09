// Work items (tasks, bugs, requests), as ada's W7 design specifies `org_proto::work` (docs/design/tasks-projects-docs.md
// §4). A mirror to build against until the Rust types land in crates/org-proto; replace it with the real shapes then.
// Unknown enum values from a newer office decode as 'unknown' and are never written.

export type ItemKind = 'task' | 'bug' | 'request' | 'unknown';
export type ItemStatus =
	| 'todo'
	| 'in_progress'
	| 'blocked'
	| 'done'
	| 'new'
	| 'triaged'
	| 'fixed'
	| 'closed'
	| 'routed'
	| 'scoping'
	| 'scoped'
	| 'approved'
	| 'declined'
	| 'merged'
	| 'unknown';
export type Resolution = 'completed' | 'cancelled' | 'verified' | 'duplicate' | 'wont_fix' | 'cant_reproduce' | 'unknown';
export type Priority = 'urgent' | 'high' | 'normal' | 'low' | 'unknown';
export type Severity = 'critical' | 'major' | 'minor' | 'trivial' | 'unknown';
export type RequestKind = 'feature' | 'change' | 'technical' | 'question' | 'unknown';
export type RoutedBy = 'person' | 'rule' | 'copilot' | 'unknown';
export type LinkKind = 'fixes' | 'part_of' | 'from_request' | 'reported_from' | 'relates_to' | 'pays_back' | 'unknown';
export type LinkTarget = { type: 'item'; item_id: string } | { type: 'doc'; project_id: string; doc_key: string };
export type CommentKind = 'note' | 'question' | 'answer' | 'unknown';

/** A position in an office's work feed. A different `incarnation` means: take a fresh snapshot. */
export interface WorkPosition {
	incarnation: number;
	seq: number;
}

export interface ChecklistCounts {
	total: number;
	done: number;
	required_left: number;
}

/** What lists, boards and live updates need. No long text. */
export interface WorkItemSummary {
	id: string;
	kind: ItemKind;
	project_id?: string;
	/** Empty until HQ assigns it ("Pending · short id"). */
	number?: number;
	short_id: string;
	title: string;
	status: ItemStatus;
	resolution?: Resolution;
	priority?: Priority;
	assignee_id?: string;
	created_by: string;
	created_at: string;
	updated_at: string;
	team_id?: string;
	feature_id?: string;
	blocked_reason?: string;
	area_id?: string;
	severity?: Severity;
	severity_confirmed: boolean;
	reopen_count: number;
	duplicate_of?: string;
	request_kind?: RequestKind;
	/** A task: the bugs it fixes. A bug: its fix tasks. */
	fixes: string[];
	fixed_by: string[];
	checklist: ChecklistCounts;
	done_anyway: boolean;
	owner_office_id: string;
	generation: number;
	version: number;
	deleted: boolean;
}

export interface ChecklistStep {
	id: string;
	text: string;
	required: boolean;
	done: boolean;
	task_id?: string;
	checked_by?: string;
	checked_at?: string;
	version: number;
}

export interface WorkLink {
	id: string;
	from_item_id: string;
	kind: LinkKind;
	to: LinkTarget;
	created_by: string;
	created_at: string;
	target_deleted: boolean;
}

export interface DoneAnyway {
	reason: string;
	steps: string[];
}

/** All durations in milliseconds. */
export interface TimeFigures {
	lead_ms: number;
	cycle_ms?: number;
	/** Present only when working time is tracked. */
	working_ms?: number;
	waiting_on_owner_ms: number;
	blocked_ms: number;
	/** Present only when working time is tracked. */
	idle_ms?: number;
	/** lead - waiting on the owner - blocked: working and idle together while working time is untracked (ada/proto-work-p1 @ 6a17a70). */
	rest_ms: number;
	/** "so far": the item is still open. */
	open: boolean;
	rounds: number;
	as_of: string;
}

/** One item in full (`work.get`). */
export interface WorkItem {
	summary: WorkItemSummary;
	backlinks_more: boolean;
	watchers_more: boolean;
	description?: string;
	definition_of_done?: string;
	branch?: string;
	done_anyway_detail?: DoneAnyway;
	what_happened?: string;
	expected?: string;
	steps?: string;
	environment?: string;
	closed_reason?: string;
	text?: string;
	/** Request: why this scoper (at most 500 characters). */
	route_reason?: string;
	routed_by?: RoutedBy;
	/** Request: set when scoped. */
	outcome?: RequestOutcome;
	/** Request: set exactly when merged. */
	merged_into?: string;
	/** Request: set exactly when declined. */
	decline_reason?: string;
	checklist: ChecklistStep[];
	links: WorkLink[];
	backlinks: WorkLink[];
	watchers: string[];
	time: TimeFigures;
	suggested_triager?: string;
	suggested_verifier?: string;
}

export type WorkStatus = 'committed' | 'queued' | 'unknown';

/** The answer to every write: the position, and every item the call changed, whole, after the change. */
export interface WorkAccepted {
	status: WorkStatus;
	position?: WorkPosition;
	items: WorkItemSummary[];
}

export interface WorkSnapshot {
	position: WorkPosition;
	items: WorkItemSummary[];
	next_cursor?: string;
}

export interface NewStep {
	step_id: string;
	text: string;
	required: boolean;
}

// ---- Task calls (§4.4). Every write starts with a ULID `change_id`. ----------------------------------------------

export interface TaskCreate {
	change_id: string;
	item_id: string;
	project_id: string;
	title: string;
	description?: string;
	definition_of_done?: string;
	assignee_id?: string;
	team_id?: string;
	priority?: Priority;
	/** P3. */
	feature_id?: string;
	/** P2: the bugs this task fixes. */
	fixes?: { link_id: string; bug_id: string }[];
	checklist?: NewStep[];
}

export interface TaskUpdate {
	change_id: string;
	item_id: string;
	expected_version: number;
	title?: string;
	description?: string;
	clear_description?: boolean;
	definition_of_done?: string;
	clear_definition_of_done?: boolean;
	priority?: Priority;
	team_id?: string;
	clear_team?: boolean;
	/** P3. */
	feature_id?: string;
	clear_feature?: boolean;
}

export interface TaskAssign {
	change_id: string;
	item_id: string;
	expected_version: number;
	/** Absent: unassign. */
	assignee_id?: string;
}

export interface TaskSetStatus {
	change_id: string;
	item_id: string;
	expected_version: number;
	status: 'todo' | 'in_progress' | 'blocked' | 'done';
	/** Exactly when `blocked`. */
	blocked_reason?: string;
	/** Only with `done`; defaults to `completed`. */
	resolution?: 'completed' | 'cancelled';
	/** Only with `done`: records "done anyway" when required steps are unticked. */
	anyway_reason?: string;
	/** P4: `{ type: 'none' }` or `{ type: 'debt', ids }`, only with `done`. */
	shortcut?: { type: 'none' } | { type: 'debt'; ids: string[] };
}

export const WORK_LIMITS = {
	title: 120,
	text: 10_000,
	description: 20_000,
	reason: 1_000,
	blockedReason: 200,
	definitionOfDone: 1_000,
	stepText: 200,
	maxSteps: 100
} as const;

/** The statuses a task moves between, in board order. */
export const TASK_STATUSES = ['todo', 'in_progress', 'blocked', 'done'] as const;
export type TaskStatus = (typeof TASK_STATUSES)[number];

// ---- Bug calls (§4.5, P2) -------------------------------------------------------------------------------------------

export interface BugReport {
	change_id: string;
	item_id: string;
	project_id: string;
	title: string;
	what_happened: string;
	severity: Exclude<Severity, 'unknown'>;
	area_id?: string;
	expected?: string;
	steps?: string;
	environment?: string;
	checklist?: NewStep[];
}

export interface BugUpdate {
	change_id: string;
	item_id: string;
	expected_version: number;
	title?: string;
	what_happened?: string;
	expected?: string;
	clear_expected?: boolean;
	steps?: string;
	clear_steps?: boolean;
	environment?: string;
	clear_environment?: boolean;
	severity?: Exclude<Severity, 'unknown'>;
	priority?: Priority;
	area_id?: string;
}

export interface BugTriage {
	change_id: string;
	item_id: string;
	expected_version: number;
	area_id: string;
	severity: Exclude<Severity, 'unknown'>;
	priority: Priority;
	assignee_id?: string;
}

export interface BugAssign {
	change_id: string;
	item_id: string;
	expected_version: number;
	assignee_id?: string;
}

export interface BugMarkFixed {
	change_id: string;
	item_id: string;
	expected_version: number;
	/** What fixed it (1 to 1,000 characters). */
	note: string;
	anyway_reason?: string;
}

export interface BugVerify {
	change_id: string;
	item_id: string;
	expected_version: number;
	outcome: 'fixed' | 'still_happens';
	/** Required for `still_happens`. */
	note?: string;
}

export interface BugClose {
	change_id: string;
	item_id: string;
	expected_version: number;
	/** Close as a duplicate of this bug… */
	duplicate_of?: string;
	/** …or with a reason. */
	resolution?: 'wont_fix' | 'cant_reproduce';
	reason?: string;
}

export interface BugReopen {
	change_id: string;
	item_id: string;
	expected_version: number;
	note: string;
}

// ---- Links (§4.6): a fix task fixes a bug --------------------------------------------------------------------------

export interface WorkLinkCall {
	change_id: string;
	link_id: string;
	from_item_id: string;
	kind: 'fixes' | 'relates_to' | 'pays_back';
	to: LinkTarget;
}

export interface WorkUnlink {
	change_id: string;
	link_id: string;
}

export const SEVERITIES = ['critical', 'major', 'minor', 'trivial'] as const;


/** Why an item waits on the owner (`WaitingStarted`/`WaitingEnded` events; ada/proto-work-p1 @ 410e1a6). */
export type WaitReason =
	| { type: 'question'; comment_id: string }
	| { type: 'message'; message_id: string }
	| { type: 'triage' }
	| { type: 'decision'; doc_key: string }
	| { type: 'unknown' };


// ---- Request calls (§4.7, P3) ---------------------------------------------------------------------------------------

export type FeatureOutcome = { type: 'new'; feature_id: string } | { type: 'extends'; feature_id: string } | { type: 'unknown' };

export interface DocRef {
	project_id: string;
	doc_key: string;
}

/** A task a scoped request proposes. At most 50 per outcome. */
export interface DraftTask {
	draft_id: string;
	title: string;
	description?: string;
	assignee_id?: string;
	team_id?: string;
	priority?: Priority;
}

export interface Estimate {
	effort?: string;
	cost_usd_cents_min?: number;
	cost_usd_cents_max?: number;
}

export interface RequestOutcome {
	feature?: FeatureOutcome;
	docs: DocRef[];
	draft_tasks: DraftTask[];
	estimate?: Estimate;
}

/** A draft as the owner edited it (RQS-9). */
export interface ApprovedTask {
	item_id: string;
	link_id: string;
	title: string;
	description?: string;
	definition_of_done?: string;
	assignee_id?: string;
	team_id?: string;
	priority?: Priority;
	checklist?: NewStep[];
}

export interface RequestCreate {
	change_id: string;
	item_id: string;
	/** The original words: 1 to 20,000 characters, never edited. */
	text: string;
	/** Defaults to the first line of the text, cut to 120 characters. */
	title?: string;
	request_kind?: Exclude<RequestKind, 'unknown'>;
	project_id?: string;
	scoper_id?: string;
	route_reason?: string;
	routed_by?: Exclude<RoutedBy, 'unknown'>;
}

export interface RequestUpdate {
	change_id: string;
	item_id: string;
	expected_version: number;
	title?: string;
	request_kind?: Exclude<RequestKind, 'unknown'>;
	project_id?: string;
	clear_project?: boolean;
}

export interface RequestRoute {
	change_id: string;
	item_id: string;
	expected_version: number;
	/** Absent: back to Not routed. */
	scoper_id?: string;
	route_reason?: string;
	routed_by?: Exclude<RoutedBy, 'unknown'>;
}

export interface RequestAccept {
	change_id: string;
	item_id: string;
	expected_version: number;
}

export interface RequestScope {
	change_id: string;
	item_id: string;
	expected_version: number;
	outcome: RequestOutcome;
}

export interface RequestAskChanges {
	change_id: string;
	item_id: string;
	expected_version: number;
	comment_id: string;
	note: string;
}

/** App only: it commits the organization to work. */
export interface RequestApprove {
	change_id: string;
	item_id: string;
	expected_version: number;
	/** 1 to 50. */
	tasks: ApprovedTask[];
}

export interface RequestDecline {
	change_id: string;
	item_id: string;
	expected_version: number;
	reason: string;
}

export interface RequestMerge {
	change_id: string;
	item_id: string;
	expected_version: number;
	into: string;
	comment_id: string;
}

export const REQUEST_LIMITS = { text: 20_000, routeReason: 500, declineReason: 1_000, draftDescription: 2_000, drafts: 50, effort: 200 } as const;
