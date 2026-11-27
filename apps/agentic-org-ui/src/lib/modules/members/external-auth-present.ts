// Pure presentation, Crockford code validation, and expiry helpers for BYOA external sign-in (`TASK-249`).
import { tone } from '$lib/components/pills';
import {
	CROCKFORD_ALPHABET,
	EXPIRING_SOON_MS,
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
	type WorkspaceFamiliarity
} from '$lib/platform/workspace/external-auth';
import type { ExternalAuthError } from './external-auth-types';

export {
	CROCKFORD_ALPHABET,
	EXPIRING_SOON_MS,
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

const FAMILIARITY_TONE: Record<FamiliaritySeverity, string> = {
	warning: tone.owner,
	caution: tone.bad,
	familiar: tone.ok,
	unknown: tone.neutral
};

/** Exact familiarity badges and semantic tones across all 7 themes (`doc-102` §4). */
export function familiarityMeta(familiarity: WorkspaceFamiliarity): FamiliarityPresentation {
	const info = familiarityInfo(familiarity);
	return { ...info, toneClass: FAMILIARITY_TONE[info.severity] };
}

/**
 * Live input formatting assist for 8-character Crockford codes (`XXXX-YYYY`).
 * Preserves typed characters so invalid letters (e.g. `U`) remain visible for explicit validation feedback.
 */
export function formatApprovalCodeAssist(raw: string): string {
	const stripped = raw
		.replace(/[\s-]+/g, '')
		.toUpperCase()
		.slice(0, 8);
	return stripped.length > 4 ? `${stripped.slice(0, 4)}-${stripped.slice(4)}` : stripped;
}

function mapCrockfordChar(ch: string): string | undefined {
	const upper = ch.toUpperCase();
	const mapped = upper === 'O' ? '0' : upper === 'I' || upper === 'L' ? '1' : upper;
	return CROCKFORD_SET.has(mapped) ? mapped : undefined;
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

/** True when a complete normalized code's lookup half disagrees with the selected request's `XXXX-••••` prefix. */
export function codePrefixMismatch(rawCode: string, codePrefix: string | undefined): boolean {
	if (!codePrefix) return false;
	const expected = codePrefix.split('-')[0]?.trim().toUpperCase();
	if (!expected || expected.length !== 4 || !/^[0-9A-HJ-NP-TV-Z]{4}$/.test(expected)) return false;
	const validated = validateApprovalCode(rawCode);
	if (!validated.ok) return false;
	return validated.code.slice(0, 4) !== expected;
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
