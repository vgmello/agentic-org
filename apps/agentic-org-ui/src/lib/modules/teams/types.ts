// What the Teams index and a team's page read (offices-and-teams-ux §7.2). Field names follow the team contract
// (docs/design/organization-data/01-organization-structure.md): `number` gives the readable `TEAM-<n>` reference,
// `max_members` the member limit. Counts are of active employees whose home team it is (TEAM-2).
import type { MemberRef, PresenceId, TeamRef } from '$lib/modules/members';
import type { ProjectRow } from '$lib/modules/projects/types';

export type { ActivityEntry, ConversationRow, WorkRow } from '$lib/modules/members';

export interface TeamProject {
	id: string;
	name: string;
	/** The accountable owner team, or a contributing team (TEAM-13). */
	role: 'owner' | 'contributor';
}

/** One row of the Teams index. */
export interface TeamRow extends TeamRef {
	parentId?: string;
	lead?: MemberRef;
	people: number;
	agents: number;
	projects: TeamProject[];
	/** `max_members`: 6 unless changed (§7.1). */
	maxMembers: number;
	status: 'active' | 'archived';
}

/** A member chip on a team page: up to six above the tabs. */
export interface TeamMember {
	id: string;
	name: string;
	kind: 'agent' | 'person';
	role?: string;
	handle: string;
	presence?: PresenceId;
	lead: boolean;
}

export interface TeamDetail {
	team: TeamRow;
	/** The team's path from the top-level team, for "Part of". */
	path: TeamRef[];
	purpose?: string;
	/** Everyone whose home team this is, the lead first. */
	members: TeamMember[];
}

/** A team's project, as the Projects tab lists it. */
export interface TeamProjectRow extends TeamProject {
	key?: string;
	/** The accountable owner team when this team only contributes. */
	ownerTeam?: TeamRef;
	/** The other contributing teams: neither this team nor the owner (TEAM-13). */
	contributors?: TeamRef[];
	/** Real open-task count when served by HQ; omitted when unavailable. */
	openWork?: number;
	/** Real project task counts when served by an authorized read; omitted when unavailable. */
	tasks?: ProjectRow['tasks'];
	/** Real project bug counts when served by an authorized read; omitted when unavailable. */
	bugs?: ProjectRow['bugs'];
}

/** Someone who may lead a team: an active employee whose home team it is. */
export interface LeadCandidate {
	id: string;
	name: string;
	kind: 'agent' | 'person';
}

/** What a team's menu opens (§7.2): each is one reviewed dialog. */
export type TeamAction = 'move' | 'rename' | 'lead' | 'add-team' | 'archive' | 'delete';

/** A person added from a team's page (PEOPLE-3, §7.4): one home team, an explicit handle. */
export interface NewTeamPerson {
	name: string;
	/** Lowercase, stored without `@`. */
	handle: string;
	homeTeamId: string;
	contact?: string;
	/** An explicit employee manager; omitted is No explicit manager. */
	reportsToId?: string;
}
