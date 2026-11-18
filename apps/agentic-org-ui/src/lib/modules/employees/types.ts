// Hire requests (our-org-in-the-app §7.1; `hire_requests` in docs/design/organization-data/04-agent-structure.md).
import type { MemberRef, TeamRef } from '$lib/modules/members/types';

export type HireState = 'drafting' | 'hiring' | 'hired' | 'withdrawn' | 'failed';

export interface HireRequest {
	id: string;
	state: HireState;
	/** The proposed agent's name, once there is one. */
	name?: string;
	role?: string;
	requestedBy: MemberRef;
	homeTeam: TeamRef;
	createdAt: string;
	updatedAt: string;
	/** Hiring stopped partway: the step that failed ("couldn't register with messaging"). */
	failedStep?: string;
	/** Steps already done, named when hiring stopped partway. */
	doneSteps?: string[];
	closedReason?: string;
	employeeId?: string;
}

/** Where the viewer may hire (APPR-1): the teams the home-team picker lists. Empty means refused everywhere. */
export interface HireAccess {
	teamIds: string[];
	/** Whether the viewer sees everyone's requests (the owner) or only its own. */
	seesAll: boolean;
}
