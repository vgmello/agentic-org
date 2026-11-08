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
	working_ms?: number;
	waiting_on_owner_ms: number;
	blocked_ms: number;
	idle_ms: number;
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
