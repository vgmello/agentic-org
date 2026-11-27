// Pure presentation, Crockford code validation, and expiry helpers for BYOA external sign-in (`TASK-249`).
import { tone } from '$lib/components/pills';
import type { ExternalAuthError, WorkspaceFamiliarity } from './external-auth-types';

/** 24-hour date and time for BYOA leases, approvals, and history entries (e.g. "18 Nov, 09:12"). */
export function formatExternalAuthTime(iso: string): string {
	return new Date(iso).toLocaleString(undefined, {
		day: 'numeric',
		month: 'short',
		hour: '2-digit',
		minute: '2-digit',
		hourCycle: 'h23'
	});
}

export const CROCKFORD_ALPHABET = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
const CROCKFORD_SET = new Set(CROCKFORD_ALPHABET.split(''));

export const MIN_LEASE_TTL_SECONDS = 300;
export const MAX_LEASE_TTL_SECONDS = 604_800;
export const DEFAULT_LEASE_TTL_SECONDS = 86_400;
export const RENEW_7D_TTL_SECONDS = 604_800;
export const EXPIRING_SOON_MS = 24 * 60 * 60 * 1000;

export const EXTERNAL_AUTH_TTL_PRESETS = [
	{ id: '8h', label: '8 hours', shortLabel: '8h', seconds: 28_800 },
	{ id: '24h', label: '24 hours', shortLabel: '24h', seconds: 86_400 },
	{ id: '7d', label: '7 days', shortLabel: '7d', seconds: 604_800 }
] as const;

export type ExternalAuthTtlPresetId = (typeof EXTERNAL_AUTH_TTL_PRESETS)[number]['id'];

export interface FamiliarityPresentation {
	label: string;
	toneClass: string;
	severity: 'warning' | 'caution' | 'familiar' | 'unknown';
	description: string;
}

/** Exact familiarity badges and semantic tones across all 7 themes (`doc-102` §4). */
export function familiarityMeta(familiarity: WorkspaceFamiliarity): FamiliarityPresentation {
	switch (familiarity) {
		case 'first_time':
			return {
				label: 'First-time workspace',
				toneClass: tone.owner,
				severity: 'warning',
				description:
					'No prior approved external session exists for this workspace. Verify host, path, and terminal code.'
			};
		case 'returning_new_key':
			return {
				label: 'Returning workspace (new key)',
				toneClass: tone.bad,
				severity: 'caution',
				description:
					'This workspace was approved before, but it is presenting a different Ed25519 key fingerprint.'
			};
		case 'returning_known_key':
			return {
				label: 'Returning workspace (known key)',
				toneClass: tone.ok,
				severity: 'familiar',
				description:
					'This workspace was previously approved with the exact same Ed25519 key fingerprint.'
			};
		default:
			return {
				label: 'Workspace familiarity unknown',
				toneClass: tone.neutral,
				severity: 'unknown',
				description: 'Workspace history could not be classified. Verify the terminal code.'
			};
	}
}

/** Strips any leading `@` so callers never render `@@handle`. */
export function normalizeHandle(raw: string | undefined): string | undefined {
	const cleaned = raw?.trim().replace(/^@+/, '');
	return cleaned ? cleaned : undefined;
}

export function formatHandle(raw: string | undefined): string | undefined {
	const clean = normalizeHandle(raw);
	return clean ? `@${clean}` : undefined;
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

export function isExpired(expiresAt: string, nowMs = Date.now()): boolean {
	const ms = Date.parse(expiresAt);
	return Number.isFinite(ms) && ms <= nowMs;
}

/** True when an active lease expires in strictly less than 24 hours and has not yet expired. */
export function isExpiringSoon(expiresAt: string, nowMs = Date.now()): boolean {
	const ms = Date.parse(expiresAt);
	if (!Number.isFinite(ms)) return false;
	const remaining = ms - nowMs;
	return remaining > 0 && remaining < EXPIRING_SOON_MS;
}

export function formatTimeRemaining(expiresAt: string, nowMs = Date.now()): string {
	const ms = Date.parse(expiresAt);
	if (!Number.isFinite(ms)) return 'Expiry unknown';
	const diff = ms - nowMs;
	if (diff <= 0) return 'Expired';
	const totalMin = Math.floor(diff / 60_000);
	if (totalMin < 1) return '< 1 min left';
	if (totalMin < 60) return `${totalMin} min left`;
	const totalHours = Math.floor(totalMin / 60);
	const remMin = totalMin % 60;
	if (totalHours < 24)
		return remMin ? `${totalHours} h ${remMin} min left` : `${totalHours} h left`;
	const days = Math.floor(totalHours / 24);
	const remHours = totalHours % 24;
	return remHours ? `${days} d ${remHours} h left` : `${days} d left`;
}

export function formatLastApproved(by?: string, at?: string): string | undefined {
	const actor = by?.trim();
	const formattedAt =
		at && Number.isFinite(Date.parse(at)) ? formatExternalAuthTime(at) : undefined;
	if (actor && formattedAt) return `Last approved by ${actor} · ${formattedAt}`;
	if (actor) return `Last approved by ${actor}`;
	if (formattedAt) return `Last approved ${formattedAt}`;
	return undefined;
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
