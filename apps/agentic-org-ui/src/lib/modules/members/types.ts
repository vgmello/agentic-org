// What the Members directory, the member profile and its tabs read. Field names follow the employee contract
// (docs/design/organization-data/04-agent-structure.md) where the record exists; presence, readiness and turns
// are observations the office reports (our-org-in-the-app §5, employee-supervision-ux §1, agent-run-modes-ux §1).

/** A team as rows and headers name it: its name and its readable reference (TEAM-3). */
export interface TeamRef {
	id: string;
	name: string;
	/** `TEAM-<number>`; empty while HQ has not numbered it. */
	ref: string;
}

export interface MemberRef {
	id: string;
	name: string;
}

/**
 * The one presence vocabulary, in priority order (our-org-in-the-app §5). Unknown is never guessed: rows omit it,
 * an agent's profile header shows "Status unknown" once.
 */
export type PresenceId =
	| 'needs_you'
	| 'with_you'
	| 'working'
	| 'waiting_answer'
	| 'paused'
	| 'available'
	| 'offline'
	| 'unknown';

export interface Presence {
	id: PresenceId;
	/** Who it waits on and for how long: "from Ana · 12 min". */
	detail?: string;
}

/** Employment status (`status` in profile.md), separate from observed presence (§7.0). */
export type EmployeeStatus = 'active' | 'paused' | 'inactive';

/** Where an agent runs: the office and when the office last reported it. `null` for an agent: Unplaced. */
export interface Placement {
	id: string;
	name: string;
	/** RFC 3339: the office's last observation. */
	observedAt?: string;
}

export interface WorkRef {
	ref: string;
	title: string;
	project?: string;
}

/** One row of the Members directory: a person or an agent. */
export interface MemberRow {
	id: string;
	kind: 'agent' | 'person';
	name: string;
	/** Stored without `@`. */
	handle: string;
	/** Role or title, as recorded (role_text when no role is referenced). */
	role?: string;
	status: EmployeeStatus;
	/** The organization's owner: a person, shown once with an Owner badge (§7.2). */
	owner: boolean;
	/** A person with an account (§7.4). Never true for an agent. */
	canSignIn: boolean;
	/** Omitted while unknown. People have one only while actually online. */
	presence?: Presence;
	/** Required on a valid record; missing means an invalid record, shown as a problem. */
	homeTeam?: TeamRef;
	/** Agents only: `null` is Unplaced. Always `undefined` for a person (PEOPLE-6). */
	office?: Placement | null;
	/** Explicit `reports_to_employee_id`; undefined is No explicit manager. */
	reportsTo?: MemberRef;
	workingOn?: WorkRef;
	/** Agents: the runtime and model it runs on, in words ("Claude Code · claude-sonnet-5"). */
	runsOn?: string;
	/** Agents: how it is started, in words. */
	howItRuns?: string;
	contact?: string;
	directReports: number;
	/** RFC 3339, for Former members: when the record became inactive, if HQ recorded it. */
	inactiveSince?: string;
}

export type TurnState =
	| 'accepted'
	| 'running'
	| 'switching'
	| 'blocked'
	| 'recovery_required'
	| 'succeeded'
	| 'failed'
	| 'cancelled';

/** The execution view (employee-supervision-ux §1): what was asked for, what actually ran and where. */
export interface ExecutionView {
	/** From execution.md; null when the agent has no execution configuration. */
	requested: { pool: string; model: string } | null;
	/** The connection of the current or last actual attempt. */
	connection?: string;
	/** The logical turn's state, or `idle` with no open turn. */
	turnState: TurnState | 'idle';
	office?: { name: string; generation: number; observedAt?: string };
	/** Concrete reasons it can't start (missing binding, sign-in, stale entitlement, coordinator outage…). */
	readiness: { known: boolean; reasons: string[] };
	/** The managed home, read-only, from the owning office. */
	home?: string;
}

/** The Supervision line: words and times, never colour alone. */
export interface Supervision {
	health: string;
	tone: 'ok' | 'waiting' | 'problem' | 'unknown';
	/** Every unresolved manual, security or budget reason. */
	holds: string[];
	/** A Pause accepted at HQ that the office has not confirmed yet. */
	stopPending?: boolean;
}

export interface MemberProfile extends MemberRow {
	title?: string;
	focus?: string;
	/** "Hired 8 Nov · hired by Sam Ortiz" (APPR-5). Absent for people and the built-in HR lead. */
	hired?: { at: string; by?: MemberRef };
	/** Teams this employee leads. */
	leads: TeamRef[];
	/** Allow hiring for the teams it leads (§7.1). */
	allowHiring: { on: boolean; canChange: boolean; reason?: string };
	currentTask?: WorkRef;
	supervision?: Supervision;
	execution?: ExecutionView;
	/** The open turn, when one exists: a second Run is Busy (RUN-4). */
	openTurn?: { id: string; startedAt: string };
	/** Why Run now can't start: paused, not configured, not signed in. */
	cannotStart?: string;
	viewer: {
		isSelf: boolean;
		/** The owner or the administrator: sign-in access and ownership actions. */
		isOwnerOrAdmin: boolean;
		/** COST-7: the owner, or a project manager for their projects. */
		costVisible: boolean;
	};
}

export type StartedByKind =
	| 'run'
	| 'message'
	| 'mention'
	| 'ask'
	| 'handoff'
	| 'assignment'
	| 'answer'
	| 'resume'
	| 'recovery';

export type AttemptOutcome =
	'running' | 'succeeded' | 'failed' | 'switched' | 'cancelled' | 'skipped';

export interface Attempt {
	n: number;
	connection: string;
	model: string;
	startedAt?: string;
	endedAt?: string;
	outcome: AttemptOutcome;
	reason?: string;
	inputTokens?: number;
	outputTokens?: number;
	usageComplete: boolean;
}

/** One logical turn (agent-run-modes-ux §1, RUN-5). */
export interface Turn {
	id: string;
	startedAt: string;
	endedAt?: string;
	startedBy: { kind: StartedByKind; by?: MemberRef; byViewer?: boolean; ref?: string };
	state: TurnState;
	/** The agent's one-line summary. */
	summary?: string;
	failure?: string;
	/** Only when the viewer may see amounts; `undefined` otherwise. */
	cost?: { usd: number; complete: boolean; inYourProjects?: boolean };
	attempts: Attempt[];
	/** Whether a session log can be opened for it. */
	hasLog: boolean;
}

export interface ActivityEntry {
	id: string;
	at: string;
	actor: string;
	kind: string;
	text: string;
}

export interface ConversationRow {
	id: string;
	title: string;
	kind: 'channel' | 'direct' | 'thread';
	lastAt: string;
	preview: string;
}

export type WorkState = 'todo' | 'in_progress' | 'in_review' | 'blocked' | 'done';

export interface WorkRow {
	ref: string;
	title: string;
	project?: string;
	assignee?: string;
	priority?: 'urgent' | 'high' | 'medium' | 'low';
	state: WorkState;
}

export interface ProcessRow {
	key: string;
	name: string;
	level: 'organization' | 'team' | 'project';
	/** "Checkout", the team or project the version comes from. */
	from?: string;
	revision: number;
	relation: 'inherited' | 'replaces' | 'added' | 'not_overridable';
}

export interface ProcessesView {
	/** False when there is no current work: the organization's set is shown with the spec's sentence. */
	hasCurrentWork: boolean;
	rows: ProcessRow[];
}

export interface RoleView {
	role?: { name: string; revision: number; body: string };
	roleText?: string;
	title?: string;
}

export interface AssignableTask {
	ref: string;
	title: string;
	project?: string;
}

export interface SignInLink {
	url: string;
	expiresAt: string;
}
