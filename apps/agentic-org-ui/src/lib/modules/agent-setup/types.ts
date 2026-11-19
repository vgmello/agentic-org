// What Organization settings › Agent setup reads and changes (our-org-in-the-app §5; agent-run-modes-ux §1, §2;
// employee-supervision-ux §1). Shapes follow docs/design/organization-data: the employee record (document 04,
// `profile.md`, its own revision) and the execution configuration (document 11, `execution.md`, its own revision)
// stay apart; office placement and readiness are the owning office's observations, with when they were made.

export interface Ref {
	id: string;
	name: string;
}

/** The agent's `execution.md` as the list needs it. `null` on the agent: Execution not configured. */
export interface ExecutionSummary {
	/** `model_profile_id` and the profile's name. */
	profile: Ref;
	/** The canonical `model_id`. */
	modelId: string;
	/** `enabled: false` means the office admits no turn for this agent. */
	enabled: boolean;
	/** The runtimes the profile's connections drive, in priority order, as words: ["Claude Code"]. */
	runtimes: string[];
}

/**
 * The model list the agent's runtime reported on its office (document 11 `office_connection_models`), for the
 * strict missing-model predicate. `reported: false` when the office has no fresh, readable list.
 */
export type ModelCatalog = { reported: true; models: string[]; asOf: string } | { reported: false };

/** One reason the office gives that the agent can't start, with the fix when one is known. */
export interface Blocker {
	/** Stable code from document 11 §8: BINDING_MISSING, AUTH_EXPIRED, DRIVER_UNAVAILABLE, … */
	code: string;
	/** In words: "Codex isn't signed in on Studio Mac". */
	reason: string;
	/** "Sign in there with `codex login`." */
	fix?: string;
}

/**
 * What the owning office reports about running the agent (GetAgentReadiness). `known: false` when it can't be
 * told (no office to ask, the office unreachable, or this server doesn't report readiness): never read as ready.
 */
export type Readiness =
	{ known: true; checkedAt: string; blockers: Blocker[] } | { known: false; why: string };

/** The agent's current placement (document 04 placement row, document 11 generation). `null` is Unplaced. */
export interface Placement {
	office: Ref;
	generation: number;
	/** RFC 3339: when the office last reported this agent. */
	observedAt?: string;
}

/** One agent in the list. */
export interface AgentSetupRow {
	memberId: string;
	name: string;
	/** Stored without `@`. */
	handle: string;
	/** The referenced role's name (`role_id`). */
	role?: string;
	/** `role_text`, kept apart from `role_id`: what was written when no role is referenced. */
	roleText?: string;
	/** The built-in HR lead: the only agent that isn't hired; it can't be offboarded or renamed. */
	builtIn: boolean;
	/** Employment status from `profile.md`. Former members are not listed. */
	status: 'active' | 'paused';
	/** Required on a valid record: missing means an invalid record, shown as a problem. */
	homeTeam?: Ref;
	execution: ExecutionSummary | null;
	placement: Placement | null;
	readiness: Readiness;
	catalog: ModelCatalog;
}

export interface AgentSetupViewer {
	/** The owner and the administrator change agent setup; everyone else reads it. */
	canChange: boolean;
}

export interface AgentSetupList {
	agents: AgentSetupRow[];
	viewer: AgentSetupViewer;
}

/** A connection's state on the agent's office, as the office last checked it. */
export type ConnectionState =
	'ready' | 'signed_out' | 'not_installed' | 'binding_missing' | 'cooling_down' | 'unknown';

/** One connection of the agent's model profile, in priority order, with what its office observed. */
export interface ConnectionView {
	id: string;
	name: string;
	/** The runtime driver it selects, as a word. */
	runtime: string;
	state: ConnectionState;
	/** RFC 3339, when the office checked it; absent when it never has. */
	checkedAt?: string;
}

/** A model a profile offers, as ListProfileModels reports it on the agent's office (document 11 §4). */
export interface ProfileModel {
	id: string;
	/** Connections in the profile that report this model, fresh or not. */
	supporting: number;
	/** Of those, the ones with fresh local evidence: not a promise of capacity. */
	ready: number;
	/** The office's evidence is older than 300 s. */
	stale: boolean;
}

/** A model profile the agent can be given. */
export interface ProfileChoice extends Ref {
	runtimes: string[];
	/** `null` when the office has no model list for this profile yet: the model can't be chosen from a list. */
	models: ProfileModel[] | null;
}

export interface Attribution {
	byName?: string;
	/** RFC 3339. */
	at?: string;
}

/** The execution configuration in full, with its own revision (`execution.md`). */
export interface ExecutionDetail extends ExecutionSummary {
	revision: number;
	connections: ConnectionView[];
	/** Approved tool IDs (`tools_config`). */
	tools: string[];
	/** Whether the file has a system prompt body; its text isn't shown here. */
	hasPrompt: boolean;
	updated?: Attribution;
}

/** One agent's setup page. */
export interface AgentSetupDetail extends Omit<AgentSetupRow, 'execution'> {
	execution: ExecutionDetail | null;
	/** The employee record's revision (`profile.md`): role changes are checked against it. */
	employeeRevision: number;
	/** `role_id` when a stable role is referenced. */
	roleId?: string;
	reportsTo?: Ref;
	/** What the office observed actually running: "Claude Code 2.1.4". */
	observedRuntime?: string;
	/** The managed home, from the same office resolver used by launch. Read-only. */
	managedHome?: string;
	/** Current manual, security and budget holds, in words. */
	holds: string[];
	/** HQ accepted a Pause the office hasn't confirmed yet. */
	stopPending: boolean;
	profiles: ProfileChoice[];
	roles: Ref[];
	viewer: AgentSetupViewer;
}

/** An execution change, against the revision the person saw. */
export interface ExecutionEdit {
	profileId: string;
	modelId: string;
	expectedRevision: number;
}

/** A role change on the employee record, against the revision the person saw. `null` clears the role. */
export interface RoleEdit {
	roleId: string | null;
	expectedRevision: number;
}
