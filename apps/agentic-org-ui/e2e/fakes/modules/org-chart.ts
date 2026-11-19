// Mock for src/lib/modules/org-chart/api.ts under `bun run dev:mock`. Employees, home teams, the owner and sign-in
// access come from the members mock (which reads the org mock), so the chart, the directory, team pages and profiles
// agree, and edits made anywhere show here. Change manager writes `member.set_reports_to` to the org mock with the
// expected revision, refusing a stale revision or a reporting loop as HQ would.
// `?mock=slow|fails|empty|unavailable` exercises the page states; `?mock=large` adds invented agents so the tree has
// more than 50 nodes; `?mock=stale` and `?mock=loop` make someone else change the record just before the first
// Change manager is saved, so it is refused with the latest lines.
import { orgApi } from '$lib/org/api';
import { officeApi } from '$lib/office/api';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { OrgChartApi } from '../../../src/lib/modules/org-chart/api';
import type { ChartEmployee } from '../../../src/lib/modules/org-chart/types';
import large from '../data/org-chart/large.json' with { type: 'json' };
import { fail, gate, rows, scenario, snapshotFor, teamRef, viewerId } from './members';

/** `?mock=stale|loop` interfere once per page load, on the first save. */
let interfered = false;

function invented(): ChartEmployee[] {
	const out: ChartEmployee[] = large.managers.map((m) => ({
		id: m.id,
		kind: 'agent',
		name: m.name,
		handle: m.id.slice(3),
		status: 'active',
		reportsToId: m.reportsTo,
		leads: [],
		office: null,
		canSignIn: false,
		revision: 1
	}));
	let n = 0;
	for (const last of large.lastNames)
		for (const first of large.firstNames) {
			const boss = large.managers[n++ % large.managers.length];
			out.push({
				id: `lg-${first}-${last}`.toLowerCase(),
				kind: 'agent',
				name: `${first} ${last}`,
				handle: `${first}-${last}`.toLowerCase(),
				status: 'active',
				reportsToId: boss.id,
				leads: [],
				office: null,
				canSignIn: false,
				revision: 1
			});
		}
	return out;
}

export const orgChartApi: OrgChartApi = {
	async chart(organizationId) {
		await gate();
		// Long enough after the page starts for "Still loading" (2 s) to show on a busy machine.
		if (scenario() === 'slow') await new Promise((r) => setTimeout(r, 3000));
		// Exactly what the production adapter answers, so the page shows its unavailable state.
		if (scenario() === 'unavailable') await unavailableCall();
		const s = await snapshotFor(organizationId);
		const connection = (await officeApi.connections()).find(
			(c) => c.organization.id === organizationId
		);
		const all = await rows(organizationId);
		const ownerId = all.find((r) => r.owner)?.id ?? '';
		const employees: ChartEmployee[] =
			scenario() === 'empty'
				? []
				: all.flatMap((r) =>
						r.status === 'inactive'
							? []
							: [
									{
										id: r.id,
										kind: r.kind,
										name: r.name,
										handle: r.handle,
										status: r.status,
										homeTeam: r.homeTeam,
										reportsToId: r.reportsTo?.id,
										leads: s.teams
											.filter((t) => t.lead_member_id === r.id)
											.flatMap((t) => teamRef(s, t.id) ?? []),
										office: r.office,
										canSignIn: r.canSignIn,
										revision: s.members.find((m) => m.id === r.id)?.version ?? 1
									}
								]
					);
		if (scenario() === 'large') employees.push(...invented());
		return {
			organization: { id: organizationId, name: connection?.organization.name ?? '' },
			ownerId,
			viewerId: viewerId(),
			employees
		};
	},

	async changeManager(organizationId, employeeId, managerId, expectedRevision) {
		await gate();
		if (scenario() === 'unavailable') await unavailableCall();
		let s = await snapshotFor(organizationId);
		const find = (id: string | null | undefined) => s.members.find((m) => m.id === id);
		const employee = find(employeeId);
		if (!employee) throw fail('not_found', "That employee isn't in this organization any more.");
		const manager = managerId ? find(managerId) : undefined;
		if (managerId && !manager)
			throw fail('not_found', "That manager isn't in this organization any more. Choose another.");

		// Someone else changes the record first: the employee's line (stale), or the chosen manager's (loop).
		if (!interfered && (scenario() === 'stale' || scenario() === 'loop')) {
			interfered = true;
			if (scenario() === 'stale')
				await orgApi.command(organizationId, {
					method: 'member.set_reports_to',
					params: { member_id: employeeId, expected_version: employee.version }
				});
			else if (manager)
				await orgApi.command(organizationId, {
					method: 'member.set_reports_to',
					params: {
						member_id: manager.id,
						expected_version: manager.version,
						reports_to_member_id: employeeId
					}
				});
			s = await snapshotFor(organizationId);
		}

		const now = find(employeeId)!;
		if (now.version !== expectedRevision)
			throw fail('conflict', `${now.name} changed while you were editing. Here's the latest.`);
		if (managerId) {
			const seen = new Set<string>();
			for (
				let up: string | undefined = managerId;
				up && !seen.has(up);
				up = find(up)?.reports_to_member_id
			) {
				if (up === employeeId)
					throw fail(
						'conflict',
						`${find(managerId)!.name} now reports to ${now.name}, so ${now.name} can't report to ${find(managerId)!.name}. Choose another manager.`
					);
				seen.add(up);
			}
		}
		await orgApi.command(organizationId, {
			method: 'member.set_reports_to',
			params: {
				member_id: employeeId,
				expected_version: now.version,
				...(managerId ? { reports_to_member_id: managerId } : {})
			}
		});
	}
};
