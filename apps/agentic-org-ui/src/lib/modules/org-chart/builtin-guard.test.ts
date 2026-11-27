import { describe, expect, it, vi } from 'vitest';
import { isBuiltInHandle } from '$lib/proto';
import { createHttpOrgChartApi } from './http-api';

describe('org-chart built-in agent guard & builtinKey propagation', () => {
	it('preserves builtin_key on ChartEmployee and protects HR & Tess with 4-arg isBuiltInHandle', async () => {
		const api = createHttpOrgChartApi({
			http: {
				read: vi.fn(async () => ({
					status: 200,
					body: { ok: true, result: { organizations: [] } }
				})),
				submit: vi.fn()
			},
			orgApi: {
				view: vi.fn(async () => ({
					sample: false,
					snapshot: {
						position: { epoch: 1, seq: 1 },
						teams: [{ id: 'team-1', name: 'Core', version: 1 }],
						members: [
							{
								id: 'm-hr',
								kind: 'agent' as const,
								name: 'HR Lead',
								handle: 'custom-people-ops',
								builtin_key: 'hr',
								home_team_id: 'team-1',
								version: 1,
								owner: false
							},
							{
								id: 'm-tess',
								kind: 'agent' as const,
								name: 'Tess',
								handle: 'tess',
								builtin_key: 'chief_of_staff',
								home_team_id: 'team-1',
								version: 1,
								owner: false
							},
							{
								id: 'm-noor',
								kind: 'agent' as const,
								name: 'Noor',
								handle: 'noor',
								home_team_id: 'team-1',
								version: 1,
								owner: false
							}
						]
					}
				})),
				command: vi.fn()
			},
			membersApi: {
				directory: vi.fn(async () => [])
			}
		});

		const chart = await api.chart('org-1');
		const hr = chart.employees.find((e) => e.id === 'm-hr')!;
		const tess = chart.employees.find((e) => e.id === 'm-tess')!;
		const noor = chart.employees.find((e) => e.id === 'm-noor')!;

		expect(hr.builtinKey).toBe('hr');
		expect(tess.builtinKey).toBe('chief_of_staff');
		expect(noor.builtinKey).toBeUndefined();

		expect(isBuiltInHandle(hr.kind, hr.handle, undefined, hr.builtinKey)).toBe(true);
		expect(isBuiltInHandle(tess.kind, tess.handle, undefined, tess.builtinKey)).toBe(true);
		expect(isBuiltInHandle(noor.kind, noor.handle, undefined, noor.builtinKey)).toBe(false);
	});
});
