import { describe, expect, it, vi } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import { createHttpShellApi } from './http-api';

const ORG = '0193a5c0-0000-7000-8000-000000000001';
const SNAPSHOT: OrgSnapshot = {
	position: { epoch: 1, seq: 1 },
	teams: [{ id: 't1', name: 'Engineering', version: 1 }],
	members: [{ id: 'm1', kind: 'person', name: 'Ada Lovelace', handle: 'ada', version: 1 }]
};

describe('createHttpShellApi', () => {
	it('populates navigation teams/projects and searches live projects and members', async () => {
		const api = createHttpShellApi({
			http: {
				read: vi.fn().mockResolvedValue({
					status: 200,
					body: {
						result: {
							projects: [
								{ id: 'p1', key: 'P-1', name: 'Checkout', slug: 'checkout', archived: false }
							]
						}
					}
				}),
				submit: vi.fn()
			},
			orgApi: { view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }) }
		});

		const nav = await api.navigation(ORG);
		expect(nav).toEqual({
			inboxOpen: null,
			projects: [{ id: 'p1', name: 'Checkout' }],
			teams: [{ id: 't1', name: 'Engineering' }]
		});

		const hits = await api.search(ORG, 'ada');
		expect(hits.members).toEqual([
			{ id: 'm1', title: 'Ada Lovelace', meta: 'Person', href: '/members/m1' }
		]);
		expect(hits.work).toEqual([]);
	});
});
