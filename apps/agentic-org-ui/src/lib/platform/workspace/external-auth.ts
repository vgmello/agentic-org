// Shared BYOA external-agent wire types, pending decoder, actor resolution, and pure presentation
// helpers (`TASK-249`). Lives in `platform/workspace` with zero module or component dependencies
// so `members` and `inbox` share one canonical contract without cross-module private imports.
import { isBuiltInAgent, isBuiltInHandle, type Member, type MemberKind } from '$lib/proto';
import { asArr, asBool, asRec, asStr, optStrs, reqStrs } from '$lib/platform/http/decode';

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
export type FamiliarityToneKey = 'owner' | 'bad' | 'ok' | 'neutral';

export interface FamiliarityInfo {
	label: string;
	severity: FamiliaritySeverity;
	description: string;
}

export const FAMILIARITY_TONE_KEY: Record<FamiliaritySeverity, FamiliarityToneKey> = {
	warning: 'owner',
	caution: 'bad',
	familiar: 'ok',
	unknown: 'neutral'
};

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

function normalizeRoleForBuiltInCheck(role: string | null | undefined): string | undefined {
	return role?.trim().replace(/[_-]+/g, ' ');
}

/**
 * True when an agent may use BYOA external sign-in. Built-in organization agents (HR and
 * Tess / Chief of Staff) are immutable and cannot use external sign-in.
 */
export function canUseExternalSignIn(
	member: Member | undefined,
	fallback?: { kind?: MemberKind; handle?: string | null; role?: string | null }
): boolean {
	if (member) {
		if (member.kind !== 'agent') return false;
		if (isBuiltInAgent(member)) return false;
		if (!member.builtin_key) {
			const normalizedRole = normalizeRoleForBuiltInCheck(member.agent?.role);
			if (
				isBuiltInHandle(member.kind, member.handle ?? member.agent?.messaging_name, normalizedRole)
			) {
				return false;
			}
		}
		return true;
	}
	if (!fallback) return false;
	return (
		fallback.kind === 'agent' &&
		!isBuiltInHandle(fallback.kind, fallback.handle, normalizeRoleForBuiltInCheck(fallback.role))
	);
}

/** Builds a member-id -> human display name map from an organization snapshot's member list. */
export function buildMemberNameMap(
	members?: readonly { id: string; name?: string; handle?: string }[]
): ReadonlyMap<string, string> {
	const map = new Map<string, string>();
	if (!members) return map;
	for (const member of members) {
		const display = member.name?.trim() || formatHandle(member.handle);
		if (display) map.set(member.id, display);
	}
	return map;
}

/**
 * Resolves a wire actor/approver member ID (`approved_by`, `last_approved_by`, `actor_id`) to a
 * human display name, falling back to `'Unknown member'` so raw UUIDv7 strings never reach the UI.
 */
export function resolveExternalAuthActor(
	actorId: string | undefined,
	membersById?: ReadonlyMap<string, string>
): string | undefined {
	const trimmed = actorId?.trim();
	if (!trimmed) return undefined;
	return membersById?.get(trimmed) ?? 'Unknown member';
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
export function formatExternalAuthTime(iso: string, timeZone?: string): string {
	const ms = Date.parse(iso);
	if (!Number.isFinite(ms)) return 'Unknown time';
	return new Date(ms).toLocaleString(undefined, {
		day: 'numeric',
		month: 'short',
		hour: '2-digit',
		minute: '2-digit',
		hourCycle: 'h23',
		...(timeZone ? { timeZone } : {})
	});
}

export function formatLastApproved(
	by?: string,
	at?: string,
	membersById?: ReadonlyMap<string, string>
): string | undefined {
	const actor = resolveExternalAuthActor(by, membersById);
	const formattedAt =
		at && Number.isFinite(Date.parse(at)) ? formatExternalAuthTime(at) : undefined;
	if (actor && formattedAt) return `Last approved by ${actor} · ${formattedAt}`;
	if (actor) return `Last approved by ${actor}`;
	if (formattedAt) return `Last approved ${formattedAt}`;
	return undefined;
}

export interface ApprovedSessionToast {
	kind: 'success' | 'warning';
	title: string;
	description: string;
}

/**
 * Formats the post-approval toast from the returned `ExternalSessionView`, surfacing a clear
 * warning if the server-returned `session.employee_id` disagrees with the viewed agent.
 */
export function formatApprovedSessionToast(
	session: ExternalSessionView,
	options: {
		expectedEmployeeId?: string;
		expectedAgentName: string;
		membersById?: ReadonlyMap<string, string>;
	}
): ApprovedSessionToast {
	const mismatched = Boolean(
		options.expectedEmployeeId && session.employee_id !== options.expectedEmployeeId
	);
	const resolvedActual =
		options.membersById?.get(session.employee_id) ??
		formatHandle(session.handle) ??
		(mismatched ? 'another agent' : options.expectedAgentName);
	const description = `${session.workspace_label} · expires ${formatExternalAuthTime(session.expires_at)}`;
	if (mismatched) {
		return {
			kind: 'warning',
			title: `Approved external sign-in for ${resolvedActual} (different agent than ${options.expectedAgentName}).`,
			description
		};
	}
	return {
		kind: 'success',
		title: `Approved external sign-in for ${resolvedActual}.`,
		description
	};
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
