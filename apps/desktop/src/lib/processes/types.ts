// Processes (docs/specs/processes-ux.md rev 3, docs/design/processes.md): how agents work, written down and inherited.
// A process is a Markdown file with front matter in a repository; here it is the file's front matter and its text.

/** Organization, then the team (a chain of nested teams), then the project; an agent's own private processes add to them. */
export type Level = 'organization' | 'team' | 'project' | 'agent';

export const LEVEL_LABEL: Record<Level, string> = { organization: 'Organization', team: 'Team', project: 'Project', agent: 'Agent' };

export type ProcessStatus = 'draft' | 'active' | 'off' | 'retired';

/** The guidance moments a process may add a short prompt to (decisions-and-tech-debt-ux §6.2). */
export interface Moments {
	starting_task?: string;
	waking_up?: string;
	finishing_task?: string;
}

export interface ChecklistStep {
	text: string;
	required: boolean;
}

/** Where a file lives, which fixes the level and scope it must declare (design §2.2). */
export interface Location {
	level: Level;
	/** The team id, project key or member id; none for the organization. */
	scope?: string;
}

/** A front matter field that could not be read (PROC-16): its name and why. */
export interface FieldProblem {
	field: string;
	message: string;
}

export interface ProcessDoc {
	/** The doc id like every doc's (`doc-<ULID>`): different for each level's file of one process. */
	id: string;
	/** The stable key that matches a process across levels. */
	key: string;
	name: string;
	level: Level;
	scope?: string;
	version: number;
	owner: string;
	summary: string;
	appliesTo?: string;
	/** False: sealed, applies as written at every lower level. */
	overridable: boolean;
	status: ProcessStatus;
	/** For an override, the inherited key and version it was written against (drift). */
	overrides?: { key: string; version: number };
	/** Why a lower level turned the process off (`status: off`). */
	reason?: string;
	moments?: Moments;
	checklist?: ChecklistStep[];
	body: string;
	/** Who wrote this version and who approved it (an off counts only when approved by someone else). */
	author?: string;
	approvedBy?: string;
	/** Where the file actually is: a level or scope that differs from the front matter is a problem. */
	location: Location;
	/** Front matter that could not be read: the file is listed with this and left out of every set. */
	problems?: FieldProblem[];
}

export interface TeamRef {
	id: string;
	name: string;
	owner_team_id?: string;
	lead_member_id?: string;
}

export interface ProjectRef {
	key: string;
	name: string;
	/** The teams the project belongs to (a shared project has several). */
	teamIds: string[];
}

export interface ResolveInput {
	docs: ProcessDoc[];
	teams: TeamRef[];
	/** The project of the work, when there is current work. */
	project?: ProjectRef;
	/** The agent, whose private processes join the set. */
	agentId?: string;
}

/** What happened to one level's file of a process, for `process why` (PROC-8). */
export interface Layer {
	level: Level;
	/** The team's name, the project's name or the agent's id. */
	scopeName?: string;
	doc: ProcessDoc;
	outcome: 'applies' | 'overridden' | 'refused' | 'left_out' | 'turned_off' | 'not_applied';
	/** In words: why it lost, was refused or left out. */
	note?: string;
}

export type EntryKind = 'inherited' | 'override' | 'addition';

export interface Drift {
	/** The version the override was written against, and what its parent is now. */
	wrote: number;
	parentVersion: number;
	parentLevel: Level;
	parentName: string;
}

export interface Entry {
	key: string;
	doc: ProcessDoc;
	kind: EntryKind;
	/** The level the winning file comes from, and its team or project name. */
	source: Level;
	scopeName?: string;
	sealed: boolean;
	/** "Sealed by <team>; applies until the project decides": a sealed version applying while a sibling team disagrees. */
	sealedNote?: string;
	drift?: Drift;
	/** The layers that had this key, highest first, and what happened to each. */
	chain: Layer[];
}

export interface TurnedOff {
	key: string;
	name: string;
	level: Level;
	scopeName?: string;
	reason: string;
	doc: ProcessDoc;
}

export interface Conflict {
	key: string;
	/** The teams whose versions disagree. */
	teams: string[];
	/** What applies meanwhile, in words. */
	applies: string;
	doc?: ProcessDoc;
}

export type ProblemKind = 'invalid' | 'mismatch' | 'clash' | 'sealed_refused' | 'private_only_adds';

export interface Problem {
	kind: ProblemKind;
	key: string;
	doc: ProcessDoc;
	/** In the spec's words. */
	text: string;
	/** The other file of a clash. */
	other?: ProcessDoc;
}

export interface EffectiveSet {
	entries: Entry[];
	turnedOff: TurnedOff[];
	conflicts: Conflict[];
	problems: Problem[];
}
