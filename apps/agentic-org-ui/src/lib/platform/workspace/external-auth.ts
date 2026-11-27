// Shared BYOA external-agent wire types, pending decoder, and pure presentation helpers (`TASK-249`).
// Lives in `platform/workspace` with zero module or component dependencies so `members` and `inbox`
// share one canonical contract without cross-module private imports.
import { asArr, asBool, asNonEmptyStr, asRec, asStr, type Rec } from '$lib/platform/http/decode';

export type WorkspaceFamiliarity =
	'first_time' | 'returning_new_key' | 'returning_known_key' | 'unknown';

export interface PendingExternalAuthView {
	request_id: string;
	employee_id: string;
	handle?: string;
	office_id: string;
	office_name: string;
	hostname?: string;
	workspace_path?: string;
	workspace_label: string;
	pubkey_fingerprint: string;
	managed_turn_active: boolean;
	requested_at: string;
	expires_at: string;
	/** Masked lookup half (`XXXX-••••`); never contains the secret half. */
	code_prefix: string;
	workspace_familiarity: WorkspaceFamiliarity;
	last_approved_by?: string;
	last_approved_at?: string;
}

export interface ExternalSessionView {
	session_id: string;
	employee_id: string;
	handle?: string;
	workspace_label: string;
	pubkey_fingerprint: string;
	approved_by: string;
	approved_at: string;
	expires_at: string;
	revoked_at?: string;
	active: boolean;
	superseded_session_id?: string;
	took_over_managed: boolean;
}

export type FamiliaritySeverity = 'warning' | 'caution' | 'familiar' | 'unknown';

export interface FamiliarityInfo {
	label: string;
	severity: FamiliaritySeverity;
	description: string;
}

export const CROCKFORD_ALPHABET = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
export const EXPIRING_SOON_MS = 24 * 60 * 60 * 1000;

const CROCKFORD_CHARS = new Set(CROCKFORD_ALPHABET.split(''));
const FAMILIARITIES: ReadonlySet<string> = new Set([
	'first_time',
	'returning_new_key',
	'returning_known_key',
	'unknown'
]);

/** Strips any leading `@` so callers never render `@@handle`. */
export function normalizeHandle(raw: string | undefined): string | undefined {
	const cleaned = raw?.trim().replace(/^@+/, '');
	return cleaned ? cleaned : undefined;
}

export function formatHandle(raw: string | undefined): string | undefined {
	const clean = normalizeHandle(raw);
	return clean ? `@${clean}` : undefined;
}

/** Defensively ensures `code_prefix` stays masked (`XXXX-••••`), never exposing a secret half. */
export function maskCodePrefix(raw: string): string {
	const lookup = raw.split('-')[0]?.trim().toUpperCase() ?? '';
	return lookup.length === 4 && [...lookup].every((ch) => CROCKFORD_CHARS.has(ch))
		? `${lookup}-••••`
		: '••••-••••';
}

export function decodeFamiliarity(raw: unknown): WorkspaceFamiliarity {
	const value = asStr(raw);
	return value && FAMILIARITIES.has(value) ? (value as WorkspaceFamiliarity) : 'unknown';
}

function optStrs<K extends string>(r: Rec, keys: readonly K[]): Partial<Record<K, string>> {
	const out: Partial<Record<K, string>> = {};
	for (const k of keys) {
		const v = asNonEmptyStr(r[k]);
		if (v) out[k] = v;
	}
	return out;
}

function reqStrs<K extends string>(r: Rec, keys: readonly K[]): Record<K, string> | undefined {
	const out = {} as Record<K, string>;
	for (const k of keys) {
		const v = asNonEmptyStr(r[k]);
		if (!v) return undefined;
		out[k] = v;
	}
	return out;
}

export function decodePendingExternalAuth(raw: unknown): PendingExternalAuthView | undefined {
	const r = asRec(raw);
	if (!r) return undefined;
	const req = reqStrs(r, [
		'request_id',
		'employee_id',
		'office_id',
		'office_name',
		'workspace_label',
		'pubkey_fingerprint',
		'requested_at',
		'expires_at',
		'code_prefix'
	]);
	if (!req) return undefined;
	const handle = normalizeHandle(asStr(r.handle));
	return {
		...req,
		...optStrs(r, ['hostname', 'workspace_path', 'last_approved_by', 'last_approved_at']),
		...(handle ? { handle } : {}),
		managed_turn_active: asBool(r.managed_turn_active) ?? false,
		code_prefix: maskCodePrefix(req.code_prefix),
		workspace_familiarity: decodeFamiliarity(r.workspace_familiarity)
	};
}

export function decodePendingList(raw: unknown): PendingExternalAuthView[] {
	const seen = new Set<string>();
	const out: PendingExternalAuthView[] = [];
	for (const entry of asArr(raw)) {
		const decoded = decodePendingExternalAuth(entry);
		if (!decoded || seen.has(decoded.request_id)) continue;
		seen.add(decoded.request_id);
		out.push(decoded);
	}
	return out;
}

/** Semantic familiarity labels and explanations (`doc-102` §4), decoupled from pill CSS classes. */
export function familiarityInfo(familiarity: WorkspaceFamiliarity): FamiliarityInfo {
	switch (familiarity) {
		case 'first_time':
			return {
				label: 'First-time workspace',
				severity: 'warning',
				description:
					'No prior approved external session exists for this workspace. Verify host, path, and terminal code.'
			};
		case 'returning_new_key':
			return {
				label: 'Returning workspace (new key)',
				severity: 'caution',
				description:
					'This workspace was approved before, but it is presenting a different Ed25519 key fingerprint.'
			};
		case 'returning_known_key':
			return {
				label: 'Returning workspace (known key)',
				severity: 'familiar',
				description:
					'This workspace was previously approved with the exact same Ed25519 key fingerprint.'
			};
		default:
			return {
				label: 'Workspace familiarity unknown',
				severity: 'unknown',
				description: 'Workspace history could not be classified. Verify the terminal code.'
			};
	}
}

/** 24-hour date and time for BYOA leases, approvals, and history entries (e.g. "18 Nov, 09:12"). */
export function formatExternalAuthTime(iso: string): string {
	const ms = Date.parse(iso);
	if (!Number.isFinite(ms)) return 'Unknown time';
	return new Date(ms).toLocaleString(undefined, {
		day: 'numeric',
		month: 'short',
		hour: '2-digit',
		minute: '2-digit',
		hourCycle: 'h23'
	});
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
