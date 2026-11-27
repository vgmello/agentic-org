import { describe, expect, it } from 'vitest';
import type { OfficeInfo, WorkItemSummary } from '$lib/proto';
import type { AgentBudgets, OrganizationCost } from '$lib/modules/costs/types';
import type { InboxItem } from '$lib/modules/inbox/types';
import { sampleOrg } from '$lib/modules/organization/sample';
import {
	KPI_PILLAR_ORDER,
	answerExecutiveQuestion,
	deriveDecisionSpotlight,
	deriveExecutiveBrief,
	deriveFinancialKpi,
	deriveKpiMetrics,
	deriveProjectCards,
	deriveTaskTimeBreakdown,
	deriveUpcomingMilestones,
	formatCycleDuration
} from './index';

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

	it('distinguishes Inbox loading and unavailable states in Executive Brief when spotlight is empty', () => {
		const snap = sampleOrg('Acme');
		const loadingBrief = deriveExecutiveBrief(
			snap,
			[],
			offices,
			'you',
			'Acme',
			Date.parse('2026-11-21T06:00:00Z'),
			{
				inboxItems: [],
				inboxState: 'loading'
			}
		);
		expect(loadingBrief.lenses.today.sections.find((s) => s.id === 'queue')?.body).toBe(
			'Checking Inbox for pending decisions…'
		);
		expect(loadingBrief.lenses.board.sections.find((s) => s.id === 'readiness')?.body).toContain(
			'Checking Inbox for pending decisions…'
		);

		const unavailableBrief = deriveExecutiveBrief(
			snap,
			[],
			offices,
			'you',
			'Acme',
			Date.parse('2026-11-21T06:00:00Z'),
			{
				inboxItems: [],
				inboxState: 'unavailable'
			}
		);
		const queue = unavailableBrief.lenses.today.sections.find((s) => s.id === 'queue');
		expect(queue?.body).toBe(
			'Inbox status unavailable — check Inbox directly for pending decisions.'
		);
		expect(queue?.body).not.toContain('queue is clear');
		const readiness = unavailableBrief.lenses.board.sections.find((s) => s.id === 'readiness');
		expect(readiness?.body).toContain(
			'Inbox status unavailable — check Inbox directly for pending decisions.'
		);
	});

	it('derives upcoming milestones, spotlight spot-jump IDs, and decision-scoped Org AI answers', () => {
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
		const homeProjects = [
			{
				id: 'atlas',
				name: 'Atlas',
				done: 4,
				total: 6,
				open: 2,
				blocked: 0,
				due: '2026-11-26',
				lead: { id: 'ada', name: 'Ada' },
				members: 3
			},
			{
				id: 'beacon',
				name: 'Beacon',
				done: 3,
				total: 3,
				open: 0,
				blocked: 0,
				due: '2026-11-29',
				lead: { id: 'sam', name: 'Sam' },
				members: 2
			}
		];
		const cards = deriveProjectCards(snap, [], { inboxItems, homeProjects });
		expect(cards.find((c) => c.id === 'atlas')?.spotDecisionId).toBe('in1');
		expect(cards.find((c) => c.id === 'beacon')?.isClear).toBe(true);

		const milestones = deriveUpcomingMilestones(snap, [], Date.parse('2026-11-25T12:00:00Z'), {
			inboxItems,
			homeProjects
		});
		expect(milestones).toHaveLength(2);
		expect(milestones[0].month).toBe('Nov');
		expect(milestones[0].day).toBe('26');
		expect(milestones[0].statusLabel).toBe('Tomorrow');
		expect(milestones[0].spotId).toBe('in1');

		const explained = answerExecutiveQuestion(
			"Explain the risk, impact, and recommendation for: Approve publishing Kai's sync update to v1.0",
			{
				snapshot: snap,
				items: [],
				offices,
				ownerId: 'you',
				domain: { inboxItems },
				decisionId: 'in1'
			}
		);
		expect(explained.headline).toContain("Approve publishing Kai's sync update to v1.0");
		expect(explained.bullets[0].text).toContain('Ada recommends: approve.');
	});

	it('resolves exact decisionId even when decision titles are prefixes of each other', () => {
		const snap = sampleOrg('hq');
		const inboxItems: InboxItem[] = [
			{
				id: 'rel-v1',
				kind: 'approval',
				source: 'Release',
				title: 'Approve release v1',
				who: { id: 'ada', name: 'Ada', kind: 'agent' },
				project: { id: 'atlas', name: 'Atlas' },
				why: 'Base release ready.',
				recommendation: 'Ship v1 base.',
				created_at: '2026-11-21T04:45:00Z',
				actions: ['approve', 'deny'],
				state: 'open'
			},
			{
				id: 'rel-v1-1',
				kind: 'approval',
				source: 'Release',
				title: 'Approve release v1.1',
				who: { id: 'sam', name: 'Sam', kind: 'person' },
				project: { id: 'beacon', name: 'Beacon' },
				why: 'Patch release ready.',
				recommendation: 'Ship v1.1 patch.',
				created_at: '2026-11-21T04:50:00Z',
				actions: ['approve', 'deny'],
				state: 'open'
			}
		];

		const v11 = answerExecutiveQuestion(
			'Explain the risk, impact, and recommendation for: Approve release v1.1',
			{
				snapshot: snap,
				items: [],
				offices,
				ownerId: 'you',
				domain: { inboxItems },
				decisionId: 'rel-v1-1'
			}
		);
		expect(v11.headline).toContain('Approve release v1.1');
		expect(v11.bullets[0].text).toContain('Ship v1.1 patch.');

		const v1 = answerExecutiveQuestion(
			'Explain the risk, impact, and recommendation for: Approve release v1',
			{
				snapshot: snap,
				items: [],
				offices,
				ownerId: 'you',
				domain: { inboxItems },
				decisionId: 'rel-v1'
			}
		);
		expect(v1.headline).toContain('Approve release v1 —');
		expect(v1.bullets[0].text).toContain('Ship v1 base.');
	});

	it('filters upcoming milestones strictly to 0..14 days and marks overdue incomplete projects as Overdue', () => {
		const snap = sampleOrg('hq');
		const nowMs = Date.parse('2026-11-25T12:00:00Z');
		const homeProjects = [
			{
				id: 'atlas',
				name: 'Atlas',
				done: 2,
				total: 6,
				open: 4,
				blocked: 0,
				due: '2026-11-23', // -2 days (overdue)
				lead: { id: 'ada', name: 'Ada' },
				members: 3
			},
			{
				id: 'beacon',
				name: 'Beacon',
				done: 1,
				total: 4,
				open: 3,
				blocked: 0,
				due: '2026-11-25', // 0 days (today)
				lead: { id: 'sam', name: 'Sam' },
				members: 2
			},
			{
				id: 'harbor',
				name: 'Harbor',
				done: 1,
				total: 2,
				open: 1,
				blocked: 0,
				due: '2026-12-09', // +14 days (boundary inclusive)
				lead: { id: 'ada', name: 'Ada' },
				members: 2
			},
			{
				id: 'far-out',
				name: 'Far Out',
				done: 0,
				total: 2,
				open: 2,
				blocked: 0,
				due: '2026-12-22', // +27 days (excluded)
				lead: { id: 'sam', name: 'Sam' },
				members: 1
			}
		];

		const milestones = deriveUpcomingMilestones(snap, [], nowMs, { homeProjects });
		expect(milestones.map((m) => m.title)).toEqual(['Beacon', 'Harbor']);
		expect(milestones[0].statusLabel).toBe('Today');
		expect(milestones[1].statusLabel).toBe('In 14 days');

		const cards = deriveProjectCards(snap, [], { homeProjects }, nowMs);
		const atlas = cards.find((c) => c.id === 'atlas');
		expect(atlas?.paceLabel).toBe('Overdue');
		expect(atlas?.paceTone).toBe('bad');
		expect(atlas?.isClear).toBe(false);
		expect(atlas?.blockerText).toBe('Past due date · 4 open');
		expect(atlas?.blockerText).not.toContain('on schedule');

		const kpisWithBudget = deriveKpiMetrics(snap, [], offices, 'you', {
			orgCost: sampleOrgCost,
			agentBudgets: sampleBudgets
		});
		expect(kpisWithBudget.budgetRemainingPct).toBe(21);

		const kpisNoBudget = deriveKpiMetrics(snap, [], offices, 'you');
		expect(kpisNoBudget.budgetRemainingPct).toBeNull();
	});

	it('marks 199/200 and 198/200 past-due projects as Overdue without claiming on schedule', () => {
		const base = sampleOrg('hq');
		const snap = {
			...base,
			projects: (base.projects ?? []).filter((p) => p.id === 'atlas' || p.id === 'beacon')
		};
		const nowMs = Date.parse('2026-11-25T12:00:00Z');
		const domain = {
			homeProjects: [
				{
					id: 'atlas',
					name: 'Atlas',
					done: 199,
					total: 200,
					open: 1,
					blocked: 0,
					due: '2026-11-24',
					lead: { id: 'ada', name: 'Ada' },
					members: 3
				},
				{
					id: 'beacon',
					name: 'Beacon',
					done: 198,
					total: 200,
					open: 2,
					blocked: 0,
					due: '2026-11-24',
					lead: { id: 'sam', name: 'Sam' },
					members: 2
				}
			]
		};
		const cards = deriveProjectCards(snap, [], domain, nowMs);

		const atlas = cards.find((c) => c.id === 'atlas');
		expect(atlas?.pct).toBe(100);
		expect(atlas?.paceLabel).toBe('Overdue');
		expect(atlas?.paceTone).toBe('bad');
		expect(atlas?.health).toBe('At risk');
		expect(atlas?.healthTone).toBe('warn');
		expect(atlas?.isClear).toBe(false);
		expect(atlas?.clearNote).toBe('Past due date · 1 open');
		expect(atlas?.blockerText).toBe('Past due date · 1 open');

		const beacon = cards.find((c) => c.id === 'beacon');
		expect(beacon?.paceLabel).toBe('Overdue');
		expect(beacon?.paceTone).toBe('bad');
		expect(beacon?.health).toBe('At risk');
		expect(beacon?.healthTone).toBe('warn');
		expect(beacon?.isClear).toBe(false);
		expect(beacon?.clearNote).toBe('Past due date · 2 open');
		expect(beacon?.blockerText).toBe('Past due date · 2 open');

		const kpis = deriveKpiMetrics(snap, [], offices, 'you', domain, nowMs);
		expect(kpis.projectsAttention).toBe(2);
		expect(kpis.atRiskProjects).toBe(2);

		const onTimeKpis = deriveKpiMetrics(
			snap,
			[],
			offices,
			'you',
			{
				homeProjects: domain.homeProjects.map((p) => ({ ...p, due: '2026-12-10' }))
			},
			nowMs
		);
		expect(onTimeKpis.projectsAttention).toBe(0);

		// A 0/0 past-due project has no incomplete tracked items (doneCount < totalCount is false)
		const zeroDomain = {
			homeProjects: [
				{
					id: 'atlas',
					name: 'Atlas',
					done: 0,
					total: 0,
					open: 0,
					blocked: 0,
					due: '2026-11-24',
					lead: { id: 'ada', name: 'Ada' },
					members: 3
				}
			]
		};
		const zeroCards = deriveProjectCards(
			{ ...snap, projects: snap.projects.filter((p) => p.id === 'atlas') },
			[],
			zeroDomain,
			nowMs
		);
		expect(zeroCards[0]?.paceLabel).toBeUndefined();
		expect(zeroCards[0]?.health).toBe('On track');
		expect(
			deriveKpiMetrics(
				{ ...snap, projects: snap.projects.filter((p) => p.id === 'atlas') },
				[],
				offices,
				'you',
				zeroDomain,
				nowMs
			).projectsAttention
		).toBe(0);
	});

	it('routes blocked task and bug spotlight details to project work rather than unbuilt /work/<ref>', () => {
		const snap = sampleOrg('hq');
		const blockedTask = makeItem({
			id: 'w-blocked-1',
			short_id: 'T-77',
			kind: 'task',
			title: 'Fix release gate',
			status: 'blocked',
			project_id: 'atlas',
			assignee_id: 'you'
		});
		const decisions = deriveDecisionSpotlight([blockedTask], snap, 'you');
		expect(decisions[0]?.detailHref).toBe('/projects/atlas/work');
		expect(decisions[0]?.detailHref).not.toMatch(/^\/work\/T-/);

		const ans = answerExecutiveQuestion(
			'Explain the risk, impact, and recommendation for: Fix release gate',
			{
				snapshot: snap,
				items: [blockedTask],
				offices,
				ownerId: 'you',
				decisionId: 'w-blocked-1'
			}
		);
		expect(ans.bullets[0].href).toBe('/projects/atlas/work');

		// Inbox-backed decisions whose open.path targets unbuilt /work/<ref> also fall back to /inbox?item=<id>
		const inboxDecisions = deriveDecisionSpotlight([], snap, 'you', Date.now(), [
			{
				id: 'in-1',
				kind: 'approval',
				source: 'Release sign-off',
				state: 'open',
				title: "Approve publishing Kai's sync update to v1.0",
				why: 'Ready to ship.',
				who: { id: 'ada', name: 'Ada', kind: 'agent' },
				project: { id: 'atlas', name: 'Atlas' },
				created_at: '2026-11-23T09:00:00.000Z',
				actions: ['approve', 'deny'],
				open: { path: '/work/T-121', label: 'Open T-121' }
			}
		]);
		expect(inboxDecisions[0]?.detailHref).toBe('/inbox?item=in-1');
		const inboxAns = answerExecutiveQuestion(
			"Explain the risk, impact, and recommendation for: Approve publishing Kai's sync update to v1.0",
			{
				snapshot: snap,
				items: [],
				offices,
				ownerId: 'you',
				decisionId: 'in-1',
				decision: inboxDecisions[0]
			}
		);
		expect(inboxAns.bullets[0].href).toBe('/inbox?item=in-1');
	});

	it('answers with explicit decision context even when shell snapshot is pending or stale', () => {
		const snap = sampleOrg('hq');
		const freshDecision = {
			id: 'in-shared',
			title: 'Approve production failover (updated)',
			recommendation: 'Ada recommends: approve failover to secondary cluster.',
			risk: { label: 'Moderate risk', sub: '2-minute write pause' },
			impact: { label: 'High impact', sub: 'Restores EU availability' },
			backgroundContext: 'Primary node disk latency crossed threshold.',
			detailHref: '/inbox?item=in-shared',
			projectId: 'atlas',
			projectName: 'Atlas'
		};

		const ansNoSnap = answerExecutiveQuestion(
			'Explain the risk, impact, and recommendation for: Approve production failover (updated)',
			{
				snapshot: undefined,
				items: [],
				offices,
				ownerId: 'you',
				domain: { inboxItems: [] },
				decisionId: 'in-shared',
				decision: freshDecision
			}
		);
		expect(ansNoSnap.headline).toContain('Approve production failover (updated)');
		expect(ansNoSnap.headline).not.toContain('All clear');
		expect(ansNoSnap.bullets[0].text).toContain(
			'Ada recommends: approve failover to secondary cluster.'
		);
		expect(ansNoSnap.bullets[0].href).toBe('/inbox?item=in-shared');

		const ansStaleShell = answerExecutiveQuestion(
			'Explain the risk, impact, and recommendation for: Approve production failover (updated)',
			{
				snapshot: snap,
				items: [],
				offices,
				ownerId: 'you',
				domain: {
					inboxItems: [
						{
							id: 'in-shared',
							kind: 'approval',
							source: 'Release sign-off',
							state: 'open',
							title: 'Stale shell title',
							why: 'Stale reason',
							who: { id: 'ada', name: 'Ada', kind: 'agent' },
							project: { id: 'atlas', name: 'Atlas' },
							created_at: '2026-11-23T09:00:00.000Z',
							actions: ['approve', 'deny']
						}
					]
				},
				decisionId: 'in-shared',
				decision: freshDecision
			}
		);
		expect(ansStaleShell.headline).toContain('Approve production failover (updated)');
		expect(ansStaleShell.headline).not.toContain('Stale shell title');
		expect(ansStaleShell.bullets[0].text).toContain(
			'Ada recommends: approve failover to secondary cluster.'
		);
	});

	it('treats placed active directory agents as active when live presence telemetry is absent', () => {
		const snap = sampleOrg('hq');
		const kpis = deriveKpiMetrics(snap, [], offices, 'you', {
			memberRows: [
				{
					id: 'a1',
					kind: 'agent',
					name: 'Backend Lead',
					handle: 'backend-lead',
					status: 'active',
					owner: false,
					canSignIn: false,
					office: { id: 'hq', name: 'HQ' },
					directReports: 0
				},
				{
					id: 'a2',
					kind: 'agent',
					name: 'Paused Agent',
					handle: 'paused',
					status: 'paused',
					owner: false,
					canSignIn: false,
					office: { id: 'hq', name: 'HQ' },
					directReports: 0
				},
				{
					id: 'a3',
					kind: 'agent',
					name: 'Unplaced Agent',
					handle: 'unplaced',
					status: 'active',
					owner: false,
					canSignIn: false,
					office: null,
					directReports: 0
				}
			]
		});
		expect(kpis.totalAgents).toBe(3);
		expect(kpis.activeAgents).toBe(1);
		expect(kpis.pausedAgents).toBe(1);
		expect(kpis.unplacedAgents).toBe(1);
	});

	it('resolves raw member, project, and team UUIDs in Decision Spotlight from the org snapshot', () => {
		const base = sampleOrg('hq');
		const memberUuid = 'a3b2bb1e-0000-7000-8000-000000000001';
		const projectUuid = '019c0000-0000-7000-8000-000000000002';
		const templateAgent = base.members.find((m) => m.kind === 'agent' && m.agent)!;
		const templateProject = base.projects![0]!;
		const snap = {
			...base,
			members: [
				...base.members,
				{
					...templateAgent,
					id: memberUuid,
					name: 'Main EU Engineer',
					office_id: 'hq'
				}
			],
			projects: [
				...(base.projects ?? []),
				{
					...templateProject,
					id: projectUuid,
					name: 'Agentic Org v1',
					key: 'P-1',
					archived: false
				}
			]
		};
		const spot = deriveDecisionSpotlight([], snap, 'you', Date.parse('2026-11-25T16:00:00Z'), [
			{
				id: 'office-hq:msg:m-1',
				kind: 'question',
				source: 'Chat message',
				title: 'Status check',
				who: { id: memberUuid, name: memberUuid, kind: 'agent' },
				project: { id: projectUuid, name: projectUuid },
				why: 'Detailed status update',
				created_at: '2026-11-25T15:55:00Z',
				actions: ['reply', 'dismiss'],
				state: 'open'
			}
		]);
		expect(spot).toHaveLength(1);
		expect(spot[0].actorName).toBe('Main EU Engineer');
		expect(spot[0].actorInitials).toBe('ME');
		expect(spot[0].projectName).toBe('Agentic Org v1');
	});

	it('answers combined budget, offices & active agents prompts with both spend and team status', () => {
		const snap = sampleOrg('hq');
		const answer = answerExecutiveQuestion('Budget, offices & active agents', {
			snapshot: snap,
			items: [],
			offices,
			ownerId: 'you',
			domain: { orgCost: sampleOrgCost }
		});
		expect(answer.headline).toContain('agents active');
		expect(answer.headline).toContain('$142');
		expect(answer.bullets.some((b) => b.href === '/settings/costs')).toBe(true);
		expect(answer.bullets.some((b) => b.href === '/teams/members')).toBe(true);
		expect(answer.bullets.some((b) => b.href === '/settings/offices')).toBe(true);
	});

	it('orders KPI pillars as Projects -> Delivery -> Team -> Financials and enforces 5-sample median lead floor + signed 7d delta', () => {
		expect(KPI_PILLAR_ORDER).toEqual(['projects', 'delivery', 'team', 'financials']);

		const snap = sampleOrg('hq');
		const nowMs = Date.parse('2026-11-27T12:00:00Z');
		const twoRecent = [
			makeItem({
				id: 'd1',
				status: 'done',
				created_at: '2026-11-26T08:00:00Z',
				updated_at: '2026-11-26T10:00:00Z' // 2h, within 7d
			}),
			makeItem({
				id: 'd2',
				status: 'done',
				created_at: '2026-11-25T06:00:00Z',
				updated_at: '2026-11-25T12:00:00Z' // 6h, within 7d
			})
		];

		// < 5 samples returns null medianCycleHours (honest floor matching /insights), and prior=0 with closedLast7d>0 yields closedPrior7d=0 for signed +N delta
		const smallSample = deriveKpiMetrics(snap, twoRecent, offices, 'you', undefined, nowMs);
		expect(smallSample.closedLast7d).toBe(2);
		expect(smallSample.closedPrior7d).toBe(0);
		expect(smallSample.medianCycleHours).toBeNull();
		expect(smallSample.throughputPoints.reduce((a, b) => a + b, 0)).toBe(2);

		// >= 5 samples computes median lead accurately
		const fiveRecent = [
			...twoRecent,
			makeItem({
				id: 'd3',
				status: 'done',
				created_at: '2026-11-24T08:00:00Z',
				updated_at: '2026-11-24T12:00:00Z' // 4h
			}),
			makeItem({
				id: 'd4',
				status: 'done',
				created_at: '2026-11-23T08:00:00Z',
				updated_at: '2026-11-23T16:00:00Z' // 8h
			}),
			makeItem({
				id: 'd5',
				status: 'done',
				created_at: '2026-11-22T08:00:00Z',
				updated_at: '2026-11-22T18:00:00Z' // 10h
			}),
			makeItem({
				id: 'd6',
				status: 'done',
				created_at: '2026-11-16T08:00:00Z',
				updated_at: '2026-11-16T12:00:00Z' // 11d ago -> prior 7d
			})
		];
		const fullSample = deriveKpiMetrics(snap, fiveRecent, offices, 'you', undefined, nowMs);
		expect(fullSample.closedLast7d).toBe(5);
		expect(fullSample.closedPrior7d).toBe(1);
		expect(fullSample.medianCycleHours).toBe(6);

		expect(formatCycleDuration(null)).toBe('—');
		expect(formatCycleDuration(0.3)).toBe('18m');
		expect(formatCycleDuration(4)).toBe('4h');
		expect(formatCycleDuration(72)).toBe('3d');
	});

	it('populates leadId on derived project cards for inline member links', () => {
		const snap = sampleOrg('hq');
		const projectId = snap.projects?.[0]?.id ?? 'proj-1';
		const cards = deriveProjectCards(snap, [], {
			homeProjects: [
				{
					id: projectId,
					name: 'Agentic Org',
					done: 3,
					total: 5,
					open: 2,
					blocked: 0,
					members: 3,
					lead: { id: 'ada-id', name: 'Ada' }
				}
			]
		});
		const match = cards.find((c) => c.id === projectId);
		expect(match?.leadId).toBe('ada-id');
		expect(match?.leadName).toBe('Ada');
	});

	it('keeps throughputPoints all zeros when only historical closures exist (> 7 days ago)', () => {
		const snap = sampleOrg('hq');
		const nowMs = Date.parse('2026-11-27T12:00:00Z');
		const historicalItems = [
			makeItem({
				id: 'old-1',
				status: 'done',
				created_at: '2026-11-01T08:00:00Z',
				updated_at: '2026-11-02T12:00:00Z'
			}),
			makeItem({
				id: 'old-2',
				status: 'done',
				created_at: '2026-11-05T08:00:00Z',
				updated_at: '2026-11-06T12:00:00Z'
			})
		];

		const kpis = deriveKpiMetrics(snap, historicalItems, offices, 'you', undefined, nowMs);
		expect(kpis.doneCount).toBe(2);
		expect(kpis.closedLast7d).toBe(0);
		expect(kpis.throughputPoints).toEqual([0, 0, 0, 0, 0, 0, 0]);
	});

	it('forwards explicit nowMs deterministically inside deriveExecutiveBrief', () => {
		const snap = sampleOrg('hq');
		const firstProject = snap.projects![0];
		const onlineOffices: OfficeInfo[] = [{ ...offices[0], status: 'active' }];
		const domain = {
			homeProjects: [
				{
					id: firstProject.id,
					name: firstProject.name,
					done: 1,
					total: 4,
					open: 3,
					blocked: 0,
					members: 2,
					due: '2026-11-25'
				}
			]
		};

		const beforeDue = deriveExecutiveBrief(
			snap,
			[],
			onlineOffices,
			'you',
			'Acme',
			Date.parse('2026-11-24T12:00:00Z'),
			domain
		);
		const afterDue = deriveExecutiveBrief(
			snap,
			[],
			onlineOffices,
			'you',
			'Acme',
			Date.parse('2026-11-27T12:00:00Z'),
			domain
		);

		expect(beforeDue.lenses.today.sections[1].body).not.toContain(`${firstProject.name} (At risk)`);
		expect(afterDue.lenses.today.sections[1].body).toContain(`${firstProject.name} (At risk)`);
	});
});
