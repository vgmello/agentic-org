import type { IconName } from '$lib/components/icons';

export interface NavItem {
	name: string;
	href: string;
	icon: IconName;
}

/** The main navigation, in order. Sections without a page of their own show a placeholder. */
export const NAV: NavItem[] = [
	{ name: 'Agents', href: '/agents', icon: 'agents' },
	{ name: 'Org chart', href: '/org-chart', icon: 'orgchart' },
	{ name: 'Projects', href: '/projects', icon: 'projects' },
	{ name: 'Work', href: '/work', icon: 'tasks' },
	{ name: 'Sessions', href: '/sessions', icon: 'sessions' },
	{ name: 'Activity', href: '/activity', icon: 'activity' },
	{ name: 'Offices', href: '/offices', icon: 'offices' },
	{ name: 'Settings', href: '/settings', icon: 'settings' }
];

/** The nav item whose section contains `pathname`. */
export const navFor = (pathname: string): NavItem | undefined =>
	NAV.find((item) => pathname === item.href || pathname.startsWith(item.href + '/'));

/** Pages that are not in the sidebar but have a title in the header. */
const EXTRA_PAGES = [{ name: 'Report a bug', href: '/report-bug' }];

/** The header title for `pathname`: its section name, or the name of a page outside the sidebar. */
export const titleFor = (pathname: string): string | undefined =>
	navFor(pathname)?.name ?? EXTRA_PAGES.find((p) => pathname === p.href)?.name;
