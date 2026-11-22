import { describe, expect, it } from 'vitest';
import type { OfficeInfo, WorkItemSummary } from '$lib/proto';
import type { AgentBudgets, OrganizationCost } from '$lib/modules/costs/types';
import type { InboxItem } from '$lib/modules/inbox/types';
import { sampleOrg } from '$lib/org/sample';
import {
	answerExecutiveQuestion,
	deriveDecisionSpotlight,
	deriveExecutiveBrief,
	deriveFinancialKpi,
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
	{
		id: 'hq',
		organization_id: 'org-1',
		name: 'HQ',
		role: 'hq',
		host: 'This Mac',
		status: 'active'
	},
	{
		id: 'sat',
		organization_id: 'org-1',
		name: 'Linux runner',
		role: 'satellite',
		host: 'node-2',
		status: 'offline'
	}
];

const sampleOrgCost: OrganizationCost = {
	period: { kind: 'this_month', endsAt: '2026-11-21T05:00:00Z', timeZone: 'UTC', toNow: true },
	totals: {
		records: 24,
		knownCostMicroUsd: 142_000_000,
		knownTokens: 12_500_000,
		cacheReadTokens: 3_100_000,
		tokenIncompleteRecords: 0,
		costIncompleteRecords: 0,
		unpricedRecords: 0,
		unknownCostRecords: 0,
		includesApiEquivalent: false
	},
	byProject: [],
	byConsumer: [],
	byWorkType: [],
	coverage: { checkpointAt: '2026-11-21T04:55:00Z', gaps: [] }
};

const sampleBudgets: AgentBudgets = {
	zone: { name: 'UTC', source: 'hq' },
	agents: [
		{
			memberId: 'jules',
			name: 'Jules',
			monthly: {
				unit: 'usd',
				limit: 180_000_000,
				spent: 180_000_000,
				complete: true,
				period: {
					key: '2026-11',
					startsAt: '2026-11-01T00:00:00Z',
					endsAt: '2026-12-01T00:00:00Z'
				},
				hold: 'held'
			}
		}
	]
};

describe('executive home view model', () => {
	it('computes dynamic office online/offline counts without hardcoding any office name', () => {
		const snap = sampleOrg('hq');
		const kpis = deriveKpiMetrics(snap, [], offices, 'you');
		expect(kpis.onlineOffices).toBe(1);
		expect(kpis.offlineOffices).toBe(1);
		expect(kpis.officePillLabel).toBe('1 office offline');
		expect(kpis.officePillTone).toBe('bad');

		const allOnline = deriveKpiMetrics(
			snap,
			[],
			[offices[0], { ...offices[1], status: 'active' }],
			'you'
		);
		expect(allOnline.onlineOffices).toBe(2);
		expect(allOnline.offlineOffices).toBe(0);
		expect(allOnline.officePillLabel).toBe('All offices online');
		expect(allOnline.officePillTone).toBe('ok');
	});

	it('derives real financials KPI from costsApi snapshots and degrades honestly when unavailable', () => {
		const unavailable = deriveFinancialKpi(undefined, undefined, 5);
		expect(unavailable.available).toBe(false);
		expect(unavailable.spentLabel).toBe('—');

		const live = deriveFinancialKpi(sampleOrgCost, sampleBudgets, 10);
		expect(live.available).toBe(true);
		expect(live.spentLabel).toBe('$142');
		expect(live.budgetLabel).toBe('/ $180 monthly');
		expect(live.usagePct).toBe(79);
		expect(live.pillLabel).toBe('Jules at cap');
		expect(live.pillTone).toBe('warn');
	});

	it('applies the >30% waiting-for-agent threshold for coral-rose vs purple highlighting', () => {
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
		expect(highQueue.primaryLink.href).toBe('/settings/offices');
		expect(highQueue.secondaryLink?.href).toBe('/teams/members');

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

	it('reconciles inbox items and work items in Decision Spotlight without duplicates', () => {
		const snap = sampleOrg('hq');
		const inboxItems: InboxItem[] = [
			{
				id: 'in1',
				kind: 'approval',
				source: 'Release sign-off',
				title: "Approve publishing Kai's sync update to v1.0",
				who: { id: 'ada', name: 'Ada', kind: 'agent' },
				project: { id: 'atlas', name: 'Atlas' },
				why: 'All 412 automated checks passed.',
				recommendation: 'Ada recommends: approve.',
				created_at: '2026-11-21T04:45:00Z',
				expires_at: '2026-11-21T08:45:00Z',
				actions: ['approve', 'deny'],
				approve_label: 'Approve release',
				open: { path: '/work/T-1', label: 'Open T-1' },
				state: 'open'
			}
		];
		const items = [
			makeItem({
				id: 'r1',
				kind: 'request',
				short_id: 'R-1',
				status: 'scoped',
				title: 'Add dark/light themes',
				project_id: 'beacon'
			}),
			makeItem({
				id: 'b1',
				kind: 'bug',
				short_id: 'B-1',
				status: 'new',
				severity: 'critical',
				title: 'Auth token race',
				project_id: 'atlas'
			}),
			makeItem({
				id: 't1',
				kind: 'task',
				short_id: 'T-1',
				status: 'blocked',
				assignee_id: 'you',
				title: 'Duplicate via ref'
			})
		];
		const spot = deriveDecisionSpotlight(
			items,
			snap,
			'you',
			Date.parse('2026-11-21T05:00:00Z'),
			inboxItems
		);
		expect(spot).toHaveLength(3);
		expect(spot[0].sourceKind).toBe('inbox');
		expect(spot[0].primaryAction).toEqual({ kind: 'approve_inbox', label: 'Approve release' });
		expect(spot[0].secondaryAction).toEqual({ kind: 'deny_inbox', label: 'Deny' });
		expect(spot[1].id).toBe('b1');
		expect(spot[2].id).toBe('r1');
	});

	it('builds a 3-lens executive brief (Today, 7-Day, Board) with plain-text copy payloads', () => {
		const snap = sampleOrg('hq');
		const items = [
			makeItem({
				id: 'r1',
				kind: 'request',
				status: 'scoped',
				title: 'Approve theme rollout',
				project_id: 'beacon'
			})
		];
		const brief = deriveExecutiveBrief(
			snap,
			items,
			offices,
			'you',
			'Acme Executive',
			Date.parse('2026-11-21T05:00:00Z'),
			{ orgCost: sampleOrgCost, agentBudgets: sampleBudgets }
		);
		expect(brief.sections).toHaveLength(3);
		expect(brief.lenses.today.sections).toHaveLength(3);
		expect(brief.lenses.week.sections).toHaveLength(3);
		expect(brief.lenses.board.sections).toHaveLength(3);
		expect(brief.lenses.today.sections[1].tone).toBe('warn');
		expect(brief.lenses.today.sections[1].body).toContain('Linux runner');
		expect(brief.lenses.week.copyText).toContain('$142');
		expect(brief.lenses.board.copyText).toContain('Executive Brief (Board)');
		expect(brief.copyText).toContain('Approve theme rollout');
	});

	it('derives project health, work links, and executive AI answers from real domain data', () => {
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
		expect(atlas?.statusPillClass).toBe('t-red');
		expect(atlas?.workHref).toBe('/projects/atlas/work');
		expect(beacon?.health).toBe('On track');
		expect(beacon?.statusPillClass).toBe('t-green');

		const ai = answerExecutiveQuestion('Which offices are offline?', {
			snapshot: snap,
			items,
			offices,
			ownerId: 'you'
		});
		expect(ai.headline).toContain('1 online, 1 offline');
		expect(ai.bullets.some((b) => b.text.includes('Linux runner'))).toBe(true);
		expect(ai.bullets.some((b) => b.href === '/settings/offices')).toBe(true);
	});
});
