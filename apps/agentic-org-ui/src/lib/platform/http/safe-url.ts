/**
 * Canonical safe-URL choke-point for untrusted internal navigation paths and external links.
 * Every server-supplied or query-derived navigation target must pass through these helpers.
 */

const ROUTE_TEMPLATE_RE = /[[\]]/;
const ROUTE_GROUP_RE = /^\(.+\)$/;
const PERCENT_ENCODED_RE = /%[0-9a-fA-F]{2}/;

function hasControlOrSpace(text: string): boolean {
	for (let i = 0; i < text.length; i++) {
		const code = text.charCodeAt(i);
		if (code <= 0x20 || code === 0x7f) return true;
	}
	return false;
}

function decodeSegmentFully(raw: string): string | undefined {
	let current = raw;
	for (let i = 0; i < 4; i++) {
		try {
			const next = decodeURIComponent(current);
			if (next === current) return current;
			current = next;
		} catch {
			return undefined;
		}
	}
	return PERCENT_ENCODED_RE.test(current) ? undefined : current;
}

function segmentsAreSafe(pathname: string): boolean {
	for (const rawSeg of pathname.split('/').filter(Boolean)) {
		const decoded = decodeSegmentFully(rawSeg);
		if (decoded === undefined) return false;
		if (
			decoded === '.' ||
			decoded === '..' ||
			decoded.includes('/') ||
			decoded.includes('\\') ||
			ROUTE_TEMPLATE_RE.test(rawSeg) ||
			ROUTE_TEMPLATE_RE.test(decoded) ||
			ROUTE_GROUP_RE.test(rawSeg) ||
			ROUTE_GROUP_RE.test(decoded)
		) {
			return false;
		}
	}
	return true;
}

export interface SafeInternalPathOptions {
	allowHash?: boolean;
	disallowApi?: boolean;
}

function isValidRawPath(trimmed: string): boolean {
	if (trimmed.length === 0 || trimmed.length > 2048) return false;
	if (!trimmed.startsWith('/') || trimmed.startsWith('//') || trimmed.includes('\\')) return false;
	if (hasControlOrSpace(trimmed)) return false;
	return segmentsAreSafe(trimmed.split(/[?#]/, 1)[0]);
}

function parseAppRelativeUrl(trimmed: string): URL | undefined {
	try {
		const parsed = new URL(trimmed, 'https://app.invalid');
		if (parsed.origin !== 'https://app.invalid' || parsed.username || parsed.password) {
			return undefined;
		}
		return parsed;
	} catch {
		return undefined;
	}
}

/**
 * Validates that `raw` is a same-origin app-relative path (`/…`) with no protocol-relative prefix,
 * backslash, control characters, dot traversal segments, route templates, or external origin.
 */
export function safeInternalPath(
	raw: unknown,
	options: SafeInternalPathOptions = {}
): `/${string}` | undefined {
	if (typeof raw !== 'string') return undefined;
	const trimmed = raw.trim();
	if (!isValidRawPath(trimmed)) return undefined;

	const parsed = parseAppRelativeUrl(trimmed);
	if (!parsed) return undefined;

	const pathname = parsed.pathname.replace(/\/+/g, '/');
	if (!pathname.startsWith('/') || pathname.startsWith('//') || pathname.includes('\\')) {
		return undefined;
	}
	if (!segmentsAreSafe(pathname)) return undefined;
	if (options.disallowApi && (pathname === '/api' || pathname.startsWith('/api/'))) {
		return undefined;
	}
	const hash = options.allowHash ? parsed.hash : '';
	return `${pathname}${parsed.search}${hash}` as `/${string}`;
}

/**
 * Splits a validated relative route href into its pathname and `?query`/`#hash` suffix so only the
 * pathname is passed to SvelteKit's `resolve()`.
 */
export function splitSafeHref(raw: string): { pathname: string; suffix: string } {
	const qIdx = raw.indexOf('?');
	const hIdx = raw.indexOf('#');
	let cut = -1;
	if (qIdx >= 0 && hIdx >= 0) cut = Math.min(qIdx, hIdx);
	else if (qIdx >= 0) cut = qIdx;
	else if (hIdx >= 0) cut = hIdx;
	if (cut < 0) return { pathname: raw || '/', suffix: '' };
	return { pathname: raw.slice(0, cut) || '/', suffix: raw.slice(cut) };
}
