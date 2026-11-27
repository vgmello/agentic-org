import { describe, expect, it, vi } from 'vitest';
import type { GatewayHttpTransport, GatewayReply } from '$lib/platform/http/gateway-errors';
import type { OrgSnapshot } from '$lib/proto';
import { createHttpInboxApi } from './http-api';
import { appendPendingExternalAuth } from './http-external-auth';
import { deriveTriageItems } from './features/triage/triage-derive';
import type { InboxItem } from './types';

const ORG = '0193a5c0-0000-7000-8000-000000000001';
const INBOX_PATH = `/api/v1/organizations/${ORG}/inbox`;
const PENDING_PATH = `/api/v1/organizations/${ORG}/external-auth/pending`;

function ok(result: unknown): GatewayReply {
	return { status: 200, body: { result } };
}

const SAMPLE_PENDING = {
	request_id: 'req-ext-1',
	employee_id: 'mem-kai',
	handle: '@kai',
	office_id: 'off-hq',
	office_name: 'HQ Mac',
	hostname: 'studio-mac.local',
	workspace_path: '/Users/dev/agentic-org',
	workspace_label: 'studio-mac:~/agentic-org',
	pubkey_fingerprint: 'a1b2c3d4e5f60718',
	managed_turn_active: true,
	requested_at: '2026-11-27T10:00:00Z',
	expires_at: '2026-11-27T10:10:00Z',
	code_prefix: 'K7P4-••••',
	workspace_familiarity: 'returning_new_key',
	last_approved_by: 'Alex Rivera',
	last_approved_at: '2026-11-20T09:00:00Z'
};

describe('appendPendingExternalAuth & Inbox triage derivation', () => {
	it('appends pending BYOA approvals inside createHttpInboxApi.list() and derives auditChecks', async () => {
		const read = vi.fn().mockImplementation(async (path: string) => {
			if (path === INBOX_PATH) {
				return ok({ as_of: '2026-11-27T10:01:00Z', items: [] });
			}
			if (path === PENDING_PATH) {
				return ok({ pending: [SAMPLE_PENDING] });
			}
			return { status: 404, body: {} };
		});
		const http: GatewayHttpTransport = {
			read,
			submit: vi.fn().mockResolvedValue({ status: 404, body: {} })
		};

		const api = createHttpInboxApi({ http, now: () => '2026-11-27T10:01:00Z' });
		const items = await api.list(ORG);

		expect(read).toHaveBeenCalledWith(PENDING_PATH);
		expect(items).toHaveLength(1);
		expect(items[0]).toMatchObject({
			id: 'external-auth:pending:req-ext-1',
			kind: 'approval',
			source: 'External sign-in',
			title: 'Approve external sign-in for @kai',
			actions: [],
			snoozeable: false,
			state: 'open',
			open: { path: '/members/mem-kai', label: 'Open agent profile' },
			external_auth: {
				request_id: 'req-ext-1',
				office_name: 'HQ Mac',
				code_prefix: 'K7P4-••••',
				workspace_familiarity: 'returning_new_key',
				managed_turn_active: true
			}
		});

		const snapshot: OrgSnapshot = {
			position: { epoch: 1, seq: 1 },
			teams: [],
			members: [
				{
					id: 'mem-kai',
					kind: 'agent',
					name: 'Kai Lindqvist',
					handle: 'kai',
					version: 1
				}
			]
		};
		const derived = deriveTriageItems(
			[],
			snapshot,
			'owner-1',
			items,
			Date.parse('2026-11-27T10:02:00Z')
		);
		expect(derived).toHaveLength(1);
		expect(derived[0].auditChecks).toEqual(
			expect.arrayContaining([
				'Office: HQ Mac',
				'Workspace: studio-mac:~/agentic-org (/Users/dev/agentic-org)',
				'Familiarity: Returning workspace (new key)',
				'Code prefix: K7P4-••••',
				'Key fingerprint: a1b2c3d4e5f60718',
				'Host: studio-mac.local',
				'Managed turn active (takeover confirmation required)'
			])
		);
		expect(derived[0].clearsLabel).toBe('When approved with the terminal code or when it expires');
	});

	it('fails soft on 403, 404, 501, and network errors while rethrowing aborts', async () => {
		for (const status of [403, 404, 501, 503]) {
			const mapped: { item: InboxItem; rawSource?: string }[] = [];
			await appendPendingExternalAuth(
				{ read: vi.fn().mockResolvedValue({ status, body: {} }) },
				ORG,
				'2026-11-27T10:01:00Z',
				mapped
			);
			expect(mapped).toEqual([]);
		}

		const mappedAfterNetErr: { item: InboxItem; rawSource?: string }[] = [];
		await appendPendingExternalAuth(
			{ read: vi.fn().mockRejectedValue(new Error('offline')) },
			ORG,
			'2026-11-27T10:01:00Z',
			mappedAfterNetErr
		);
		expect(mappedAfterNetErr).toEqual([]);

		const ac = new AbortController();
		const abortErr = new DOMException('Aborted', 'AbortError');
		ac.abort(abortErr);
		await expect(
			appendPendingExternalAuth(
				{ read: vi.fn().mockRejectedValue(abortErr) },
				ORG,
				'2026-11-27T10:01:00Z',
				[],
				ac.signal
			)
		).rejects.toBe(abortErr);
	});
});
