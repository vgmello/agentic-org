// Mock for src/lib/modules/employees/api.ts under `bun run dev:mock`: who may hire, and the hire requests list
// (JSON in e2e/fakes/data/employees). `?hire=refused` makes the viewer someone who may hire nowhere;
// `?mock=slow|fails|empty` exercises the list's states.
import type { EmployeesApi } from '../../../src/lib/modules/employees/api';
import type { HireRequest } from '../../../src/lib/modules/employees/types';
import requests from '../data/employees/hire-requests.json' with { type: 'json' };
import { fail, gate, scenario, snapshotFor, teamRef } from './members';

const changed = new Map<string, Partial<HireRequest>>();
const ago = (min: number) => new Date(Date.now() - min * 60_000).toISOString();

export const employeesApi: EmployeesApi = {
	async hireAccess(organizationId) {
		const s = await snapshotFor(organizationId);
		if (new URLSearchParams(location.search).get('hire') === 'refused')
			return { teamIds: [], seesAll: false };
		return { teamIds: s.teams.map((t) => t.id), seesAll: true };
	},
	async hireRequests(organizationId) {
		await gate();
		if (scenario() === 'empty') return [];
		const s = await snapshotFor(organizationId);
		const name = (id: string) => s.members.find((m) => m.id === id)?.name ?? id;
		return requests.map((r) => ({
			id: r.id,
			state: r.state as HireRequest['state'],
			name: r.name,
			role: r.role,
			requestedBy: { id: r.requestedBy, name: name(r.requestedBy) },
			homeTeam: teamRef(s, r.homeTeam) ?? { id: r.homeTeam, name: r.homeTeam, ref: '' },
			createdAt: ago(r.agoMin),
			updatedAt: ago(Math.max(0, r.agoMin - 5)),
			failedStep: r.failedStep,
			doneSteps: r.doneSteps,
			closedReason: r.closedReason,
			employeeId: r.employeeId,
			...changed.get(r.id)
		}));
	},
	async withdraw(_organizationId, requestId) {
		await gate();
		const r = requests.find((x) => x.id === requestId);
		if (!r || (changed.get(requestId)?.state ?? r.state) !== 'drafting')
			throw fail('conflict', 'Only a request that is still drafting can be withdrawn.');
		changed.set(requestId, {
			state: 'withdrawn',
			closedReason: 'Withdrawn by you',
			updatedAt: new Date().toISOString()
		});
	},
	async retry(_organizationId, requestId) {
		await gate();
		const r = requests.find((x) => x.id === requestId);
		if (!r || !r.failedStep) throw fail('conflict', 'This hire has nothing left to finish.');
		// The mock keeps the member out of the org snapshot, so the finished hire is shown as still hiring.
		changed.set(requestId, {
			failedStep: undefined,
			doneSteps: [...(r.doneSteps ?? []), 'Registered with messaging'],
			updatedAt: new Date().toISOString()
		});
	}
};
