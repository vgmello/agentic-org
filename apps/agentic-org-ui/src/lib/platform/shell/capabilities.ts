import { unavailableFeature } from '../feature-factory';
import { UNAVAILABLE_MESSAGE } from '../unavailable-api';

// Enable areas only as real gateway adapters land, including any future product routes.
const unavailable = unavailableFeature(UNAVAILABLE_MESSAGE);
const available = Object.freeze({ available: true as const });

// A3: session pages; A4.5: Create organization; A6: /set-password + /settings/access;
// TASK-76/62: /security + /cli-approve; U1 (TASK-40): Offices, Teams, Members, Org Chart, Projects;
// B7 (TASK-46): Home, Inbox, Chat.
const EXACT_ENABLED_PAGES: ReadonlySet<string> = new Set([
	'/',
	'/sign-in',
	'/register',
	'/set-password',
	'/welcome',
	'/create',
	'/security',
	'/approve',
	'/cli-approve',
	'/home',
	'/inbox',
	'/chat',
	'/settings/offices',
	'/offices',
	'/settings/access',
	'/teams',
	'/teams/members',
	'/agents',
	'/teams/org-chart',
	'/org-chart',
	'/projects'
]);

const RESERVED_TEAM_SEGMENTS: ReadonlySet<string> = new Set([
	'hire',
	'hires',
	'members',
	'org-chart'
]);

const TEAM_ROUTE = /^\/teams\/([^/]+)(?:\/projects)?$/;
const MEMBER_ROUTE = /^\/members\/([^/]+)(?:\/role)?$/;
const PROJECT_ROUTE = /^\/projects\/([^/]+)(?:\/(?:team|settings))?$/;

function isLiveRoute(pathname: string): boolean {
	if (EXACT_ENABLED_PAGES.has(pathname)) return true;
	const team = TEAM_ROUTE.exec(pathname);
	if (team && !RESERVED_TEAM_SEGMENTS.has(team[1])) return true;
	if (MEMBER_ROUTE.test(pathname)) return true;
	if (PROJECT_ROUTE.test(pathname)) return true;
	return false;
}

export function areaCapability(pathname: string) {
	return isLiveRoute(pathname) ? available : unavailable();
}
