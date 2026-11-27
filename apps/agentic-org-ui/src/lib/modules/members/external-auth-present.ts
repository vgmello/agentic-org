// Pure presentation, Crockford code validation, prefix matching, and expiry helpers for BYOA external sign-in (`TASK-249`).
import { tone } from '$lib/components/pills';
import {
	CROCKFORD_ALPHABET,
	EXPIRING_SOON_MS,
	FAMILIARITY_TONE_KEY,
	familiarityInfo,
	formatExternalAuthTime,
	formatHandle,
	formatLastApproved,
	formatTimeRemaining,
	isExpired,
	isExpiringSoon,
	normalizeHandle,
	type FamiliarityInfo,
	type FamiliaritySeverity,
	type PendingExternalAuthView,
	type WorkspaceFamiliarity
} from '$lib/platform/workspace/external-auth';
import type { ExternalAuthError } from './external-auth-types';

export {
	CROCKFORD_ALPHABET,
	EXPIRING_SOON_MS,
	FAMILIARITY_TONE_KEY,
	familiarityInfo,
	formatExternalAuthTime,
	formatHandle,
	formatLastApproved,
	formatTimeRemaining,
	isExpired,
	isExpiringSoon,
	normalizeHandle,
	type FamiliarityInfo,
	type FamiliaritySeverity
};

const CROCKFORD_SET = new Set(CROCKFORD_ALPHABET.split(''));
const CLI_CODE_TOKEN_RE =
	/(?:^|[\s:'"(])([0-9A-Za-z]{4}[\s-]+[0-9A-Za-z]{4}|[0-9A-Za-z]{8})(?=$|[\s.,;:'")\]])/g;

export const MIN_LEASE_TTL_SECONDS = 300;
export const MAX_LEASE_TTL_SECONDS = 604_800;
export const DEFAULT_LEASE_TTL_SECONDS = 86_400;
export const RENEW_7D_TTL_SECONDS = 604_800;

export const EXTERNAL_AUTH_TTL_PRESETS = [
	{ id: '8h', label: '8 hours', shortLabel: '8h', seconds: 28_800 },
	{ id: '24h', label: '24 hours', shortLabel: '24h', seconds: 86_400 },
	{ id: '7d', label: '7 days', shortLabel: '7d', seconds: 604_800 }
] as const;

export type ExternalAuthTtlPresetId = (typeof EXTERNAL_AUTH_TTL_PRESETS)[number]['id'];

export interface FamiliarityPresentation extends FamiliarityInfo {
	toneClass: string;
}

/** Exact familiarity badges and semantic tones across all 7 themes (`doc-102` §4). */
export function familiarityMeta(familiarity: WorkspaceFamiliarity): FamiliarityPresentation {
	const info = familiarityInfo(familiarity);
	return { ...info, toneClass: tone[FAMILIARITY_TONE_KEY[info.severity]] };
}

function mapCrockfordChar(ch: string): string | undefined {
	const upper = ch.toUpperCase();
	const mapped = upper === 'O' ? '0' : upper === 'I' || upper === 'L' ? '1' : upper;
	return CROCKFORD_SET.has(mapped) ? mapped : undefined;
}

function extractLastCliCode(raw: string): string | undefined {
	let last: string | undefined;
	for (const match of raw.matchAll(CLI_CODE_TOKEN_RE)) {
		const token = match[1]?.replace(/[\s-]+/g, '').toUpperCase();
		if (
			token &&
			token.length === 8 &&
			[...token].every((ch) => mapCrockfordChar(ch) !== undefined)
		) {
			last = token;
		}
	}
	return last;
}

/**
 * Live input formatting assist for 8-character Crockford codes (`XXXX-YYYY`).
 * Extracts the last valid `XXXX-YYYY` token when pasting a full CLI command (e.g.
 * `agentic-org agent approve K7P4-9M2X`), while preserving short typed characters so invalid
 * letters (e.g. `U`) remain visible for explicit validation feedback.
 */
export function formatApprovalCodeAssist(raw: string): string {
	const stripped = raw.replace(/[\s-]+/g, '').toUpperCase();
	const candidate =
		stripped.length > 8 ? (extractLastCliCode(raw) ?? stripped.slice(0, 8)) : stripped;
	return candidate.length > 4 ? `${candidate.slice(0, 4)}-${candidate.slice(4)}` : candidate;
}

/**
 * Validates and normalizes an approval code identically to `apps/agentic-org-web`:
 * strips spaces/hyphens, maps `O`->`0` and `I`/`L`->`1`, checks 8 Crockford base32 digits -> `LLLL-SSSS`.
 */
export function validateApprovalCode(
	raw: string
): { ok: true; code: string } | { ok: false; message: string } {
	const stripped = raw.replace(/[\s-]+/g, '');
	if (!stripped) {
		return { ok: false, message: 'Enter the 8-character approval code shown in the terminal.' };
	}
	const digits: string[] = [];
	for (const ch of stripped) {
		const mapped = mapCrockfordChar(ch);
		if (!mapped) {
			return {
				ok: false,
				message: 'Approval codes use 0–9 and A–Z (excluding U and symbols).'
			};
		}
		digits.push(mapped);
	}
	if (digits.length !== 8) {
		return {
			ok: false,
			message: 'Enter all 8 characters of the approval code (XXXX-YYYY).'
		};
	}
	return { ok: true, code: `${digits.slice(0, 4).join('')}-${digits.slice(4).join('')}` };
}

function prefixOfCodePrefix(codePrefix: string | undefined): string | undefined {
	if (!codePrefix) return undefined;
	const expected = codePrefix.split('-')[0]?.trim().toUpperCase();
	return expected && expected.length === 4 && /^[0-9A-HJ-NP-TV-Z]{4}$/.test(expected)
		? expected
		: undefined;
}

/** Extracts the normalized 4-character Crockford lookup prefix once 4 valid digits are entered. */
export function extractCodeLookupPrefix(rawCode: string): string | undefined {
	const stripped = rawCode.replace(/[\s-]+/g, '');
	if (stripped.length < 4) return undefined;
	let out = '';
	for (const ch of stripped.slice(0, 4)) {
		const mapped = mapCrockfordChar(ch);
		if (!mapped) return undefined;
		out += mapped;
	}
	return out;
}

/** True when a complete normalized code's lookup half disagrees with the selected request's `XXXX-••••` prefix. */
export function codePrefixMismatch(rawCode: string, codePrefix: string | undefined): boolean {
	const expected = prefixOfCodePrefix(codePrefix);
	if (!expected) return false;
	const validated = validateApprovalCode(rawCode);
	if (!validated.ok) return false;
	return validated.code.slice(0, 4) !== expected;
}

export type PendingApprovalResolution =
	| { status: 'matched'; display: PendingExternalAuthView; matched: PendingExternalAuthView }
	| { status: 'awaiting_prefix'; display?: PendingExternalAuthView; matched: undefined }
	| { status: 'mismatch'; display?: PendingExternalAuthView; matched: undefined }
	| { status: 'no_pending'; display: undefined; matched: undefined };

/**
 * Resolves which pending BYOA request for the viewed agent matches the entered code prefix.
 * Blocks cross-agent or unmatched-prefix approval so operators always review the exact workspace
 * familiarity, host, path, fingerprint, and takeover state for the code they submit.
 */
export function resolvePendingApprovalTarget(options: {
	rawCode: string;
	explicitPending?: PendingExternalAuthView;
	candidates?: readonly PendingExternalAuthView[];
	expectedEmployeeId?: string;
}): PendingApprovalResolution {
	const { rawCode, explicitPending, candidates, expectedEmployeeId } = options;
	const typedPrefix = extractCodeLookupPrefix(rawCode);

	if (explicitPending) {
		if (expectedEmployeeId && explicitPending.employee_id !== expectedEmployeeId) {
			return { status: 'mismatch', display: undefined, matched: undefined };
		}
		const expectedPrefix = prefixOfCodePrefix(explicitPending.code_prefix);
		if (!typedPrefix) {
			return { status: 'awaiting_prefix', display: explicitPending, matched: undefined };
		}
		if (expectedPrefix && typedPrefix !== expectedPrefix) {
			return { status: 'mismatch', display: explicitPending, matched: undefined };
		}
		return { status: 'matched', display: explicitPending, matched: explicitPending };
	}

	const scoped = (candidates ?? []).filter(
		(req) => !expectedEmployeeId || req.employee_id === expectedEmployeeId
	);
	if (scoped.length === 0) {
		return { status: 'no_pending', display: undefined, matched: undefined };
	}
	if (!typedPrefix) {
		return { status: 'awaiting_prefix', display: undefined, matched: undefined };
	}
	const matched = scoped.find((req) => prefixOfCodePrefix(req.code_prefix) === typedPrefix);
	return matched
		? { status: 'matched', display: matched, matched }
		: { status: 'mismatch', display: undefined, matched: undefined };
}

export function historyEventLabel(event: string): string {
	switch (event) {
		case 'approved':
			return 'Approved';
		case 'renewed':
			return 'Renewed';
		case 'superseded':
			return 'Superseded';
		case 'revoked':
			return 'Revoked';
		case 'expired':
			return 'Expired';
		default:
			return event ? `${event[0].toUpperCase()}${event.slice(1)}` : 'Event';
	}
}

export function historyEventTone(event: string): string {
	switch (event) {
		case 'approved':
			return tone.ok;
		case 'renewed':
			return tone.info;
		case 'superseded':
			return tone.owner;
		case 'revoked':
		case 'expired':
			return tone.bad;
		default:
			return tone.neutral;
	}
}

export function formatRetrySeconds(retryAfterMs: number | undefined): number | undefined {
	if (typeof retryAfterMs !== 'number' || !Number.isFinite(retryAfterMs) || retryAfterMs <= 0) {
		return undefined;
	}
	return Math.max(1, Math.ceil(retryAfterMs / 1000));
}

export function formatExternalAuthErrorMessage(err: ExternalAuthError): string {
	if (err.code === 'rate_limited') {
		const secs = formatRetrySeconds(err.retry_after_ms);
		return secs !== undefined
			? `Too many attempts. Try again in ${secs} s.`
			: err.message || 'Too many attempts. Try again later.';
	}
	if (err.code === 'expired') {
		return err.message || 'This approval code has expired. Run agent-os login again.';
	}
	if (err.code === 'not_authorized' || err.code === 'owner_protected') {
		return (
			err.message ||
			'Only authorized human operators (owner or reporting/team lead) can manage external sign-ins for this agent.'
		);
	}
	if (err.code === 'conflict') {
		return (
			err.message ||
			'A managed turn is currently running for this agent. Confirm takeover to park it.'
		);
	}
	return err.message || 'External sign-in request failed.';
}
