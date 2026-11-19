import { unavailableCall } from '$lib/platform/unavailable-api';
import type {
	ActivityEntry,
	ConversationRow,
	NewProject,
	ProcessPreview,
	ProjectDetail,
	ProjectsOverview,
	ProjectTeamView,
	RepositoryChoice,
	TeamChoice
} from './types';

/**
 * What the Projects screens read and change (work-items-ux §3, offices-and-teams-ux §7.9, development-processes §3).
 * Refusals reject with an AppError: `not_found` for a project that isn't there, `conflict` when the project changed
 * since `expectedRevision` (the caller re-reads and keeps what was typed), `not_authorized`, and a refusal about one
 * field carries `field` (a `ProjectField`) so the form shows it there. Every change is one accepted operation at HQ.
 */
export interface ProjectsApi {
	list(organizationId: string): Promise<ProjectsOverview>;
	project(organizationId: string, projectId: string): Promise<ProjectDetail>;
	team(organizationId: string, projectId: string): Promise<ProjectTeamView>;
	/** Newest first. */
	activity(organizationId: string, projectId: string): Promise<ActivityEntry[]>;
	/** Only the threads the viewer may read (PRJ-2). */
	conversations(organizationId: string, projectId: string): Promise<ConversationRow[]>;
	/** Active teams a project may name, with path and reference. */
	teamChoices(organizationId: string): Promise<TeamChoice[]>;
	/** Code repositories registered with the organization. */
	repositoryChoices(organizationId: string): Promise<RepositoryChoice[]>;
	/** Creates the project (and, by default, its team) in one operation. Idempotent on `draft.operationId`. */
	create(organizationId: string, draft: NewProject): Promise<{ projectId: string; number: number }>;
	rename(
		organizationId: string,
		projectId: string,
		change: { name: string; slug: string; expectedRevision: number }
	): Promise<void>;
	/** How many of the project's tasks would move their accountable team (§7.9: previewed before changing owner). */
	ownerChangePreview(
		organizationId: string,
		projectId: string,
		teamId: string
	): Promise<{ tasks: number }>;
	/** Changes the accountable team and every project task's `team_id` together; assignments stay. */
	setOwner(
		organizationId: string,
		projectId: string,
		change: { teamId: string; expectedRevision: number }
	): Promise<void>;
	setContributors(
		organizationId: string,
		projectId: string,
		change: { teamIds: string[]; expectedRevision: number }
	): Promise<void>;
	/** Replaces the project's repository associations; each needs its integration branch. */
	setRepositories(
		organizationId: string,
		projectId: string,
		change: {
			repositories: { repositoryId: string; integrationBranch: string }[];
			expectedRevision: number;
		}
	): Promise<void>;
	/** What choosing `processId` would create and use; `projectId` null for a project not yet created. */
	processPreview(
		organizationId: string,
		projectId: string | null,
		processId: string
	): Promise<ProcessPreview>;
	/** Sets or clears `development_process_id`; missing roles and sections are added in the same change. */
	setProcess(
		organizationId: string,
		projectId: string,
		change: { processId: string | null; expectedRevision: number }
	): Promise<void>;
}

// The web API does not serve projects yet: every call refuses, and the pages say so.
// Under `bun run dev:mock`, e2e/fakes/modules/projects.ts stands in for this file.
export const projectsApi: ProjectsApi = {
	list: unavailableCall,
	project: unavailableCall,
	team: unavailableCall,
	activity: unavailableCall,
	conversations: unavailableCall,
	teamChoices: unavailableCall,
	repositoryChoices: unavailableCall,
	create: unavailableCall,
	rename: unavailableCall,
	ownerChangePreview: unavailableCall,
	setOwner: unavailableCall,
	setContributors: unavailableCall,
	setRepositories: unavailableCall,
	processPreview: unavailableCall,
	setProcess: unavailableCall
};
