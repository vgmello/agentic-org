import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import type { OfficeInfo, WorkItemSummary } from '$lib/proto';
import type { AgentBudgets, OrganizationCost } from '$lib/modules/costs/types';
import { sampleOrg } from '$lib/modules/organization/sample';
import {
	deriveDecisionSpotlight,
	deriveExecutiveBrief,
	deriveKpiMetrics,
	deriveProjectCards,
	deriveTaskTimeBreakdown
} from '$lib/modules/home/model';
import CapacityBreakdown from './CapacityBreakdown.svelte';
import ExecutiveBriefCard from './ExecutiveBriefCard.svelte';
import HomeDayOne from './HomeDayOne.svelte';
import HomeKpiStrip from './HomeKpiStrip.svelte';
import HomeSkeleton from './HomeSkeleton.svelte';
import HomeThroughputSparkline from './HomeThroughputSparkline.svelte';
import ProjectScorecard from './ProjectScorecard.svelte';
import SpotlightActionBar from './SpotlightActionBar.svelte';
import SpotlightHeaderBar from './SpotlightHeaderBar.svelte';

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

const orgCost: OrganizationCost = {
	period: { kind: 'this_month', endsAt: '2026-11-27T12:00:00Z', timeZone: 'UTC', toNow: true },
	totals: {
		records: 18,
		knownCostMicroUsd: 142_000_000,
		knownTokens: 10_000_000,
		cacheReadTokens: 2_000_000,
		tokenIncompleteRecords: 0,
		costIncompleteRecords: 0,
		unpricedRecords: 0,
		unknownCostRecords: 0,
		includesApiEquivalent: false
	},
	byProject: [],
	byConsumer: [],
	byWorkType: [],
	coverage: { checkpointAt: '2026-11-27T11:55:00Z', gaps: [] }
};

const agentBudgets: AgentBudgets = {
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

function makeWorkItem(overrides: Partial<WorkItemSummary>): WorkItemSummary {
	return {
		id: 'item-1',
		kind: 'task',
		short_id: 'T-1',
		title: 'Ship sync update',
		status: 'in_progress',
		created_by: 'sam',
		created_at: '2026-11-26T08:00:00Z',
		updated_at: '2026-11-26T12:00:00Z',
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

describe('Home Prototype Parity (TASK-262)', () => {
	it('renders HomeKpiStrip pillars in Projects -> Delivery -> Team -> Financials order with live financials and bar sparkline', () => {
		const snap = sampleOrg('hq');
		const nowMs = Date.parse('2026-11-27T12:00:00Z');
		const items = [
			makeWorkItem({ id: 'w1', status: 'done' }),
			makeWorkItem({ id: 'w2', status: 'blocked' })
		];
		const kpis = deriveKpiMetrics(snap, items, offices, 'you', { orgCost, agentBudgets }, nowMs);
		const { body } = render(HomeKpiStrip, { props: { kpis } });

		const projectsIdx = body.indexOf('id="hb-pil-projects"');
		const deliveryIdx = body.indexOf('id="hb-pil-proj"');
		const teamIdx = body.indexOf('id="hb-pil-team"');
		const finIdx = body.indexOf('id="hb-pil-fin"');
		expect(projectsIdx).toBeGreaterThan(-1);
		expect(projectsIdx).toBeLessThan(deliveryIdx);
		expect(deliveryIdx).toBeLessThan(teamIdx);
		expect(teamIdx).toBeLessThan(finIdx);

		// Delivery bar sparkline + real sub-metrics linking to /insights
		expect(body).toContain('data-testid="home-throughput-sparkline"');
		expect(body).toContain('Median cycle');
		expect(body).toContain('Closed · 7d');
		expect(body).toContain('href="/insights"');

		// Team sub-metrics: agents count + compact N on · M off
		expect(body).toContain('1 on');
		expect(body).toContain('1 off');

		// Financials uses real API values ($142 / $180 monthly), never hardcoded $210 or 74% AI capacity free
		expect(body).toContain('$142');
		expect(body).toContain('/ $180 monthly');
		expect(body).not.toContain('$210');
		expect(body).not.toContain('74%');
	});

	it('renders interactive queue dot buttons with aria-current and accessible labels in SpotlightHeaderBar', () => {
		const snap = sampleOrg('hq');
		const spotlight = deriveDecisionSpotlight([], snap, 'you', Date.parse('2026-11-27T12:00:00Z'), [
			{
				id: 'in-1',
				kind: 'approval',
				source: 'Run approval',
				title: 'Publish release v1.0',
				who: { id: 'kai', name: 'Kai', kind: 'agent' },
				why: 'All checks green.',
				created_at: '2026-11-27T10:00:00Z',
				expires_at: '2026-11-27T16:00:00Z',
				actions: ['approve', 'deny'],
				state: 'open'
			},
			{
				id: 'in-2',
				kind: 'question',
				source: 'Chat ask',
				title: 'Confirm rollout window',
				who: { id: 'ada', name: 'Ada', kind: 'agent' },
				why: 'Need confirmation on cutover hour.',
				created_at: '2026-11-27T11:00:00Z',
				actions: ['reply'],
				state: 'open'
			}
		]);

		const { body } = render(SpotlightHeaderBar, {
			props: {
				currentDecision: spotlight[0],
				spotlight,
				safeIdx: 0,
				onSelect: () => {}
			}
		});

		expect(body).toContain('role="group"');
		expect(body).toContain('aria-label="Priority queue"');
		expect(body).toContain('aria-label="Decision 1 of 2: Publish release v1.0"');
		expect(body).toContain('aria-current="true"');
		expect(body).toContain('aria-label="Decision 2 of 2: Confirm rollout window"');
		expect(body).toContain('aria-label="Previous decision"');
		expect(body).toContain('aria-label="Next decision"');
	});

	it('supports inline question reply box, handled receipts, and consequential Review in Inbox routing in SpotlightActionBar', () => {
		const snap = sampleOrg('hq');
		const decisions = deriveDecisionSpotlight([], snap, 'you', Date.parse('2026-11-27T12:00:00Z'), [
			{
				id: 'q-1',
				kind: 'question',
				source: 'Chat ask',
				title: 'Which region first?',
				who: { id: 'ada', name: 'Ada', kind: 'agent' },
				why: 'Staging is ready in both EU and US.',
				created_at: '2026-11-27T11:00:00Z',
				actions: ['reply'],
				state: 'open'
			},
			{
				id: 'c-1',
				kind: 'approval',
				source: 'Policy gate',
				title: 'Promote production signing key',
				who: { id: 'sage', name: 'Sage', kind: 'agent' },
				why: 'Requires full key audit in Inbox.',
				created_at: '2026-11-27T11:30:00Z',
				actions: [],
				open: { path: '/inbox?item=c-1', label: 'Requires full audit in Inbox' },
				state: 'open'
			}
		]);
		const question = decisions.find((d) => d.id === 'q-1')!;
		const consequential = decisions.find((d) => d.id === 'c-1')!;

		const noop = () => {};
		const baseProps = {
			totalCount: 2,
			busy: false,
			declining: false,
			declineReason: '',
			replying: false,
			replyText: '',
			restoreDenyFocus: false,
			onClearRestoreDenyFocus: noop,
			onPrimary: noop,
			onSecondary: noop,
			onArmDeny: noop,
			onCancelDeny: noop,
			onOpenReply: noop,
			onSubmitReply: noop,
			onCancelReply: noop
		};

		// Question offers Answer… button
		const qDefault = render(SpotlightActionBar, {
			props: { ...baseProps, decision: question }
		});
		expect(qDefault.body).toContain('Answer…');

		// Replying mode renders inline SpotlightReplyBox
		const qReplying = render(SpotlightActionBar, {
			props: { ...baseProps, decision: question, replying: true, replyText: 'Start with EU' }
		});
		expect(qReplying.body).toContain('Send answer');
		expect(qReplying.body).toContain('Answer Ada');

		// Handled receipt shows outcome pill + quoted response note
		const qReceipt = render(SpotlightActionBar, {
			props: {
				...baseProps,
				decision: question,
				receipt: { outcome: 'Answered', note: 'Start with EU' }
			}
		});
		expect(qReceipt.body).toContain('Answered');
		expect(qReceipt.body).toContain('Start with EU');
		expect(qReceipt.body).not.toContain('Send answer');

		// Consequential inbox-only item routes via Review in Inbox without redundant Details link
		const cRoute = render(SpotlightActionBar, {
			props: { ...baseProps, decision: consequential }
		});
		expect(cRoute.body).toContain('Review in Inbox');
		expect(cRoute.body).toContain('Requires full audit in Inbox');
		expect(cRoute.body).not.toContain('>Details<');
	});

	it('renders ProjectScorecard with shared SectionHead, inline Lead link, and no avatar footer stack', () => {
		const snap = sampleOrg('hq');
		const projectId = snap.projects?.[0]?.id ?? 'proj-1';
		const cards = deriveProjectCards(snap, [], {
			homeProjects: [
				{
					id: projectId,
					name: 'Agentic Org',
					done: 4,
					total: 5,
					open: 1,
					blocked: 0,
					members: 4,
					lead: { id: 'ada', name: 'Ada' }
				}
			]
		});
		const { body } = render(ProjectScorecard, {
			props: { projects: cards, attentionCount: 0 }
		});

		expect(body).toContain('id="exec-projects-heading"');
		expect(body).toContain('All projects');
		expect(body).toContain('Lead');
		expect(body).toContain('href="/members/ada"');
		expect(body).not.toContain('proj-foot');
		expect(body).not.toContain('avatar-stack');
	});

	it('renders CapacityBreakdown with outside SectionHead, Insights link, and accessible linked bar segments', () => {
		const taskTime = deriveTaskTimeBreakdown(
			[
				makeWorkItem({ id: 't1', status: 'in_progress' }),
				makeWorkItem({ id: 't2', status: 'todo' }),
				makeWorkItem({ id: 't3', status: 'todo' })
			],
			'you',
			1
		);
		const { body } = render(CapacityBreakdown, {
			props: { taskTime, offlineOffices: 1 }
		});

		expect(body).toContain('Where task time goes');
		expect(body).toContain('href="/insights"');
		expect(body).toContain('role="group"');
		expect(body).toContain('aria-label="Agents working: 33% (1 task)"');
		expect(body).toContain('aria-label="Waiting for an agent: 67% (2 tasks)"');
	});

	it('renders ExecutiveBriefCard with scannable icon highlights, Show more label, and no internal card header', () => {
		const snap = sampleOrg('hq');
		const brief = deriveExecutiveBrief(snap, [], offices, 'you', 'Agentic Org');
		const { body } = render(ExecutiveBriefCard, {
			props: { brief, catchUp: [], lens: 'today', ariaLabelledby: 'hb-brief-h' }
		});

		expect(body).toContain('aria-labelledby="hb-brief-h"');
		expect(body).toContain('dg-list');
		expect(body).toContain('Show more');
		expect(body).not.toContain('Show full brief');
		expect(body).not.toContain('Executive Brief');

		// Lead sentence is never duplicated verbatim inside the highlight list
		const leadMatch = body.match(/<p class="brief-lead[^"]*">([^<]+)<\/p>/);
		expect(leadMatch).toBeTruthy();
		const leadSentence = leadMatch![1].trim();
		const listSection = body.slice(body.indexOf('<ul'));
		expect(listSection).not.toContain(leadSentence);

		// Single-section brief renders cleanly without an empty <ul> or duplicated sentence
		const singleBrief = {
			...brief,
			lenses: {
				...brief.lenses,
				today: {
					...brief.lenses.today,
					sections: [
						{
							id: 'solo',
							kicker: 'Live Status',
							tone: 'ok' as const,
							body: 'Everything is tracking smoothly today.'
						}
					]
				}
			}
		};
		const soloRender = render(ExecutiveBriefCard, {
			props: { brief: singleBrief, catchUp: [], lens: 'today' }
		});
		expect(soloRender.body).toContain('Live Status');
		expect(soloRender.body.match(/Everything is tracking smoothly today\./g)).toHaveLength(1);
		expect(soloRender.body).not.toContain('dg-list');
	});

	it('renders a sliding window in SpotlightHeaderBar so active dot is visible when spotlight.length > 6', () => {
		const snap = sampleOrg('hq');
		const eightItems = Array.from({ length: 8 }, (_, idx) => ({
			id: `in-${idx + 1}`,
			kind: 'question' as const,
			source: 'Chat ask',
			title: `Decision item ${idx + 1}`,
			who: { id: 'ada', name: 'Ada', kind: 'agent' as const },
			why: 'Needs input.',
			created_at: `2026-11-27T1${idx}:00:00Z`,
			actions: ['reply' as const],
			state: 'open' as const
		}));
		const spotlight = deriveDecisionSpotlight(
			[],
			snap,
			'you',
			Date.parse('2026-11-27T20:00:00Z'),
			eightItems
		);
		expect(spotlight).toHaveLength(8);

		const { body } = render(SpotlightHeaderBar, {
			props: {
				currentDecision: spotlight[7],
				spotlight,
				safeIdx: 7,
				onSelect: () => {}
			}
		});

		expect(body).toContain(`aria-label="Decision 8 of 8: ${spotlight[7].title}"`);
		expect(body).toContain('aria-current="true"');
		expect(body).toContain('Priority 8 of 8');
	});

	it('labels non-inbox consequential routes accurately in SpotlightActionBar and renders 4 cards in HomeSkeleton', () => {
		const snap = sampleOrg('hq');
		const [chatDecision, projectDecision] = deriveDecisionSpotlight(
			[],
			snap,
			'you',
			Date.parse('2026-11-27T12:00:00Z'),
			[
				{
					id: 'chat-1',
					kind: 'approval',
					source: 'Chat approval',
					title: 'Confirm channel policy',
					who: { id: 'ada', name: 'Ada', kind: 'agent' },
					why: 'Review thread context.',
					created_at: '2026-11-27T11:30:00Z',
					actions: [],
					open: { path: '/chat/general', label: 'Open conversation' },
					state: 'open'
				},
				{
					id: 'proj-1',
					kind: 'approval',
					source: 'Release gate',
					title: 'Review release checklist',
					who: { id: 'kai', name: 'Kai', kind: 'agent' },
					why: 'Check project release details.',
					created_at: '2026-11-27T11:00:00Z',
					actions: [],
					open: { path: '/projects/p1/releases', label: 'Inspect release' },
					state: 'open'
				}
			]
		);

		const noop = () => {};
		const baseProps = {
			totalCount: 2,
			busy: false,
			declining: false,
			declineReason: '',
			replying: false,
			replyText: '',
			restoreDenyFocus: false,
			onClearRestoreDenyFocus: noop,
			onPrimary: noop,
			onSecondary: noop,
			onArmDeny: noop,
			onCancelDeny: noop,
			onOpenReply: noop,
			onSubmitReply: noop,
			onCancelReply: noop
		};

		const chatRender = render(SpotlightActionBar, {
			props: { ...baseProps, decision: chatDecision }
		});
		expect(chatRender.body).toContain('Open in Chat');
		expect(chatRender.body).not.toContain('Review in Inbox');

		const projRender = render(SpotlightActionBar, {
			props: { ...baseProps, decision: projectDecision }
		});
		expect(projRender.body).toContain('Review details');
		expect(projRender.body).not.toContain('Review in Inbox');

		const skelRender = render(HomeSkeleton);
		expect(skelRender.body).toContain('xl:grid-cols-4');
		expect(skelRender.body.match(/class="exec-card/g)).toHaveLength(4);

		const emptySpark = render(HomeThroughputSparkline, {
			props: { points: [0, 0, 0, 0, 0, 0, 0] }
		});
		expect(emptySpark.body).not.toContain('home-throughput-sparkline');
	});

	it('renders HomeDayOne empty-org CTAs and cleared-in-session receipt state', () => {
		const dayOne = render(HomeDayOne, { props: { isDayOne: true } });
		expect(dayOne.body).toContain('Launch your organization');
		expect(dayOne.body).toContain('Create first project');
		expect(dayOne.body).toContain('Hire first agent');

		const cleared = render(HomeDayOne, {
			props: {
				isDayOne: false,
				sessionHandledCount: 2,
				lastReceipt: { title: 'Confirm rollout', outcome: 'Answered', note: 'Proceed at 14:00 UTC' }
			}
		});
		expect(cleared.body).toContain('Priority spotlight cleared');
		expect(cleared.body).toContain('Answered — Confirm rollout');
		expect(cleared.body).toContain('Proceed at 14:00 UTC');
	});
});
