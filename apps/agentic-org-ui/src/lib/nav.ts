import type { IconName } from '$lib/components/icons';

export interface NavItem {
	name: string;
	href: string;
	icon: IconName;
	/** Short visible text and its spoken meaning. */
	badge?: { text: string; label: string };
}

/** The main navigation, in order. Sections without a page of their own show a placeholder. */
export const NAV: NavItem[] = [
	// Chat is the second item after Home (chat-ux §0.1); Home isn't in the nav yet, so it leads.
	{ name: 'Chat', href: '/chat', icon: 'chat' },
	{ name: 'Agents', href: '/agents', icon: 'agents' },
	{ name: 'Org chart', href: '/org-chart', icon: 'orgchart' },
	{ name: 'Projects', href: '/projects', icon: 'projects' },
	{ name: 'Work', href: '/work', icon: 'tasks' },
	{ name: 'Sessions', href: '/sessions', icon: 'sessions' },
	{ name: 'Activity', href: '/activity', icon: 'activity' },
	{ name: 'Processes', href: '/processes', icon: 'processes' },
	{ name: 'Offices', href: '/offices', icon: 'offices' },
	{ name: 'Settings', href: '/settings', icon: 'settings' }
];

/** The nav item whose section contains `pathname`. */
export const navFor = (pathname: string): NavItem | undefined =>
	NAV.find((item) => pathname === item.href || pathname.startsWith(item.href + '/'));
