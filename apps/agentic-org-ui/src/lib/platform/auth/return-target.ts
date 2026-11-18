/** Pages that never serve as a return target: the credential pages themselves. */
const CREDENTIAL_PAGES = ['/sign-in', '/register'];

/**
 * SIGN-8: a return target is a page of this HQ only. Anything else (another origin, a
 * protocol-relative or backslash path, the API, a credential page) is ignored. The fragment is
 * dropped so no link secret is carried along.
 */
export function safeReturnTarget(raw: string | null | undefined, origin: string): string | undefined {
	if (typeof raw !== 'string' || raw.length === 0 || raw.length > 2048) return undefined;
	if (!raw.startsWith('/') || raw.startsWith('//') || raw.includes('\\') || /[\x00-\x20\x7f]/.test(raw)) return undefined;
	let url: URL;
	try {
		url = new URL(raw, origin);
	} catch {
		return undefined;
	}
	if (url.origin !== origin || url.username || url.password) return undefined;
	const path = url.pathname;
	if (path === '/api' || path.startsWith('/api/')) return undefined;
	if (CREDENTIAL_PAGES.some((page) => path === page || path.startsWith(page + '/'))) return undefined;
	return path + url.search;
}

/** The sign-in page URL that returns to `target` afterwards, when it is a valid target. */
export function signInHref(target: string | undefined, origin: string, note?: 'signed-out' | 'ended'): string {
	const params = new URLSearchParams();
	const safe = safeReturnTarget(target, origin);
	if (safe && safe !== '/') params.set('next', safe);
	if (note) params.set('note', note);
	const query = params.toString();
	return query ? `/sign-in?${query}` : '/sign-in';
}
