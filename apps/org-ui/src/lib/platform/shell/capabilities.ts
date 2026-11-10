import { NAV } from '$lib/nav';
import { UNAVAILABLE_MESSAGE } from '../unavailable-api';

// Enable areas here only as real gateway adapters land. Ordinary dev and release share this default.
const unavailable = Object.freeze({ available: false as const, reason: 'unavailable' as const, message: UNAVAILABLE_MESSAGE });
const errorPage = Object.freeze({ available: true as const });
export function areaCapability(pathname: string) {
	const known = ['/', '/welcome', '/create'].includes(pathname) || NAV.some((item) => item.href === pathname) || /^\/projects\/[^/]+$/.test(pathname);
	// Preserve framework 404s for unknown paths; they are not unserved product areas.
	return known ? unavailable : errorPage;
}
