import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { orgApi } from '$lib/modules/organization/api';
import { workItemsApi } from '$lib/modules/workitems/api';
import { createHttpTeamsApi } from './http-api';
import type {
	ActivityEntry,
	ConversationRow,
	NewTeamPerson,
	TeamDetail,
	TeamProjectRow,
	TeamRow,
	WorkRow
} from './types';

/**
 * What the Teams index and a team's page read, archiving or deleting a team and adding a person to it. Moves, renames
 * and leads go through `orgApi`.
 */
export interface TeamsApi {
	/** Every team, active and archived, in any order; the screen builds the tree. */
	list(organizationId: string, signal?: AbortSignal): Promise<TeamRow[]>;
	team(organizationId: string, teamId: string, signal?: AbortSignal): Promise<TeamDetail>;
	work(organizationId: string, teamId: string, signal?: AbortSignal): Promise<WorkRow[]>;
	projects(organizationId: string, teamId: string, signal?: AbortSignal): Promise<TeamProjectRow[]>;
	activity(organizationId: string, teamId: string, signal?: AbortSignal): Promise<ActivityEntry[]>;
	conversations(
		organizationId: string,
		teamId: string,
		signal?: AbortSignal
	): Promise<ConversationRow[]>;
	/**
	 * Archive keeps the team's ID and history (§7.10). Refused (`conflict`, the blockers named) while teams under it,
	 * home employees, projects it owns or its open tasks remain.
	 */
	archive(organizationId: string, teamId: string): Promise<void>;
	/** Delete (TEAM-9): refused the same way, and also while archived teams still sit under it. No cascade. */
	delete(organizationId: string, teamId: string): Promise<void>;
	/**
	 * Add a person with this team as their one home team (PEOPLE-3/4): an explicit handle, unique across people and
	 * agents. A refused handle (`already_exists` with a suggestion, or `invalid_request`) carries `field: 'handle'`,
	 * so the form shows it on the Handle field.
	 */
	addPerson(organizationId: string, person: NewTeamPerson): Promise<{ memberId: string }>;
}

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

const gatewayTeamsApi = createHttpTeamsApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal),
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
	},
	orgApi,
	workItemsApi
});

// Production teams API backed by `/api/v1/organizations/{org}/snapshot` and projects/team mutations.
// Under `bun run dev:mock`, e2e/fakes/modules/teams.ts stands in for this file.
export const teamsApi: TeamsApi = Object.freeze<TeamsApi>({
	list: (orgId, signal) =>
		signedInSession() ? gatewayTeamsApi.list(orgId, signal) : unavailableCall(),
	team: (orgId, teamId, signal) =>
		signedInSession() ? gatewayTeamsApi.team(orgId, teamId, signal) : unavailableCall(),
	work: (orgId, teamId, signal) =>
		signedInSession() ? gatewayTeamsApi.work(orgId, teamId, signal) : unavailableCall(),
	projects: (orgId, teamId, signal) =>
		signedInSession() ? gatewayTeamsApi.projects(orgId, teamId, signal) : unavailableCall(),
	activity: (orgId, teamId, signal) =>
		signedInSession() ? gatewayTeamsApi.activity(orgId, teamId, signal) : unavailableCall(),
	conversations: (orgId, teamId, signal) =>
		signedInSession() ? gatewayTeamsApi.conversations(orgId, teamId, signal) : unavailableCall(),
	archive: unavailableCall,
	delete: (orgId, teamId) =>
		signedInSession() ? gatewayTeamsApi.delete(orgId, teamId) : unavailableCall(),
	addPerson: (orgId, person) =>
		signedInSession() ? gatewayTeamsApi.addPerson(orgId, person) : unavailableCall()
});
