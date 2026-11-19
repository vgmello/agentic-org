import { unavailableCall } from '$lib/platform/unavailable-api';
import type {
	ActivityEntry,
	ConversationRow,
	NewTeamPerson,
	ProcessRow,
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
	list(organizationId: string): Promise<TeamRow[]>;
	team(organizationId: string, teamId: string): Promise<TeamDetail>;
	work(organizationId: string, teamId: string): Promise<WorkRow[]>;
	projects(organizationId: string, teamId: string): Promise<TeamProjectRow[]>;
	activity(organizationId: string, teamId: string): Promise<ActivityEntry[]>;
	conversations(organizationId: string, teamId: string): Promise<ConversationRow[]>;
	processes(organizationId: string, teamId: string): Promise<ProcessRow[]>;
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

// The web API does not serve these yet: every call refuses. Under `bun run dev:mock`,
// e2e/fakes/modules/teams.ts stands in for this file.
export const teamsApi: TeamsApi = {
	list: unavailableCall,
	team: unavailableCall,
	work: unavailableCall,
	projects: unavailableCall,
	activity: unavailableCall,
	conversations: unavailableCall,
	processes: unavailableCall,
	archive: unavailableCall,
	delete: unavailableCall,
	addPerson: unavailableCall
};
