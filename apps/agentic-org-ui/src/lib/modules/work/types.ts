// What the Work screens read and change. Field names follow the task contract
// (docs/design/organization-data/03-features-and-work.md `tasks` and `task_details`, 05 operational data, 12 task
// assignment and history) in the browser's camelCase: `team` is the required accountable `team_id`, `assignedTeam`
// and `assignee` the independent assignment pair, `reporter` the immutable admitted creator. Every type uses the same
// `T-<office code><number>` reference. `null` means not set or unknown, never zero.

export type TaskType = 'task' | 'bug' | 'request';
/** One closed list for every type (document 03). */
export type TaskStatus = 'todo' | 'in_progress' | 'blocked' | 'cancelled' | 'done';
export type Priority = 'urgent' | 'high' | 'normal' | 'low';
export type Severity = 'critical' | 'major' | 'minor' | 'trivial';
/** Why a task was closed (`resolution`, with `closed_reason` in details). */
export type Resolution =
	'fixed' | 'duplicate' | 'wont_fix' | 'cant_reproduce' | 'completed' | 'cancelled';

export interface Ref {
	id: string;
	name: string;
}

/** A person or an agent (an employee). Kind is a word on screen, never a shape. */
export interface EmployeeRef extends Ref {
	kind: 'agent' | 'person';
	/** Offboarded or deactivated: kept on history, never offered for new assignment. */
	former?: boolean;
}

export interface ProjectRef extends Ref {
	/** `P-<number>`. */
	number: number;
}

export interface Attribution {
	/** RFC 3339. */
	at: string;
	by: EmployeeRef;
}

/** Another task named by a link: enough to show its type, reference, title and status. */
export interface TaskLink {
	id: string;
	ref: string;
	type: TaskType;
	title: string;
	status: TaskStatus;
}

/**
 * Work actually waiting on the owner through an available source (work-items §11). `inboxItemId` names the open
 * Inbox item the row stands for; a source without one says so in `text`.
 */
export interface NeedsMe {
	source:
		'owner_block' | 'review' | 'handoff' | 'ask' | 'budget' | 'verify' | 'critical' | 'approval';
	/** "Kai asked which retry limit to use". */
	text: string;
	inboxItemId?: string;
}

/** A task budget's state as everyone who can see the task may read it (costs §1 "Who sees amounts"). */
export type BudgetWord = 'ok' | 'warned' | 'reached';

/** The checklist as a count, for rows and cards (CHK-1). */
export interface ChecklistCount {
	done: number;
	total: number;
	/** Required steps not ticked yet. */
	requiredLeft: number;
}

/** One row of the shared task projection: board, list and Needs me read the same data. */
export interface TaskRow {
	id: string;
	ref: string;
	type: TaskType;
	title: string;
	status: TaskStatus;
	priority: Priority;
	/** Bugs only; null is Not set. */
	severity: Severity | null;
	/** Whole points; null when not estimated. */
	effort: number | null;
	project: ProjectRef | null;
	/** The accountable team: always present. */
	team: Ref;
	assignedTeam: Ref | null;
	assignee: EmployeeRef | null;
	reporter: EmployeeRef;
	/** RFC 3339. */
	createdAt: string;
	updatedAt: string;
	/** When the current status was entered: Done's seven days count from it. */
	statusAt: string;
	checklist: ChecklistCount;
	/** Required when Blocked (TSK-3). */
	blockedReason: string | null;
	reopenCount: number;
	needsMe: NeedsMe | null;
	/** Null when the task has no budget. */
	budget: BudgetWord | null;
	/** A bug marked as fixed, waiting for its verification (BUG-14). */
	awaitingVerification?: boolean;
}

/** Who is looking: "my" filters derive from the admitted employee, never from an owner flag (WORK-2). */
export interface Viewer {
	/** The viewer's employee identity; null when the account has none (then the "my" filters say so). */
	employee: EmployeeRef | null;
	/** Teams the viewer's employee belongs to. */
	teamIds: string[];
	/** The authenticated organization owner. */
	owner: boolean;
}

export interface StaleCopy {
	/** RFC 3339: when the last copy was taken. */
	copiedAt: string;
}

/** The queue in one scope (organization or project). */
export interface WorkQueue {
	rows: TaskRow[];
	viewer: Viewer;
	/** Needs me sources this server reports. Empty: Needs me is not available and is not inferred. */
	needsMeSources: NeedsMe['source'][];
	stale?: StaleCopy;
}

export type WorkScope = { kind: 'organization' } | { kind: 'project'; projectId: string };

// ---- The task page ----

export interface ChecklistStep {
	id: string;
	text: string;
	required: boolean;
	done: boolean;
	/** Who ticked it and when (CHK-3); absent while unticked. */
	checked?: Attribution;
	linkedTask?: TaskLink;
	revision: number;
}

/** Shared task discussion. `note` is an old shared note comment: still shared (decision 10). */
export interface Comment {
	id: string;
	kind: 'comment' | 'note' | 'question' | 'answer';
	author: EmployeeRef;
	at: string;
	body: string;
}

/** An author's private task note (PN-01..06). Never mixed into shared history or counts. */
export interface PrivateNote {
	id: string;
	author: EmployeeRef;
	body: string;
	at: string;
	revision: number;
	/** The viewer wrote it: only the author edits, deletes or publishes. */
	mine: boolean;
}

/**
 * One immutable history event. `kind` is the event tag; an unknown tag renders as "Unknown change" and is kept.
 * `text` is the sentence after the actor ("assigned T-A42 to Platform · Ana").
 */
export interface ActivityEvent {
	id: string;
	at: string;
	actor: EmployeeRef | null;
	kind: string;
	text: string;
	/** Free text the actor gave: a reason, a note. */
	note?: string;
}

/** Permitted deliverables (§9.3). A target without a valid resolver keeps its identity and says why. */
export interface OutputItem {
	id: string;
	kind: 'document' | 'commit' | 'branch' | 'file';
	label: string;
	/** An in-app path, only when the target resolves. */
	href?: string;
	/** Why it can't be opened. */
	unresolved?: string;
}

/** Per-repository evidence (TSK-8): only what is actually known. */
export interface RepositoryEvidence {
	repository: string;
	branch?: string;
	worktree?: string;
	integratedCommit?: string;
}

/** A decision, design, spec or evaluation explaining the work (decisions-and-tech-debt §5). */
export interface WhyLink {
	id: string;
	key?: string;
	title: string;
	kind: 'decision' | 'design' | 'spec' | 'evaluation';
	status: 'draft' | 'proposed' | 'current' | 'deprecated';
}

/**
 * Time on the task (§9.2, costs "Shared duration definitions"). Whole seconds. `startedAt` null with
 * `startUnavailable` means history begins after the start (a baseline): never zero. Working time is not tracked.
 */
export interface TaskTime {
	createdAt: string;
	startedAt: string | null;
	startUnavailable: boolean;
	/** Still open: durations run to now and read "so far". */
	open: boolean;
	leadSeconds: number | null;
	cycleSeconds: number | null;
	waitingOnYouSeconds: number;
	blockedSeconds: number;
	/** "Typical for a task in Platform": a range, only when there are enough comparable tasks. */
	typical?: { label: string; cost: string; time: string };
}

export interface BugReport {
	whatHappened: string;
	expected: string | null;
	steps: string | null;
	environment: string | null;
	/** The values the reporter gave, kept apart from current triage values (TRI-4). */
	reported: { severity: Severity | null; priority: Priority; at: string; by: EmployeeRef };
}

export interface FixRecord {
	how: { kind: 'task'; task: TaskLink } | { kind: 'commit'; commit: string } | { kind: 'note' };
	note: string | null;
	at: string;
	by: EmployeeRef;
}

export interface Verification {
	result: 'fixed' | 'still_happens';
	note: string | null;
	at: string;
	by: EmployeeRef;
}

export interface BugDetails {
	report: BugReport;
	severityConfirmed: boolean;
	suggestedVerifier: EmployeeRef | null;
	suggestedTriager: EmployeeRef | null;
	/** The latest Mark as fixed, until it is verified or reopened. */
	fixed: FixRecord | null;
	verifications: Verification[];
	closedReason: string | null;
	duplicateOf: TaskLink | null;
	/** Tasks whose `fixes` link points at this bug (derived `fixed_by`). */
	fixTasks: TaskLink[];
}

export interface DraftTask {
	draftId: string;
	title: string;
	description?: string;
	assignee?: EmployeeRef;
	team?: Ref;
	priority?: Priority;
}

/** `details.request_outcome` (document 03). */
export interface RequestOutcome {
	feature?: { type: 'new' | 'extends'; featureId: string; ref: string; title: string };
	docs: { projectId: string; docKey: string; title: string }[];
	draftTasks: DraftTask[];
	estimate?: { effort: string; costUsdCentsMin: number | null; costUsdCentsMax: number | null };
	/** Who recorded the outcome and when. */
	recorded: Attribution;
}

export interface RequestDetails {
	text: string;
	routeReason: string | null;
	routingSource: 'person' | 'rule' | 'copilot' | null;
	outcome: RequestOutcome | null;
	/** Ask for changes returned the outcome to the scoper. */
	changesAsked?: Attribution & { comment: string };
	declineReason: string | null;
	mergedInto: TaskLink | null;
	/** Tasks Approve and create made, linked to the request. */
	created: TaskLink[];
	approved?: Attribution;
}

/** What the viewer may do here. The task service decides regardless; these only choose what to show. */
export interface TaskPermissions {
	edit: boolean;
	triage: boolean;
	/** The reporter or the suggested verifier, when a fix is waiting (FIX-4, FIX-6). */
	verify: boolean;
	/** Owner or authority over the accountable team; never the scoper of its own outcome. */
	approve: boolean;
	/** Why approve is unavailable when it is, in words. */
	approveReason?: string;
}

export interface TaskDetail extends TaskRow {
	/** The concurrency revision every change names. */
	revision: number;
	description: string;
	feature: { id: string; ref: string; title: string } | null;
	milestone: { id: string; name: string } | null;
	resolution: Resolution | null;
	latestUpdate: Attribution;
	steps: ChecklistStep[];
	comments: Comment[];
	activity: ActivityEvent[];
	output: OutputItem[];
	/** A recorded shared completion explanation. */
	completion: (Attribution & { text: string }) | null;
	/** Bugs this task fixes (its authored `fixes` links). */
	fixes: TaskLink[];
	relatesTo: TaskLink[];
	repositories: RepositoryEvidence[];
	why: WhyLink[];
	time: TaskTime;
	bug?: BugDetails;
	request?: RequestDetails;
	can: TaskPermissions;
	stale?: StaleCopy;
}

// ---- Cost and time, budgets (costs-and-insights §2, §4; document 17) ----

export type BudgetUnit = 'usd' | 'tokens';

/** A task's lifetime budget. Amounts are integer micro-USD or tokens, as `unit` says. */
export interface TaskBudget {
	unit: BudgetUnit;
	/** The effective limit (base + floor(base / 5) after the one extension). */
	limit: number;
	/** Known spend; a lower bound when `complete` is false. */
	spent: number;
	complete: boolean;
	incompleteReason?: string;
	/** The runtime reports no usage: this budget can't stop its unmeasured work. */
	unmeasured?: boolean;
	/** Reached: stop_pending until stopping is verified, then held. */
	hold?: 'stop_pending' | 'held';
	extended: boolean;
	baseLimit?: number;
	/** The policy revision a change is made against. */
	policyRevision: number;
}

export interface AgentUsage {
	agent: EmployeeRef;
	tokens: number | null;
	costMicroUsd: number | null;
	lastActivityAt: string;
}

/** The cost block as the owner or a project manager sees it. */
export interface TaskCostAmounts {
	knownCostMicroUsd: number | null;
	knownTokens: number | null;
	records: number;
	unpricedRecords: number;
	/** "Antigravity doesn't report usage". */
	incompleteReason?: string;
	includesApiEquivalent: boolean;
	byAgent: AgentUsage[];
	budget: TaskBudget | null;
}

/** Who may act on the budget (costs §4 "Actor/unit checks"). */
export interface BudgetRights {
	/** Set or lower in the same unit (an agent project manager may). */
	set: boolean;
	/** Raise, clear, extend, change unit: a person only. */
	raise: boolean;
	/** Stop uses existing transition authority. */
	stop: boolean;
}

/**
 * The cost block. Amounts are scoped: anyone else sees only the budget state (none, OK, 80%, reached) with no
 * amounts.
 */
export type TaskCost =
	| { scope: 'amounts'; amounts: TaskCostAmounts; rights: BudgetRights }
	| { scope: 'state'; budget: BudgetWord | null; hold?: 'stop_pending' | 'held' };

export type BudgetAction = 'raise' | 'allow_once' | 'clear' | 'stop';

/** Where a budget operation stands when its outcome was not confirmed. */
export type BudgetOperation =
	{ state: 'applied' } | { state: 'not_applied'; reason: string } | { state: 'pending' };

// ---- Capture and choices ----

/** A team that can be accountable or receive new assignments: active, in this organization. */
export interface TeamChoice extends Ref {
	path: string;
}

/** An active employee that can receive new assignments, with actual availability (RQS-8). */
export interface EmployeeChoice extends EmployeeRef {
	role?: string;
	/** "Available", "Working on T-A31", "Paused". Assignment wakes nobody. */
	availability: string;
}

export interface ProjectChoice extends ProjectRef {
	/** The project's accountable team: a project task always uses it. */
	ownerTeam: Ref;
	archived: boolean;
}

export interface Choices {
	teams: TeamChoice[];
	employees: EmployeeChoice[];
	projects: ProjectChoice[];
	/** The organization the dialog captures into, named in it (RQS-1). */
	organizationName: string;
}

export interface NewTask {
	/** Reused when the same attempt is sent again, so a lost answer never creates a second task. */
	operationId: string;
	type: TaskType;
	title: string;
	description: string;
	projectId: string | null;
	/** Accountable team. With a project the server uses the project's owner team. */
	teamId: string;
	assignedTeamId: string | null;
	assigneeId: string | null;
	priority: Priority;
	severity: Severity | null;
	bug?: { whatHappened: string; expected: string; steps: string; environment: string };
	/** A fix task: its authored `fixes` link points at this bug. */
	fixesTaskId?: string;
	/** Created from a request or a task: `relates_to`. */
	relatesToTaskId?: string;
}

/** The pair saved together at the expected revision (§5). */
export interface AssignmentChange {
	assignedTeamId: string | null;
	assigneeId: string | null;
	expectedRevision: number;
}

/** What a stale assignment write meets: the current pair with who set it (TRI-6). */
export interface AssignmentConflict {
	assignedTeam: Ref | null;
	assignee: EmployeeRef | null;
	by: EmployeeRef;
	at: string;
	revision: number;
}

export type CloseAs =
	| { as: 'done' }
	| { as: 'cancelled'; reason: string | null }
	| { as: 'duplicate'; originalRef: string }
	| { as: 'wont_fix'; reason: string }
	| { as: 'cant_reproduce'; reason: string };

export type FixHow =
	{ kind: 'task'; taskRef: string } | { kind: 'commit'; commit: string } | { kind: 'note' };

/** The task fields a form writes; `field` on a refusal names one of these. */
export type TaskField =
	| 'title'
	| 'description'
	| 'whatHappened'
	| 'team'
	| 'project'
	| 'assignment'
	| 'reason'
	| 'note'
	| 'original'
	| 'step'
	| 'limit'
	| 'body'
	| 'commit';
