// The profile's tabs (our-org-in-the-app §5): every member has Overview, Work, Conversations and Activity; agents
// also have Runs, Processes, Role and Notes and memory; Cost only for a viewer who may see amounts (COST-7).
import type { MemberProfile } from '../../types';

export type ProfileTab =
	| 'overview'
	| 'work'
	| 'conversations'
	| 'activity'
	| 'runs'
	| 'processes'
	| 'role'
	| 'notes'
	| 'cost';

export const TAB_LABEL: Record<ProfileTab, string> = {
	overview: 'Overview',
	work: 'Work',
	conversations: 'Conversations',
	activity: 'Activity',
	runs: 'Runs',
	processes: 'Processes',
	role: 'Role',
	notes: 'Notes and memory',
	cost: 'Cost'
};

const ALL: ProfileTab[] = [
	'overview',
	'work',
	'conversations',
	'activity',
	'runs',
	'processes',
	'role',
	'notes',
	'cost'
];
const AGENT_ONLY: ProfileTab[] = ['runs', 'processes', 'role', 'notes', 'cost'];

export const profileBase = (memberId: string) => `/members/${encodeURIComponent(memberId)}`;

export function tabsFor(p: Pick<MemberProfile, 'kind' | 'viewer'>): ProfileTab[] {
	return ALL.filter((tab) => {
		if (p.kind !== 'agent' && AGENT_ONLY.includes(tab)) return false;
		if (tab === 'cost') return p.viewer.costVisible;
		return true;
	});
}

export const tabHref = (memberId: string, tab: ProfileTab) =>
	tab === 'overview' ? profileBase(memberId) : `${profileBase(memberId)}/${tab}`;

/** Which tab a path is on: `/members/noor/runs` → runs; anything unknown is Overview. */
export function tabOf(pathname: string, memberId: string): ProfileTab {
	const rest = pathname.slice(profileBase(memberId).length).split('/').filter(Boolean)[0];
	return rest && Object.hasOwn(TAB_LABEL, rest) ? (rest as ProfileTab) : 'overview';
}
