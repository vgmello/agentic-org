import { describe, expect, it, vi } from 'vitest';
import { createHttpOfficeApi, type OfficeHttpTransport } from './http-api';

const INSTANCE = '0193a5c0-0000-7000-8000-000000000001';
const ORG_A = '0193a5c0-0000-7000-8000-000000000002';
const ORG_B = '0193a5c0-0000-7000-8000-000000000003';
const OFFICE_A = '0193a5c0-0000-7000-8000-000000000004';
const OFFICE_B = '0193a5c0-0000-7000-8000-000000000005';
const OP = '01a1c554-5200-7000-8000-000000000006';

const LIST_REPLY = {
	status: 200,
	body: {
		result: {
			organizations: [
				{
					id: ORG_A,
					slug: 'alpha',
					name: 'Alpha',
					created_at: '2026-11-20T00:00:00Z',
					employee_id: 'emp-a',
					recovery: 'acknowledged',
					offices: [
						{
							id: OFFICE_A,
							organization_id: ORG_A,
							name: 'Alpha HQ',
							role: 'hq',
							host: 'mac-mini',
							status: 'active'
						}
					]
				},
				{
					id: ORG_B,
					slug: 'beta',
					name: 'Beta',
					created_at: '2026-11-20T00:00:01Z',
					employee_id: 'emp-b',
					recovery: 'pending',
					offices: [
						{
							id: OFFICE_B,
							organization_id: ORG_B,
							name: 'Beta HQ',
							role: 'hq',
							host: 'mac-studio',
							status: 'offline',
							status_reason: 'crashed'
						}
					]
				}
			],
			creating: []
		}
	}
};

describe('createHttpOfficeApi', () => {
	it('places the selected organization last in connections() and selects on openOrganization()', async () => {
		let selected: string | undefined = ORG_A;
		const http: OfficeHttpTransport = {
			read: vi.fn().mockResolvedValue(LIST_REPLY),
			submit: vi.fn()
		};
		const api = createHttpOfficeApi({
			http,
			instanceId: () => INSTANCE,
			getSelectedOrganizationId: () => selected,
			selectOrganization: (id) => {
				selected = id;
			}
		});

		const conns = await api.connections();
		expect(conns.map((c) => c.organization.id)).toEqual([ORG_B, ORG_A]);
		expect(conns[1].office).toEqual({
			id: OFFICE_A,
			organization_id: ORG_A,
			name: 'Alpha HQ',
			role: 'hq',
			host: 'mac-mini',
			status: 'active'
		});

		const opened = await api.openOrganization(ORG_B);
		expect(opened.organization.name).toBe('Beta');
		expect(selected).toBe(ORG_B);
	});

	it('posts office restart with UUIDv7 operation_id and office scope', async () => {
		const submit = vi.fn().mockResolvedValue({
			status: 200,
			body: { outcome: 'completed', operation_id: OP, result: {} }
		});
		const api = createHttpOfficeApi({
			http: { read: vi.fn().mockResolvedValue(LIST_REPLY), submit },
			instanceId: () => INSTANCE,
			newId: () => OP,
			getSelectedOrganizationId: () => ORG_B,
			selectOrganization: () => {}
		});

		await api.restartOffice(OFFICE_B);
		expect(submit).toHaveBeenCalledWith(
			`/api/v1/organizations/${ORG_B}/offices/${OFFICE_B}/restart`,
			{
				operation_id: OP,
				scope: { kind: 'office', instance_id: INSTANCE, org_id: ORG_B, office_id: OFFICE_B },
				params: {}
			}
		);
	});

	it('surfaces restart refusal errors cleanly', async () => {
		const submit = vi.fn().mockResolvedValue({
			status: 409,
			body: { outcome: 'rejected', error: { code: 'conflict', message: 'Already starting.' } }
		});
		const api = createHttpOfficeApi({
			http: { read: vi.fn().mockResolvedValue(LIST_REPLY), submit },
			instanceId: () => INSTANCE,
			newId: () => OP,
			getSelectedOrganizationId: () => ORG_A,
			selectOrganization: () => {}
		});

		await expect(api.restartOffice(OFFICE_A)).rejects.toEqual({
			source: 'api',
			code: 'conflict',
			message: 'Already starting.'
		});
	});
});
