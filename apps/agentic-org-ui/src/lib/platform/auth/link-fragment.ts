/**
 * Takes the one-time link secret from the fragment and removes it from the address bar and
 * history entry before any request is made, so it is never in a request line, Referer or
 * later history entry (web-api-v1 §4 "One-time links").
 */
export function takeLinkSecret(loc: Pick<Location, 'hash' | 'pathname' | 'search'>, hist: Pick<History, 'replaceState' | 'state'>): string | undefined {
	const raw = loc.hash.startsWith('#') ? loc.hash.slice(1) : loc.hash;
	if (raw.length > 0) hist.replaceState(hist.state, '', loc.pathname + loc.search);
	return raw.length > 0 && raw.length <= 1024 && /^[\x21-\x7e]+$/.test(raw) ? raw : undefined;
}
