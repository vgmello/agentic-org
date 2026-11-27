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

function isUnsafeSegment(rawSeg: string, decoded: string): boolean {
	return (
		decoded === '.' ||
		decoded === '..' ||
		decoded.includes('/') ||
		decoded.includes('\\') ||
		ROUTE_TEMPLATE_RE.test(rawSeg) ||
		ROUTE_TEMPLATE_RE.test(decoded) ||
		ROUTE_GROUP_RE.test(rawSeg) ||
		ROUTE_GROUP_RE.test(decoded)
	);
}

function parseSafeSegments(pathname: string): string[] | undefined {
	const segments: string[] = [];
	for (const rawSeg of pathname.split('/').filter(Boolean)) {
		const decoded = decodeSegmentFully(rawSeg);
		if (decoded === undefined || isUnsafeSegment(rawSeg, decoded)) return undefined;
		segments.push(decoded);
	}
	return segments;
}

function segmentsAreSafe(pathname: string): boolean {
	return parseSafeSegments(pathname) !== undefined;
}

export interface SafeInternalPathOptions {
	allowHash?: boolean;
	disallowApi?: boolean;
}

function isValidRawPrefix(trimmed: string): boolean {
	if (trimmed.length === 0 || trimmed.length > 2048) return false;
	if (!trimmed.startsWith('/') || trimmed.startsWith('//') || trimmed.includes('\\')) return false;
	return !hasControlOrSpace(trimmed);
}

function isValidRawPath(trimmed: string): boolean {
	return isValidRawPrefix(trimmed) && segmentsAreSafe(trimmed.split(/[?#]/, 1)[0]);
}

function rawSegmentsAllowLiteralDot(pathname: string): boolean {
	for (const rawSeg of pathname.split('/').filter(Boolean)) {
		if (rawSeg === '.' || rawSeg === '..') continue;
		const decoded = decodeSegmentFully(rawSeg);
		if (decoded === undefined || isUnsafeSegment(rawSeg, decoded)) return false;
	}
	return true;
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

	const pathname = parsed.pathname;
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
 * Parses and normalizes an untrusted app-relative path (`/…`), collapsing duplicate slashes and
 * rejecting external origins, protocol-relative prefixes, backslashes, control characters, encoded
 * traversal (`%2e%2e`), route templates (`[id]`), and route groups (`(shell)`).
 */
export function parseNormalizedInternalUrl(
	raw: unknown
): { pathname: string; segments: string[]; search: string; hash: string } | undefined {
	if (typeof raw !== 'string') return undefined;
	const trimmed = raw.trim();
	if (!isValidRawPrefix(trimmed)) return undefined;
	if (!rawSegmentsAllowLiteralDot(trimmed.split(/[?#]/, 1)[0])) return undefined;
	const parsed = parseAppRelativeUrl(trimmed);
	if (!parsed) return undefined;
	const pathname = parsed.pathname.replace(/\/+/g, '/');
	const segments = parseSafeSegments(pathname);
	if (!segments) return undefined;
	return { pathname, segments, search: parsed.search, hash: parsed.hash };
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
