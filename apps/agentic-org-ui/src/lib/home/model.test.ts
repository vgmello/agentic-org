import { describe, expect, it } from 'vitest';
import type { OfficeInfo, WorkItemSummary } from '$lib/proto';
import { sampleOrg } from '$lib/org/sample';
import {
	answerExecutiveQuestion,
	deriveDecisionSpotlight,
	deriveExecutiveBrief,
	deriveKpiMetrics,
	deriveProjectCards,
	deriveTaskTimeBreakdown
} from './model';

function makeItem(overrides: Partial<WorkItemSummary>): WorkItemSummary {
	return {
		id: 'item-1',
		kind: 'task',
		short_id: 'T-1',
		title: 'Ship sync update',
		status: 'todo',
		created_by: 'sam',
		created_at: '2026-11-20T08:00:00Z',
		updated_at: '2026-11-21T04:00:00Z',
		severity_confirmed: true,
		reopen_count: 0,
		fixes: [],
		fixed_by: [],
		checklist: { total: 2, done: 2, required_left: 0 },
		done_anyway: false,
		owner_office_id: 'hq',
		generation: 1,
		version: 1,
		deleted: false,
		...overrides
	};
}

const offices: OfficeInfo[] = [
	{ id: 'hq', organization_id: 'org-1', name: 'HQ', role: 'hq', host: 'This Mac', status: 'active' },
	{ id: 'sat', organization_id: 'org-1', name: 'Linux runner', role: 'satellite', host: 'node-2', status: 'offline' }
];

describe('executive home view model', () => {
	it('computes dynamic office online/offline counts without hardcoding any office name', () => {
		const snap = sampleOrg('hq');
		const kpis = deriveKpiMetrics(snap, [], offices, 'you');
		expect(kpis.onlineOffices).toBe(1);
		expect(kpis.offlineOffices).toBe(1);
		expect(kpis.officePillLabel).toBe('1 office offline');
		expect(kpis.officePillTone).toBe('bad');

		const allOnline = deriveKpiMetrics(snap, [], [offices[0], { ...offices[1], status: 'active' }], 'you');
		expect(allOnline.onlineOffices).toBe(2);
		expect(allOnline.offlineOffices).toBe(0);
		expect(allOnline.officePillLabel).toBe('All offices online');
		expect(allOnline.officePillTone).toBe('ok');
	});

	it('applies the >30% waiting-for-agent threshold for coral-rose vs purple highlighting', () => {
		// 2 queued out of 4 open = 50% (> 30%) -> queuedIsHigh = true
		const highQueue = deriveTaskTimeBreakdown(
			[
				makeItem({ id: 't1', status: 'in_progress' }),
				makeItem({ id: 't2', status: 'in_progress' }),
				makeItem({ id: 't3', status: 'todo' }),
				makeItem({ id: 't4', status: 'todo' })
			],
			'you',
			1
		);
		expect(highQueue.queuedPct).toBe(50);
		expect(highQueue.queuedIsHigh).toBe(true);

		// 1 queued out of 4 open = 25% (<= 30%) -> queuedIsHigh = false (purple)
		const normalQueue = deriveTaskTimeBreakdown(
			[
				makeItem({ id: 't1', status: 'in_progress' }),
				makeItem({ id: 't2', status: 'in_progress' }),
				makeItem({ id: 't3', status: 'in_progress' }),
				makeItem({ id: 't4', status: 'todo' })
			],
			'you',
			0
		);
		expect(normalQueue.queuedPct).toBe(25);
		expect(normalQueue.queuedIsHigh).toBe(false);
	});

	it('prioritizes urgent bugs, scoped requests, and owner-blocked tasks in Decision Spotlight', () => {
		const snap = sampleOrg('hq');
		const items = [
			makeItem({ id: 'r1', kind: 'request', status: 'scoped', title: 'Add dark/light themes', project_id: 'beacon' }),
			makeItem({ id: 'b1', kind: 'bug', status: 'new', severity: 'critical', title: 'Auth token race', project_id: 'atlas' }),
			makeItem({ id: 't1', kind: 'task', status: 'in_progress', title: 'Routine chore' })
		];
		const spot = deriveDecisionSpotlight(items, snap, 'you', Date.parse('2026-11-21T05:00:00Z'));
		expect(spot).toHaveLength(2);
		expect(spot[0].id).toBe('b1');
		expect(spot[0].risk.label).toBe('Moderate risk');
		expect(spot[0].primaryAction.label).toBe('Triage bug');
		expect(spot[0].backgroundContext).toContain('T-1');
		expect(spot[1].id).toBe('r1');
		expect(spot[1].primaryAction.kind).toBe('approve_request');
		expect(spot[1].primaryAction.label).toBe('Approve request');
		expect(spot[1].secondaryAction?.label).toBe('Deny');
		expect(spot[1].risk.label).toBe('Low risk');
	});

	it('builds an executive brief with live sections and plain-text copy payload', () => {
		const snap = sampleOrg('hq');
		const items = [
			makeItem({ id: 'r1', kind: 'request', status: 'scoped', title: 'Approve theme rollout', project_id: 'beacon' })
		];
		const brief = deriveExecutiveBrief(
			snap,
			items,
			offices,
			'you',
			'Acme Executive',
			Date.parse('2026-11-21T05:00:00Z')
		);
		expect(brief.sections).toHaveLength(3);
		expect(brief.sections[1].tone).toBe('warn');
		expect(brief.sections[1].body).toContain('Linux runner');
		expect(brief.copyText).toContain('Executive Brief');
		expect(brief.copyText).toContain('Approve theme rollout');
	});

	it('derives project health and executive AI answers from real snapshot + work items', () => {
		const snap = sampleOrg('hq');
		const items = [
			makeItem({ id: 't1', project_id: 'atlas', status: 'done' }),
			makeItem({ id: 'b1', kind: 'bug', project_id: 'atlas', status: 'new', severity: 'critical' }),
			makeItem({ id: 't2', project_id: 'beacon', status: 'done' })
		];
		const cards = deriveProjectCards(snap, items);
		const atlas = cards.find((c) => c.id === 'atlas');
		const beacon = cards.find((c) => c.id === 'beacon');
		expect(atlas?.health).toBe('Off track');
		expect(beacon?.health).toBe('On track');

		const ai = answerExecutiveQuestion('Which offices are offline?', {
			snapshot: snap,
			items,
			offices,
			ownerId: 'you'
		});
		expect(ai.headline).toContain('1 online, 1 offline');
		expect(ai.bullets.some((b) => b.text.includes('Linux runner'))).toBe(true);
	});
});
