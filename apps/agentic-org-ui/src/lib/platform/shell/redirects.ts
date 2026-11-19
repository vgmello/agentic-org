import { redirect } from '@sveltejs/kit';

/**
 * Old addresses that moved (app-shell-ux §0), kept indefinitely. Each keeps its complete query string.
 * `/` → `/home` belongs to the sign-in flow (src/routes/+page.ts), not to this table.
 */
export const MOVED: Readonly<Record<string, string>> = {
	'/agents': '/teams/members',
	'/org-chart': '/teams/org-chart',
	'/offices': '/settings/offices',
	'/processes': '/settings/processes',
	'/sessions': '/activity'
};

/** Where an old address now lives, with its query string; undefined for an address that did not move. */
export function movedTo(url: URL): string | undefined {
	const target = MOVED[url.pathname.replace(/\/$/, '') || '/'];
	return target === undefined ? undefined : target + url.search;
}

/** A route's `load` for an old address: `export const load = redirectMoved;` */
export function redirectMoved({ url }: { url: URL }): never {
	const target = movedTo(url);
	if (target === undefined) throw new Error(`${url.pathname} has no new address`);
	redirect(308, target);
}
