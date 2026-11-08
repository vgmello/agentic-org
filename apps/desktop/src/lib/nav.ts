import type { IconName } from '$lib/components/icons';

export interface NavItem {
	name: string;
	href: string;
	icon: IconName;
}

/** The main navigation, in order. Only Agents has content so far; the rest show a placeholder. */
export const NAV: NavItem[] = [
	{ name: 'Agents', href: '/agents', icon: 'agents' },
	{ name: 'Org chart', href: '/org-chart', icon: 'orgchart' },
	{ name: 'Projects', href: '/projects', icon: 'projects' },
	{ name: 'Tasks', href: '/tasks', icon: 'tasks' },
	{ name: 'Sessions', href: '/sessions', icon: 'sessions' },
	{ name: 'Activity', href: '/activity', icon: 'activity' },
	{ name: 'Settings', href: '/settings', icon: 'settings' }
];

/** The nav item whose section contains `pathname`. */
export const navFor = (pathname: string): NavItem | undefined =>
	NAV.find((item) => pathname === item.href || pathname.startsWith(item.href + '/'));
