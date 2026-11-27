// Live wire contracts for BYOA external-agent sign-in approvals and lease management (`TASK-249`).
// Mirrors `crates/org-proto/src/external_auth.rs` and `apps/agentic-org-web/src/api/external_auth_routes/`.
import type {
	ExternalSessionView,
	PendingExternalAuthView,
	WorkspaceFamiliarity
} from '$lib/platform/workspace/external-auth';

export type { ExternalSessionView, PendingExternalAuthView, WorkspaceFamiliarity };

export type ExternalHistoryEvent =
	'approved' | 'renewed' | 'superseded' | 'revoked' | 'expired' | (string & {});

export interface ExternalHistoryEntry {
	/** Stable client-assigned deduplication key for keyed `{#each}` rendering. */
	id: string;
	at: string;
	event: ExternalHistoryEvent;
	session_id?: string;
	actor_id?: string;
	detail?: string;
	hostname?: string;
	workspace_path?: string;
	workspace_label?: string;
	pubkey_fingerprint?: string;
}

export interface MemberExternalSessionsView {
	employee_id: string;
	active?: ExternalSessionView;
	pending: PendingExternalAuthView[];
	history: ExternalHistoryEntry[];
}

export interface ExternalAuthApproveInput {
	/** Full Crockford base32 code (`LLLL-SSSS`). Never retain or log after submission. */
	code: string;
	command_id?: string;
	/** Requested lease lifetime in seconds (`300..=604800`). */
	ttl_seconds?: number;
	/** Park a live managed turn first instead of refusing with `409 conflict`. */
	takeover?: boolean;
}

export interface ExternalAuthRenewInput {
	command_id?: string;
	session_id?: string;
	/** Extended lease lifetime from now in seconds (`300..=604800`). */
	ttl_seconds?: number;
}

export interface ExternalAuthRevokeInput {
	command_id?: string;
	session_id?: string;
}

export interface ExternalAuthRevokeResult {
	employee_id: string;
	revoked: boolean;
	session_id?: string;
}

export type ExternalAuthErrorCode =
	| 'invalid_request'
	| 'not_authorized'
	| 'owner_protected'
	| 'not_found'
	| 'conflict'
	| 'expired'
	| 'rate_limited'
	| 'unavailable'
	| 'unknown';

export interface ExternalAuthError {
	source: 'api';
	code: ExternalAuthErrorCode;
	status?: number;
	message: string;
	retry_after_ms?: number;
}

export interface ExternalAuthApi {
	pending(
		organizationId: string,
		options?: { memberId?: string; signal?: AbortSignal }
	): Promise<PendingExternalAuthView[]>;
	memberSessions(
		organizationId: string,
		memberId: string,
		options?: { limit?: number; signal?: AbortSignal }
	): Promise<MemberExternalSessionsView>;
	approve(
		organizationId: string,
		input: ExternalAuthApproveInput,
		signal?: AbortSignal
	): Promise<ExternalSessionView>;
	renew(
		organizationId: string,
		memberId: string,
		input?: ExternalAuthRenewInput,
		signal?: AbortSignal
	): Promise<ExternalSessionView>;
	revoke(
		organizationId: string,
		memberId: string,
		input?: ExternalAuthRevokeInput,
		signal?: AbortSignal
	): Promise<ExternalAuthRevokeResult>;
}
