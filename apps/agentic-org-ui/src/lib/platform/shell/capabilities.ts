import { unavailableFeature } from '../feature-factory';
import { UNAVAILABLE_MESSAGE } from '../unavailable-api';

// Enable areas only as real gateway adapters land, including any future product routes.
const unavailable = unavailableFeature(UNAVAILABLE_MESSAGE);
const available = Object.freeze({ available: true as const });
// A3: the session pages run against the real web gateway (register, sign in, SIGN-9, sign out).
// A4.5: Create organization runs against the real organization routes (FC-018).
// TASK-76 / TASK-62 / FC-173: instance Security & Access settings and CLI approval run against the real gateway.
const SESSION_PAGES: ReadonlySet<string> = new Set([
	'/',
	'/sign-in',
	'/register',
	'/welcome',
	'/create',
	'/security',
	'/cli-approve'
]);
export function areaCapability(pathname: string) {
	return SESSION_PAGES.has(pathname) ? available : unavailable();
}
