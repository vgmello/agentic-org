import { unavailableCall } from '$lib/platform/unavailable-api';
import type { ActivityEntry, ConversationRow, ProcessRow, TeamDetail, TeamProjectRow, TeamRow, WorkRow } from './types';

/** What the Teams index and a team's page read, and archiving a team. Moves, renames and leads go through `orgApi`. */
export interface TeamsApi {
	/** Every team, active and archived, in any order; the screen builds the tree. */
	list(organizationId: string): Promise<TeamRow[]>;
	team(organizationId: string, teamId: string): Promise<TeamDetail>;
	work(organizationId: string, teamId: string): Promise<WorkRow[]>;
	projects(organizationId: string, teamId: string): Promise<TeamProjectRow[]>;
	activity(organizationId: string, teamId: string): Promise<ActivityEntry[]>;
	conversations(organizationId: string, teamId: string): Promise<ConversationRow[]>;
	processes(organizationId: string, teamId: string): Promise<ProcessRow[]>;
	/** Archive keeps the team's ID and history (§7.10). Rejects with the blockers named. */
	archive(organizationId: string, teamId: string): Promise<void>;
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
	archive: unavailableCall
};
