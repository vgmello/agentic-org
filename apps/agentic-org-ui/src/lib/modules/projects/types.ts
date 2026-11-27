// What the Projects screens read and change. Field names follow the project contract
// (docs/design/organization-data/02-project-structure.md: `projects`, `project_contributing_teams`,
// `project_repositories`) in the browser's camelCase: `number` gives the readable `P-<n>` reference, `slug` is the
// organization-unique slug HQ registered, `ownerTeam` is the required `owner_team_id`, `developmentProcess` the
// optional `development_process_id`. Task counts use the five task statuses (work-items-ux): never `in_review`.
import type { ActivityEntry, ConversationRow, MemberRef, TeamRef } from '$lib/modules/members';
import type { StaleCopy } from '$lib/proto';

export type { ActivityEntry, ConversationRow, StaleCopy, TeamRef };

/** The project's tasks of every type by status (`todo`, `in_progress`, `blocked`, `done`, `cancelled`). */
export interface TaskCounts {
	todo: number;
	inProgress: number;
	blocked: number;
	done: number;
	cancelled: number;
}

/** Open bug-typed tasks by severity; `unset` is a bug whose severity nobody has set (PRJ-5). */
export interface BugCounts {
	critical: number;
	major: number;
	minor: number;
	trivial: number;
	unset: number;
}

/** Features by status, for the Overview line (features-ux "Overview tile"). Retired ones are not counted. */
export interface FeatureCounts {
	planned: number;
	inProgress: number;
	live: number;
}

export interface DevelopmentProcessRef {
	id: string;
	name: string;
}

/** A tab a development process declares (`tabs[]` in its manifest, development-processes §4). */
export interface ProcessTab {
	key: string;
	title: string;
	view: 'documents' | 'work' | 'features' | 'releases';
}

/** One row of the Projects list. */
export interface ProjectRow {
	id: string;
	/** `P-<number>`. */
	number: number;
	slug: string;
	name: string;
	/** Required: a project always has its accountable team (TEAM-14). */
	ownerTeam: TeamRef;
	contributors: TeamRef[];
	archived: boolean;
	tasks: TaskCounts;
	bugs: BugCounts;
	developmentProcess: DevelopmentProcessRef | null;
}

export interface ProjectsOverview {
	projects: ProjectRow[];
	/** Creating a project is the owner's or a team lead's (offices-and-teams §5b); the server decides regardless. */
	viewer: { canCreate: boolean };
	stale?: StaleCopy;
}

/** One project/repository association, with its own required integration branch. */
export interface ProjectRepository {
	repositoryId: string;
	/** The registration's descriptive `key`. */
	key: string;
	kind: 'remote' | 'local';
	/** A remote's URL; a local-only registration has none. */
	remoteUrl?: string;
	integrationBranch: string;
}

/** The living brief (`projects/<project-id>/brief.md`). */
export interface ProjectBrief {
	markdown: string;
	/** RFC 3339. */
	updatedAt?: string;
	updatedBy?: string;
}

export interface ProjectDetail extends ProjectRow {
	/** The concurrency `revision`: every change names the one it was made against. */
	revision: number;
	brief: ProjectBrief | null;
	features: FeatureCounts;
	/** How many releases the project holds (it gets a Releases tab when it holds any, PKG-15). */
	releases: number;
	/** The tabs its development process declares, in order. */
	processTabs: ProcessTab[];
	repositories: ProjectRepository[];
	/** The owner or a lead in the owning team's lead chain changes a project (development-processes §3). */
	viewer: { canEdit: boolean };
	stale?: StaleCopy;
}

/** A person or agent on the project's teams (Team tab). */
export interface ProjectMember {
	id: string;
	name: string;
	kind: 'agent' | 'person';
	role?: string;
	homeTeam: TeamRef;
}

export interface ProjectTeamEntry extends TeamRef {
	lead?: MemberRef;
}

export interface ProjectTeamView {
	owner: ProjectTeamEntry;
	contributors: ProjectTeamEntry[];
	/** Everyone whose home team is the owner team or a contributing team, A–Z. */
	members: ProjectMember[];
}

/** A team a project may name: active, with its full path from its top-level team (§7.9). */
export interface TeamChoice extends TeamRef {
	/** "Engineering › Web". */
	path: string;
	/**
	 * The organization's top-level team, made with the organization (organization-data 01): a new project's team goes
	 * under it unless the person chooses another place (offices-and-teams-ux §7.9). HQ marks it; the browser never
	 * guesses it.
	 */
	organizationTeam: boolean;
}

/** A code repository registered with the organization (`repositories.yaml`). */
export interface RepositoryChoice {
	id: string;
	key: string;
	kind: 'remote' | 'local';
	remoteUrl?: string;
	/** The branch a new association suggests. */
	defaultBranch: string;
}

export interface NewProject {
	/** Reused when the same attempt is sent again, so a lost answer never creates a second project. */
	operationId: string;
	name: string;
	slug: string;
	/**
	 * `new`: a team named after the project, made with it in one operation (§7.9), under `parentTeamId` (the
	 * top-level team) or with no parent when none is named (organization-data 02 "Creating a project").
	 */
	owner: { kind: 'new'; parentTeamId?: string } | { kind: 'existing'; teamId: string };
	developmentProcessId: string | null;
}

/** Where a create whose answer was lost stands, asked by its operation ID. */
export type CreateOutcome =
	| { state: 'applied'; projectId: string; number: number }
	| { state: 'not_applied'; reason: string }
	| { state: 'pending' };

/** What choosing a development process would do (development-processes §3), computed by HQ before confirming. */
export interface ProcessPreview {
	/** Roles of the process the organization lacks: created, by key. */
	createsRoles: { key: string; name: string }[];
	/** Roles the organization already has with the same key: used as they are. */
	usesRoles: { key: string; name: string }[];
	/** Sections of the documentation tree the project gains. */
	addsSections: string[];
}

/** The project fields a form writes; `field` on a refusal names one of these. */
export type ProjectField = 'name' | 'slug' | 'owner' | 'repositories' | 'process';
