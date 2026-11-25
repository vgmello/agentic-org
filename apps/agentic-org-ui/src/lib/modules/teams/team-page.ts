// A team's page (offices-and-teams-ux §7.2) as pure functions: its tabs and which one a URL shows, the
// members shown above the tabs, the header's words, and the read that tells "not found" apart from a failure.
import { toAppError } from '$lib/proto';
import type { TeamDetail, TeamMember } from './types';

/** The tabs, in order. Overview is the default at the team's URL (Prototype Release 1149b4f1). */
export const TEAM_TABS = [
	{ id: 'overview', label: 'Overview', path: '' },
	{ id: 'members', label: 'Members', path: '/members' },
	{ id: 'work', label: 'Work', path: '/work' },
	{ id: 'projects', label: 'Projects', path: '/projects' },
	{ id: 'activity', label: 'Activity', path: '/activity' },
	{ id: 'conversations', label: 'Conversations', path: '/conversations' },
	{ id: 'processes', label: 'Processes', path: '/processes' }
] as const;
export type TeamTabId = (typeof TEAM_TABS)[number]['id'];

export const teamHref = (teamId: string, tab: TeamTabId = 'overview') =>
	`/teams/${encodeURIComponent(teamId)}${TEAM_TABS.find((t) => t.id === tab)!.path}`;

/** Which tab a pathname under `/teams/<id>` shows; anything else is Overview. */
export function tabFor(pathname: string, teamId: string): (typeof TEAM_TABS)[number] {
	const base = teamHref(teamId);
	const rest = pathname.startsWith(base) ? pathname.slice(base.length).replace(/\/$/, '') : '';
	return TEAM_TABS.find((t) => t.path === rest) ?? TEAM_TABS[0];
}

/** At most six member rows sit above the tabs; the rest are one step away behind "All N members". */
export const MEMBERS_SHOWN = 6;

export function shownMembers(members: TeamMember[], expanded: boolean): TeamMember[] {
	return expanded ? members : members.slice(0, MEMBERS_SHOWN);
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** "2 people", "1 agent": the team's real home-team counts (TEAM-2). */
export const peopleText = (team: TeamDetail['team']) => plural(team.people, 'person', 'people');
export const agentsText = (team: TeamDetail['team']) => plural(team.agents, 'agent', 'agents');

/** "Members: 5 of 6" against `max_members`. */
export const membersOfMax = (detail: TeamDetail) =>
	`${detail.members.length} of ${detail.team.maxMembers}`;

/** A team that isn't there is its own state, not a failure to retry (§7.10). */
export type TeamLookup = { found: true; detail: TeamDetail } | { found: false; message: string };

export async function lookUpTeam(read: () => Promise<TeamDetail>): Promise<TeamLookup> {
	try {
		return { found: true, detail: await read() };
	} catch (e) {
		const failure = toAppError(e);
		if (failure.source === 'api' && failure.code === 'not_found')
			return { found: false, message: failure.message };
		throw e;
	}
}
