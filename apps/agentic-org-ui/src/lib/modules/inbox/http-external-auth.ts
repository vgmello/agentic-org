// Live Inbox adapter for pending BYOA external-agent sign-in requests (`TASK-249`).
// Calls `GET /api/v1/organizations/{org}/external-auth/pending`, fails soft on 403/404/501,
// and dynamically loads the shared pending decoder only when pending requests exist so Shell
// first-paint JS stays within its shrink-only bundle budget.
import { asRec } from '$lib/platform/http/decode';
import type { GatewayHttpTransport, GatewayReply } from '$lib/platform/http/gateway-errors';
import type { PendingExternalAuthView } from '$lib/platform/workspace/external-auth';
import type { InboxItem } from './types';

export const EXTERNAL_AUTH_INBOX_PREFIX = 'external-auth:pending:';

const FAMILIARITY_LABEL: Record<PendingExternalAuthView['workspace_familiarity'], string> = {
	first_time: 'First-time workspace',
	returning_new_key: 'Returning workspace (new key)',
	returning_known_key: 'Returning workspace (known key)',
	unknown: 'Workspace familiarity unknown'
};

function recommendationFor(req: PendingExternalAuthView): string {
	switch (req.workspace_familiarity) {
		case 'returning_known_key':
			return 'Returning workspace with the same key. Verify the full terminal code before approving.';
		case 'returning_new_key':
			return 'Returning workspace presenting a new key. Verify the public key fingerprint and terminal code before approving.';
		case 'first_time':
			return 'First-time workspace. Confirm host, workspace path, and full terminal code before approving.';
		default:
			return 'Confirm host, workspace path, and full terminal code before approving.';
	}
}

export function mapPendingExternalAuthToInboxItem(
	req: PendingExternalAuthView,
	fallbackIso: string
): InboxItem {
	const cleanHandle = req.handle?.trim().replace(/^@+/, '');
	const handleLabel = cleanHandle ? `@${cleanHandle}` : req.employee_id;
	const familiarityLabel =
		FAMILIARITY_LABEL[req.workspace_familiarity] ?? FAMILIARITY_LABEL.unknown;
	const takeoverNote = req.managed_turn_active
		? ' A managed turn is active and will be parked on takeover.'
		: '';
	return {
		id: `${EXTERNAL_AUTH_INBOX_PREFIX}${req.request_id}`,
		kind: 'approval',
		source: 'External sign-in',
		title: `Approve external sign-in for ${handleLabel}`,
		who: {
			id: req.employee_id,
			name: handleLabel,
			kind: 'agent'
		},
		why: `${familiarityLabel} · ${req.workspace_label} on ${req.office_name} (code ${req.code_prefix}).${takeoverNote}`,
		recommendation: recommendationFor(req),
		created_at: req.requested_at || fallbackIso,
		expires_at: req.expires_at,
		actions: [],
		open: {
			path: `/members/${encodeURIComponent(req.employee_id)}`,
			label: 'Open agent profile'
		},
		snoozeable: false,
		state: 'open',
		external_auth: { ...req }
	};
}

async function readRawPending(
	http: Pick<GatewayHttpTransport, 'read'>,
	organizationId: string,
	signal?: AbortSignal
): Promise<unknown[] | undefined> {
	const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/external-auth/pending`;
	let reply: GatewayReply | undefined;
	try {
		reply = await (signal ? http.read(path, signal) : http.read(path));
	} catch (err) {
		if (signal?.aborted) throw err;
		return undefined;
	}
	if (!reply || reply.status !== 200) return undefined;
	const root = asRec(reply.body);
	const result = root?.error ? undefined : asRec(root?.result);
	return Array.isArray(result?.pending) && result.pending.length > 0 ? result.pending : undefined;
}

/**
 * Fetches pending BYOA external sign-in requests and appends deduplicated `InboxItem`s.
 * Fails soft on 403/404/501/non-200 or network errors so the main Inbox queue remains usable;
 * always rethrows when `signal` is aborted.
 */
export async function appendPendingExternalAuth(
	http: Pick<GatewayHttpTransport, 'read'>,
	organizationId: string,
	asOf: string,
	mapped: { item: InboxItem; rawSource?: string }[],
	signal?: AbortSignal
): Promise<void> {
	const rawPending = await readRawPending(http, organizationId, signal);
	if (!rawPending) return;
	const { decodePendingList } = await import('$lib/platform/workspace/external-auth');
	if (signal?.aborted) return;
	const seen = new Set(mapped.map((entry) => entry.item.id));
	for (const req of decodePendingList(rawPending)) {
		const item = mapPendingExternalAuthToInboxItem(req, asOf);
		if (seen.has(item.id)) continue;
		seen.add(item.id);
		mapped.push({ item, rawSource: 'external_auth' });
	}
}
