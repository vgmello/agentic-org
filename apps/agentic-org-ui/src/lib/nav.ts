import type { IconName } from '$lib/components/icons';
import type { NavPlace, ShellNavigation } from '$lib/modules/shell/types';

export interface NavItem {
	name: string;
	href: string;
	icon: IconName;
	/** Short visible text and its spoken meaning. */
	badge?: { text: string; label: string };
	/** A destination shown only once its source is served (app-shell-ux §0). */
	gate?: 'inbox';
	/** Places listed under the item in the full navigation: up to five and View all. */
	sublist?: 'projects' | 'teams';
	/** Other paths that belong to this item (a member profile marks Teams current). */
	also?: string[];
}

/**
 * The main navigation, in order (app-shell-ux §0). The full sidebar, the rail and the drawer show the same
 * destinations. Following is left out: its work service is not in v1.
 */
export const NAV: NavItem[] = [
	{ name: 'Home', href: '/home', icon: 'home' },
	{ name: 'Inbox', href: '/inbox', icon: 'inbox', gate: 'inbox' },
	{ name: 'Projects', href: '/projects', icon: 'projects', sublist: 'projects' },
	{ name: 'Work', href: '/work', icon: 'tasks' },
	{ name: 'Chat', href: '/chat', icon: 'chat' },
	{ name: 'Activity', href: '/activity', icon: 'activity' },
	{ name: 'Teams', href: '/teams', icon: 'teams', sublist: 'teams', also: ['/members'] },
	{ name: 'Organization settings', href: '/settings', icon: 'settings' }
];

/** Shell pages that are not navigation destinations; their area name heads the page. */
const OTHER_AREAS: { href: string; name: string }[] = [
	{ href: '/search', name: 'Search' },
	{ href: '/preferences', name: 'Your preferences' },
	{ href: '/insights', name: 'Insights' }
];

const within = (pathname: string, href: string) =>
	pathname === href || pathname.startsWith(href + '/');

/** The nav item whose section contains `pathname`. */
export const navFor = (pathname: string): NavItem | undefined =>
	NAV.find(
		(item) => within(pathname, item.href) || item.also?.some((href) => within(pathname, href))
	);

/** The area a shell page belongs to: the shell's `h1` and the last part of the page title before "Agentic Org". */
export const areaFor = (pathname: string): string | undefined =>
	navFor(pathname)?.name ?? OTHER_AREAS.find((area) => within(pathname, area.href))?.name;

/** "3 open" for the Inbox badge; no badge for zero or an unknown count. */
const inboxBadge = (open: number) => ({
	text: open > 99 ? '99+' : String(open),
	label: `${open} open`
});

/** The destinations to show now: gated ones only once served, with the Inbox count as an amber badge when nonzero. */
export function navItems(navigation: ShellNavigation | undefined): NavItem[] {
	const open = navigation?.inboxOpen;
	return NAV.flatMap((item) => {
		if (item.gate !== 'inbox') return [item];
		if (open === null || open === undefined) return [];
		return [open > 0 ? { ...item, badge: inboxBadge(open) } : item];
	});
}

export const SUBLIST_LIMIT = 5;

/** The first five places by name, A–Z (no last-activity time exists yet, so not Recent), as links. */
export function sublistLinks(
	places: NavPlace[],
	base: '/projects' | '/teams'
): { name: string; href: string }[] {
	return [...places]
		.sort(
			(a, b) =>
				a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) || a.id.localeCompare(b.id)
		)
		.slice(0, SUBLIST_LIMIT)
		.map((place) => ({ name: place.name, href: `${base}/${encodeURIComponent(place.id)}` }));
}

export { pageTitle } from './page-title';
