import { describe, expect, it } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import {
	buildPeopleCanvasItems,
	buildTeamCanvasItems,
	hotPathIds,
	layoutCanvasTree
} from './canvas-layout';
import type { ChartEmployee } from './types';

function emp(overrides: Partial<ChartEmployee> & { id: string; name: string }): ChartEmployee {
	return {
		kind: 'agent',
		handle: overrides.id,
		status: 'active',
		leads: [],
		canSignIn: false,
		revision: 1,
		...overrides
	};
}

describe('org-chart canvas-layout', () => {
	it('builds team hierarchy using owner_team_id or lead manager team and computes layout edges', () => {
		const employees: ChartEmployee[] = [
			emp({
				id: 'owner',
				name: 'Founder',
				kind: 'person',
				canSignIn: true,
				homeTeam: { id: 'exec', name: 'Executive', ref: 'TEAM-1' }
			}),
			emp({
				id: 'eng-lead',
				name: 'Ada',
				reportsToId: 'owner',
				homeTeam: { id: 'eng', name: 'Engineering', ref: 'TEAM-2' }
			})
		];
		const snapshot = {
			teams: [
				{ id: 'exec', name: 'Executive', lead_member_id: 'owner' },
				{ id: 'eng', name: 'Engineering', lead_member_id: 'eng-lead' }
			]
		} as unknown as OrgSnapshot;

		const items = buildTeamCanvasItems(snapshot, employees);
		const layout = layoutCanvasTree(items, new Set());
		expect(layout.nodes).toHaveLength(2);
		expect(layout.edges).toEqual([expect.objectContaining({ fromId: 'exec', toId: 'eng' })]);
		expect(hotPathIds(layout.byId, 'eng')).toEqual(new Set(['eng', 'exec']));
	});

	it('collapses subtrees and reports hidden descendant count', () => {
		const employees: ChartEmployee[] = [
			emp({ id: 'owner', name: 'Founder', kind: 'person', canSignIn: true }),
			emp({ id: 'mgr', name: 'Manager', reportsToId: 'owner' }),
			emp({ id: 'ic', name: 'Engineer', reportsToId: 'mgr' })
		];
		const items = buildPeopleCanvasItems(employees, 'owner');
		const collapsedLayout = layoutCanvasTree(items, new Set(['owner']));
		expect(collapsedLayout.nodes).toHaveLength(1);
		expect(collapsedLayout.nodes[0].collapsed).toBe(true);
		expect(collapsedLayout.nodes[0].hiddenCount).toBe(2);
	});
});
