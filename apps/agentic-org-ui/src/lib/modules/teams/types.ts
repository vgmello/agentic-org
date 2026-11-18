// What the Teams index and a team's page read (offices-and-teams-ux §7.2). Field names follow the team contract
// (docs/design/organization-data/01-organization-structure.md): `number` gives the readable `TEAM-<n>` reference,
// `max_members` the member limit. Counts are of active employees whose home team it is (TEAM-2).
import type { MemberRef, PresenceId, TeamRef } from '$lib/modules/members/types';

export type {
	ActivityEntry,
	ConversationRow,
	ProcessRow,
	WorkRow
} from '$lib/modules/members/types';

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
	openWork: number;
}

/** Someone who may lead a team: an active employee whose home team it is. */
export interface LeadCandidate {
	id: string;
	name: string;
	kind: 'agent' | 'person';
}
