import { safeInternalPath } from '$lib/platform/http/safe-url';

/** Pages that never serve as a return target: the credential pages themselves. */
const CREDENTIAL_PAGES = ['/sign-in', '/register'];

/**
 * SIGN-8: a return target is a page of this HQ only. Anything else (another origin, a
 * protocol-relative or backslash path, a dot segment, the API, a credential page) is ignored. The
 * fragment is dropped so no link secret is carried along.
 */
export function safeReturnTarget(
	raw: string | null | undefined,
	origin: string
): `/${string}` | undefined {
	const safe = safeInternalPath(raw, { allowHash: false, disallowApi: true });
	if (!safe) return undefined;
	let url: URL;
	try {
		url = new URL(safe, origin);
	} catch {
		return undefined;
	}
	if (url.origin !== origin || url.username || url.password) return undefined;
	const path = url.pathname;
	if (CREDENTIAL_PAGES.some((page) => path === page || path.startsWith(page + '/'))) {
		return undefined;
	}
	return `${path}${url.search}` as `/${string}`;
}

/** The sign-in page URL that returns to `target` afterwards, when it is a valid target. */
export function signInHref(
	target: string | undefined,
	origin: string,
	note?: 'signed-out' | 'ended'
): '/sign-in' | `/sign-in?${string}` {
	const params = new URLSearchParams();
	const safe = safeReturnTarget(target, origin);
	if (safe && safe !== '/') params.set('next', safe);
	if (note) params.set('note', note);
	const query = params.toString();
	return query ? `/sign-in?${query}` : '/sign-in';
}
