import { unavailableFeature } from '../feature-factory';
import { UNAVAILABLE_MESSAGE } from '../unavailable-api';

// Enable areas only as real gateway adapters land, including any future product routes.
const unavailable = unavailableFeature(UNAVAILABLE_MESSAGE);
const available = Object.freeze({ available: true as const });
// A3: the session pages run against the real web gateway (register, sign in, SIGN-9, sign out).
const SESSION_PAGES: ReadonlySet<string> = new Set(['/', '/sign-in', '/register', '/welcome']);
export function areaCapability(pathname: string) {
	return SESSION_PAGES.has(pathname) ? available : unavailable();
}
