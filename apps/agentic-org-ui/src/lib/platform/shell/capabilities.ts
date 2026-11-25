import { unavailableFeature } from '../feature-factory';
import { UNAVAILABLE_MESSAGE } from '../unavailable-api';

// Enable areas only as real gateway adapters land, including any future product routes.
const unavailable = unavailableFeature(UNAVAILABLE_MESSAGE);
const available = Object.freeze({ available: true as const });

// A3: session pages; A4.5: Create organization; A6: /set-password + /settings/access;
// TASK-76/62: /security + /cli-approve; U1 (TASK-40): Offices, Teams, Members, Org Chart, Projects;
// B7 (TASK-46): Home, Inbox, Chat; FC-108 (TASK-41): Roles directory;
// B4/B5 (TASK-49): Work, Work item detail, Project work.
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
	'/work',
	'/chat',
	'/settings/offices',
	'/offices',
	'/settings/roles',
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

const ROLE_ROUTE =
	/^\/settings\/roles\/[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const TEAM_ROUTE = /^\/teams\/([^/]+)(?:\/(?:members|projects))?$/;
const MEMBER_ROUTE = /^\/members\/([^/]+)(?:\/role)?$/;
const PROJECT_ROUTE = /^\/projects\/([^/]+)(?:\/(?:work|team|settings))?$/;
const WORK_ITEM_ROUTE = /^\/work\/[^/]+$/;

function normalizePathname(pathname: string): string {
	return pathname.length > 1 && pathname.endsWith('/') ? pathname.slice(0, -1) : pathname;
}

function isLiveRoute(pathname: string): boolean {
	const normalized = normalizePathname(pathname);
	if (EXACT_ENABLED_PAGES.has(normalized)) return true;
	if (ROLE_ROUTE.test(normalized)) return true;
	const team = TEAM_ROUTE.exec(normalized);
	if (team && !RESERVED_TEAM_SEGMENTS.has(team[1])) return true;
	if (MEMBER_ROUTE.test(normalized)) return true;
	if (PROJECT_ROUTE.test(normalized)) return true;
	if (WORK_ITEM_ROUTE.test(normalized)) return true;
	return false;
}

export function areaCapability(pathname: string) {
	return isLiveRoute(pathname) ? available : unavailable();
}
