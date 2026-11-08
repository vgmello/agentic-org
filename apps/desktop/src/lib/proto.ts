// Hand-written mirror of the `org-proto` crate's types (crates/org-proto/src/{types,calls,error}.rs).
// Keep every wire type in this one file, so swapping in generated types touches nothing else.

/** An office's role in its organization. Missing or unrecognised means `unknown`, never HQ. */
export type OfficeRole = 'hq' | 'satellite' | 'unknown';

/** Missing or unrecognised means `unknown`, never healthy. */
export type OfficeStatus =
	| 'active'
	| 'starting'
	| 'pending'
	| 'offline'
	| 'unavailable'
	| 'update_needed'
	| 'unknown';

/** An office: a node of one organization, run by its own server process. */
export interface OfficeInfo {
	id: string;
	organization_id: string;
	name: string;
	role?: OfficeRole;
	/** Where it runs, for display: "This Mac" or a machine name. */
	host: string;
	/** How to reach it from this machine, when it is on this machine. */
	endpoint?: string;
	status?: OfficeStatus;
}

/** Set when an office server answers `hello`. */
export interface OfficeHello {
	organization_name: string;
	office: OfficeInfo;
}

export interface HelloResult {
	api_version: number;
	server_version: string;
	/** The machine, for display: "This Mac". */
	host: string;
	/** Absent when this machine's host answers. */
	office?: OfficeHello;
}

export interface Organization {
	id: string;
	name: string;
	/** RFC 3339, UTC, with milliseconds. Ids are opaque (ULIDs): never parse them. */
	created_at: string;
	/** Its offices; from this machine's host, its offices on this machine (HQ first). */
	offices: OfficeInfo[];
}

/**
 * An organization and its office on this machine (HQ first), which the app is connected to.
 * App-local: remembered between launches, not part of `org-proto`.
 */
export interface Connection {
	office: OfficeInfo;
	organization: Organization;
}

export interface OrganizationCreateParams {
	name: string;
}

export interface ListOrganizationsResult {
	organizations: Organization[];
}

/**
 * Errors the office reports (`org-proto`). Treat any code you don't recognise as `unknown`:
 * the server may grow new ones.
 */
export const API_ERROR_CODES = [
	'invalid_request',
	'unknown_method',
	'invalid_name',
	'already_exists',
	'storage_error',
	'cycle',
	'not_empty',
	'not_a_member',
	'owner_protected',
	'not_found',
	'conflict',
	'snapshot_required',
	'expired',
	'clock_skew',
	'unknown_recipient',
	'not_authorized',
	'unavailable',
	'unknown'
] as const;
export type ApiErrorCode = (typeof API_ERROR_CODES)[number];

/** Errors from making the office ready. These belong to the app, not to `org-proto`. */
export type SetupErrorCode = 'not_installed' | 'needs_approval' | 'unreachable';

/** What every failed call looks like to the UI. `source` says which layer failed. */
export type AppError =
	| { source: 'api'; code: ApiErrorCode; message: string }
	| { source: 'setup'; code: SetupErrorCode; message: string }
	| { source: 'app'; code: string; message: string };

/** Trimmed, no control, invisible or direction-changing characters (OrganizationName). The office is the authority; the app only pre-checks. */
export const ORGANIZATION_NAME_MAX_CHARS = 64;

// ---- The organization's structure (crates/org-proto/src/org.rs) -----------------------------

/** A position in the organization's event log: `(epoch, seq)`, compared in that order. */
export interface Position {
	epoch: number;
	seq: number;
}

/** A team. Teams own teams (a tree). Optional lead: a member of the team. */
export interface Team {
	id: string;
	name: string;
	owner_team_id?: string;
	lead_member_id?: string;
	version: number;
}

/** A kind this build doesn't know decodes as `unknown` (a missing kind is an error on the wire). */
export type MemberKind = 'agent' | 'person' | 'unknown';

/** What an agent runs on. A runtime this build doesn't know is `unknown` (or `{ other }` for a named one). */
export type AgentRuntime = 'claude_code' | 'codex' | 'antigravity' | { other: string } | 'unknown';
/** Whether the agent takes work. `unknown` is never read as working. */
export type AgentStatus = 'active' | 'paused' | 'offboarded' | 'unknown';
/** How an agent is run. Missing means a person starts it. */
export type RunMode = 'always_on' | 'on_call' | 'scheduled' | 'unknown';

/** When a scheduled agent runs. Both may be empty if a newer writer left them out. */
export interface Schedule {
	spec: string;
	time_zone: string;
}

/** Who an agent is and how it runs (`AgentProfile` in org-proto). Set for agents, absent for people. */
export interface AgentProfile {
	role: string;
	title: string;
	focus?: string;
	runtime: AgentRuntime;
	model?: string;
	run_mode?: RunMode;
	schedule?: Schedule;
	/** An absolute path on the agent's office's machine, shown and never opened by the app. */
	home?: string;
	/** The handle agents address it by (`--to <handle>`). */
	messaging_name: string;
	status: AgentStatus;
	/** RFC 3339, set exactly when `status` is `offboarded`. */
	offboarded_at?: string;
}

/**
 * An agent or a person. The owner is a person with `owner: true`: in no team, full rights
 * everywhere. `office_id` places an agent in an office; people aren't placed in offices.
 * `reports_to_member_id` empty means directly to the owner; reporting lines are separate from teams.
 */
export interface Member {
	id: string;
	kind: MemberKind;
	name: string;
	contact?: string;
	owner?: boolean;
	office_id?: string;
	reports_to_member_id?: string;
	/** Agents only. */
	agent?: AgentProfile;
	/** RFC 3339, set by HQ when the member was created; missing on older records. */
	created_at?: string;
	version: number;
}

/** A member in a team. A member can be in any number of teams. */
export interface Membership {
	id: string;
	team_id: string;
	member_id: string;
	title_in_team?: string;
	version: number;
}

/** A team working on a project. Usually one per project; several teams can share one. */
export interface ProjectTeam {
	id: string;
	project_id: string;
	team_id: string;
	version: number;
}

/** What the charts need to show a project. */
export interface ProjectSummary {
	id: string;
	name: string;
}

/**
 * An organization's structure as the app reads it. App-local (not in `org-proto`): `sample` is true while the
 * snapshot is invented because the office behind it is the mock.
 */
export interface OrgView {
	snapshot: OrgSnapshot;
	sample: boolean;
}

/** What happened to an organization command: committed by HQ, or queued while HQ is unreachable. */
export type CommandStatus = 'committed' | 'queued' | 'unknown';

/** The answer to every organization command (an error means HQ rejected it). */
export interface CommandAccepted {
	status: CommandStatus;
	/** Set when `status` is `committed`. */
	position?: Position;
}

/** Everything the org chart needs, as of `position` (`org.snapshot`). */
export interface OrgSnapshot {
	position: Position;
	teams: Team[];
	members: Member[];
	memberships: Membership[];
	projects?: ProjectSummary[];
	project_teams?: ProjectTeam[];
}

// ---- office.app_token (crates/org-proto/src/calls.rs) -------------------------------------------

export interface OfficeAppTokenParams {
	office_id: string;
}

/**
 * `office.app_token` (the host, on its socket only): the office's token, 64 lowercase hex digits,
 * which lets the app bind to that office as a human. THE RESULT IS A SECRET: only the Rust shell
 * ever asks for it and holds it, and the WebView never receives it. If it ever must pass through
 * TypeScript: never log it, never put it in a store, a URL or storage, and drop it as soon as it is
 * used. Errors: `not_authorized` (the host's code-signature check failed), `not_found`,
 * `unavailable` (the office isn't running).
 */
export interface OfficeAppTokenResult {
	app_token: string;
}

export * from './proto-work';
