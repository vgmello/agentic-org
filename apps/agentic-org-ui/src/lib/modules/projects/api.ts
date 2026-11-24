import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { orgApi } from '$lib/modules/organization/api';
import { createHttpProjectsApi } from './http-api';
import type {
	ActivityEntry,
	CreateOutcome,
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
	/**
	 * Creates the project (and, by default, its team) in one operation. Idempotent on `draft.operationId`: a repeat
	 * answers with the project it created. A lost answer rejects with `OUTCOME_UNKNOWN` (./outcome.ts).
	 */
	create(organizationId: string, draft: NewProject): Promise<{ projectId: string; number: number }>;
	/** Where a create whose outcome was unknown stands, by its operation ID. */
	createOperation(organizationId: string, operationId: string): Promise<CreateOutcome>;
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
	/**
	 * Sets the accountable team and the contributing teams in one accepted operation (offices-and-teams-ux §7.9): when
	 * the accountable team changes, every project task's `team_id` moves with it, with history; assignments stay. It is
	 * applied whole or not at all, against `expectedRevision`; the caller re-reads afterwards and never guesses the new
	 * revision.
	 */
	setTeams(
		organizationId: string,
		projectId: string,
		change: { ownerTeamId: string; contributorTeamIds: string[]; expectedRevision: number }
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

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

const gatewayProjectsApi = createHttpProjectsApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal),
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
	},
	orgApi,
	instanceId: () => signedInSession()?.instance_id,
	newId: () => appSession.newOperationId()
});

// Production projects API backed by `/api/v1/organizations/{org}/projects`.
// Under `bun run dev:mock`, e2e/fakes/modules/projects.ts stands in for this file.
export const projectsApi: ProjectsApi = Object.freeze<ProjectsApi>({
	list: (orgId) => (signedInSession() ? gatewayProjectsApi.list(orgId) : unavailableCall()),
	project: (orgId, id) =>
		signedInSession() ? gatewayProjectsApi.project(orgId, id) : unavailableCall(),
	team: (orgId, id) => (signedInSession() ? gatewayProjectsApi.team(orgId, id) : unavailableCall()),
	activity: (orgId, id) =>
		signedInSession() ? gatewayProjectsApi.activity(orgId, id) : unavailableCall(),
	conversations: (orgId, id) =>
		signedInSession() ? gatewayProjectsApi.conversations(orgId, id) : unavailableCall(),
	teamChoices: (orgId) =>
		signedInSession() ? gatewayProjectsApi.teamChoices(orgId) : unavailableCall(),
	repositoryChoices: (orgId) =>
		signedInSession() ? gatewayProjectsApi.repositoryChoices(orgId) : unavailableCall(),
	create: (orgId, draft) =>
		signedInSession() ? gatewayProjectsApi.create(orgId, draft) : unavailableCall(),
	createOperation: (orgId, opId) =>
		signedInSession() ? gatewayProjectsApi.createOperation(orgId, opId) : unavailableCall(),
	rename: unavailableCall,
	ownerChangePreview: (orgId, id, teamId) =>
		signedInSession()
			? gatewayProjectsApi.ownerChangePreview(orgId, id, teamId)
			: unavailableCall(),
	setTeams: (orgId, id, change) =>
		signedInSession() ? gatewayProjectsApi.setTeams(orgId, id, change) : unavailableCall(),
	setRepositories: unavailableCall,
	processPreview: unavailableCall,
	setProcess: unavailableCall
});
