/** The kinds the Activity filter offers (our-org-in-the-app §6). */
export type ActivityKind = 'work' | 'conversation' | 'decision' | 'run' | 'org';
export type ActivityKindFilter = 'all' | ActivityKind;

export const KIND_FILTERS: { id: ActivityKindFilter; label: string }[] = [
	{ id: 'all', label: 'All kinds' },
	{ id: 'work', label: 'Work' },
	{ id: 'conversation', label: 'Conversations' },
	{ id: 'decision', label: 'Decisions' },
	{ id: 'run', label: 'Runs' },
	{ id: 'org', label: 'Org changes' }
];

/** One word for a row's kind, shown beside its text. */
export const KIND_WORD: Record<ActivityKind, string> = {
	work: 'Work',
	conversation: 'Conversation',
	decision: 'Decision',
	run: 'Run',
	org: 'Org change'
};

/** What a row did. Consecutive rows with the same actor and action collapse into one group. */
export type ActivityAction =
	| 'message'
	| 'broadcast'
	| 'status'
	| 'task'
	| 'task-done'
	| 'bug'
	| 'comment'
	| 'merge'
	| 'run'
	| 'team'
	| 'member'
	| 'decision';

export interface ActivityActor {
	id: string;
	name: string;
	kind: 'agent' | 'person';
}

/** A recorded run, expandable in place (our-org-in-the-app §6.1). Only recorded runs appear. */
export interface ActivityRun {
	id: string;
	memberId: string;
	state: 'running' | 'succeeded' | 'failed';
	started_at: string;
	ended_at?: string;
	/** What started it, in words: "Assigned T-12", "Message from Alex Kim". */
	trigger: string;
	/** The result in words; absent while running. */
	outcome?: string;
	/** The supported log summary: a few lines, never the log. */
	log?: string[];
}

export interface ActivityRow {
	id: string;
	/** An instant. */
	at: string;
	actor: ActivityActor;
	kind: ActivityKind;
	action: ActivityAction;
	/** What happened, as a sentence after the actor's name: "sent a message to Mira Castillo". */
	text: string;
	/** A message preview or a status reason, quoted under the sentence. */
	preview?: string;
	/** Where the row's subject is, when it has a page: a work item, a project, a member. */
	href?: { path: string; label: string };
	teamIds: string[];
	projectIds: string[];
	/** Everyone it concerns besides the actor. */
	memberIds: string[];
	/** A status change: hidden by default as noisy. */
	status?: boolean;
	/** An authorized message the owner may read in the side panel. */
	thread?: { id: string; with: string };
	run?: ActivityRun;
}

/** What the feed is asked for. Team, member and project narrow it; `kind` and `q` filter it. */
export interface ActivityQuery {
	team?: string;
	member?: string;
	project?: string;
	kind: ActivityKindFilter;
	q?: string;
	/** Include Status rows, which the default hides. */
	includeStatus?: boolean;
}

/** The fixed scope of an embedded feed: a team's, a project's or a member's. */
export type ActivityScope = Pick<ActivityQuery, 'team' | 'member' | 'project'>;

export interface ActivitySources {
	messaging: 'connected' | 'not-connected';
	git: 'ok' | 'paused';
}

export interface ActivityPage {
	/** Newest first, at most `limit`. */
	rows: ActivityRow[];
	/** How many older matching rows were left out. */
	more: number;
	/** The organization has no activity at all, before any filter. */
	organizationEmpty: boolean;
	sources: ActivitySources;
}

export interface ActivityOptions {
	teams: { id: string; name: string }[];
	members: { id: string; name: string }[];
	projects: { id: string; name: string }[];
}

export interface ThreadMessage {
	id: string;
	at: string;
	from: ActivityActor;
	/** Absent when this message is not visible to the owner (per-message authorization). */
	body?: string;
}

export interface ActivityThread {
	id: string;
	/** The agent the owner would message to weigh in. */
	counterpart: ActivityActor;
	messages: ThreadMessage[];
	/** The owner is a participant allowed to reply: the thread then points at Chat instead of the read-only line. */
	ownerMayReply: boolean;
}
