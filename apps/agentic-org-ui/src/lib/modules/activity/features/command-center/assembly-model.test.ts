import { describe, expect, it } from 'vitest';
import type { ActivityRow } from '$lib/modules/activity/types';
import type { InboxItem } from '$lib/modules/inbox/types';
import type { ItemStatus, OrgSnapshot, WorkItemSummary } from '$lib/proto';
import {
	classifyAssemblyStage,
	computeBottleneck,
	computeCompletionPct,
	computeThroughput7d,
	deriveActivityCommandCenter,
	deriveDecisionStreamEntries,
	extractInboxWorkRefs,
	filterStreamEntries,
	formatTelemetryHours,
	mergeLiveActivityRows
} from './assembly-model';

const NOW = Date.parse('2026-11-23T12:00:00.000Z');
const DAY = 86_400_000;

function makeItem(
	overrides: Partial<WorkItemSummary> & Pick<WorkItemSummary, 'id' | 'short_id' | 'kind' | 'status'>
): WorkItemSummary {
	return {
		number: Number(overrides.short_id.split('-')[1] ?? 1),
		title: `Work item ${overrides.short_id}`,
		created_by: 'ada',
		created_at: new Date(NOW - 2 * DAY).toISOString(),
		updated_at: new Date(NOW - 3_600_000).toISOString(),
		severity_confirmed: overrides.kind === 'bug',
		fixes: [],
		fixed_by: [],
		checklist: { total: 2, done: 1, required_left: 1 },
		done_anyway: false,
		owner_office_id: 'hq',
		generation: 1,
		reopen_count: 0,
		version: 1,
		deleted: false,
		...overrides
	};
}

function makeInbox(overrides: Partial<InboxItem> & Pick<InboxItem, 'id' | 'title'>): InboxItem {
	return {
		kind: 'approval',
		source: 'Release sign-off',
		who: { id: 'ada', name: 'Ada', kind: 'agent' },
		why: 'Needs approval',
		created_at: new Date(NOW - 1_800_000).toISOString(),
		actions: ['approve', 'deny'],
		state: 'open',
		...overrides
	};
}

function makeSnapshot(): OrgSnapshot {
	return {
		position: { epoch: 1, seq: 10 },
		teams: [{ id: 'platform', name: 'Platform', version: 1 }],
		members: [
			{ id: 'you', name: 'You', kind: 'person', owner: true, version: 1 },
			{
				id: 'ada',
				name: 'Ada Lovelace',
				kind: 'agent',
				agent: {
					role: 'em',
					title: 'Engineering Manager',
					runtime: 'claude_code',
					messaging_name: 'ada',
					status: 'active'
				},
				version: 1
			},
			{
				id: 'kai',
				name: 'Kai Sato',
				kind: 'agent',
				agent: {
					role: 'rust',
					title: 'Rust Developer',
					runtime: 'codex',
					messaging_name: 'kai',
					status: 'active'
				},
				version: 1
			},
			{
				id: 'avery',
				name: 'Avery Cho',
				kind: 'agent',
				agent: {
					role: 'ui',
					title: 'UI Developer',
					runtime: 'codex',
					messaging_name: 'avery',
					status: 'paused'
				},
				version: 1
			},
			{
				id: 'former',
				name: 'Former Agent',
				kind: 'agent',
				agent: {
					role: 'old',
					title: 'Retired',
					runtime: 'codex',
					messaging_name: 'former',
					status: 'offboarded',
					offboarded_at: new Date(NOW - 10 * DAY).toISOString()
				},
				version: 1
			}
		],
		projects: [
			{ id: 'proj-b', name: 'Beta Release', key: 'v2.0', version: 1 },
			{ id: 'proj-a', name: 'Alpha Core', key: 'v1.0', version: 1 },
			{ id: 'proj-old', name: 'Archived Legacy', key: 'v0.1', archived: true, version: 1 }
		],
		areas: [
			{ id: 'area-api', project_id: 'proj-a', name: 'API', version: 1 },
			{ id: 'area-ui', project_id: 'proj-a', name: 'Interface', version: 1 },
			{ id: 'area-b-gen', project_id: 'proj-b', name: 'General', general: true, version: 1 }
		]
	};
}

describe('assembly-model', () => {
	it('classifies stages across tasks, bugs, requests (merged & declined), cancelled tasks, checklist-ready items, and open inbox references', () => {
		const inboxItem = makeInbox({
			id: 'in-1',
			title: 'Approve T-102 before deploy',
			open: { path: '/work/T-102', label: 'Open T-102' }
		});
		const inboxMap = new Map<string, InboxItem>([['T-102', inboxItem]]);

		expect(
			classifyAssemblyStage(
				makeItem({ id: '1', short_id: 'T-101', kind: 'task', status: 'done' }),
				inboxMap
			)
		).toBe('merged');
		expect(
			classifyAssemblyStage(
				makeItem({ id: '2', short_id: 'BUG-1', kind: 'bug', status: 'closed' }),
				inboxMap
			)
		).toBe('merged');
		expect(
			classifyAssemblyStage(
				makeItem({ id: '3', short_id: 'R-1', kind: 'request', status: 'approved' }),
				inboxMap
			)
		).toBe('merged');
		expect(
			classifyAssemblyStage(
				makeItem({ id: '3b', short_id: 'R-1B', kind: 'request', status: 'merged' }),
				inboxMap
			)
		).toBe('merged');

		// Terminal non-merged items must be excluded (null), never dumped into Queue
		expect(
			classifyAssemblyStage(
				makeItem({ id: '3c', short_id: 'R-DEC', kind: 'request', status: 'declined' }),
				inboxMap
			)
		).toBeNull();
		expect(
			classifyAssemblyStage(
				makeItem({
					id: '3d',
					short_id: 'T-CAN1',
					kind: 'task',
					status: 'done',
					resolution: 'cancelled'
				}),
				inboxMap
			)
		).toBeNull();
		expect(
			classifyAssemblyStage(
				makeItem({ id: '3e', short_id: 'T-CAN2', kind: 'task', status: 'cancelled' as ItemStatus }),
				inboxMap
			)
		).toBeNull();

		// Open inbox overrides in_progress into review
		expect(
			classifyAssemblyStage(
				makeItem({ id: '4', short_id: 'T-102', kind: 'task', status: 'in_progress' }),
				inboxMap,
				'you'
			)
		).toBe('review');
		// Blocked task enters review only when assigned to the owner (otherwise stays in queue unless an open Inbox item references it)
		expect(
			classifyAssemblyStage(
				makeItem({
					id: '5',
					short_id: 'T-103',
					kind: 'task',
					status: 'blocked',
					assignee_id: 'you'
				}),
				inboxMap,
				'you'
			)
		).toBe('review');
		expect(
			classifyAssemblyStage(
				makeItem({
					id: '5b',
					short_id: 'T-103B',
					kind: 'task',
					status: 'blocked',
					assignee_id: 'kai'
				}),
				inboxMap,
				'you'
			)
		).toBe('queue');
		expect(
			classifyAssemblyStage(
				makeItem({ id: '6', short_id: 'BUG-2', kind: 'bug', status: 'fixed' }),
				inboxMap,
				'you'
			)
		).toBe('review');
		expect(
			classifyAssemblyStage(
				makeItem({ id: '7', short_id: 'R-2', kind: 'request', status: 'scoped' }),
				inboxMap,
				'you'
			)
		).toBe('review');

		// Checks-ready stage requires in_progress + non-empty fully completed checklist with 0 required_left
		expect(
			classifyAssemblyStage(
				makeItem({
					id: '8',
					short_id: 'T-104',
					kind: 'task',
					status: 'in_progress',
					checklist: { total: 3, done: 3, required_left: 0 }
				}),
				inboxMap
			)
		).toBe('ci');

		// Agents stage when in_progress with incomplete checklist
		expect(
			classifyAssemblyStage(
				makeItem({
					id: '9',
					short_id: 'T-105',
					kind: 'task',
					status: 'in_progress',
					checklist: { total: 3, done: 2, required_left: 1 }
				}),
				inboxMap
			)
		).toBe('agents');

		// Queue stage for todo / new / triaged / scoping
		expect(
			classifyAssemblyStage(
				makeItem({ id: '10', short_id: 'T-106', kind: 'task', status: 'todo' }),
				inboxMap
			)
		).toBe('queue');
		expect(
			classifyAssemblyStage(
				makeItem({ id: '11', short_id: 'BUG-3', kind: 'bug', status: 'triaged' }),
				inboxMap
			)
		).toBe('queue');
		expect(
			classifyAssemblyStage(
				makeItem({ id: '12', short_id: 'R-3', kind: 'request', status: 'scoping' }),
				inboxMap
			)
		).toBe('queue');
	});

	it('excludes declined requests and cancelled tasks from active pipeline counts while counting merged requests as done', () => {
		const snapshot = makeSnapshot();
		const workItems: WorkItemSummary[] = [
			makeItem({
				id: 'r-m',
				short_id: 'R-10',
				kind: 'request',
				status: 'merged',
				project_id: 'proj-b'
			}),
			makeItem({
				id: 'r-d',
				short_id: 'R-11',
				kind: 'request',
				status: 'declined',
				project_id: 'proj-b'
			}),
			makeItem({
				id: 't-c1',
				short_id: 'T-12',
				kind: 'task',
				status: 'done',
				resolution: 'cancelled',
				project_id: 'proj-b'
			}),
			makeItem({
				id: 't-c2',
				short_id: 'T-13',
				kind: 'task',
				status: 'cancelled' as ItemStatus,
				project_id: 'proj-b'
			}),
			makeItem({ id: 't-q', short_id: 'T-14', kind: 'task', status: 'todo', project_id: 'proj-b' })
		];

		const model = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems,
			inboxItems: [],
			activityRows: [],
			projectScope: 'proj-b',
			nowMs: NOW
		});

		expect(model.totalItems).toBe(2);
		expect(model.mergedItems).toBe(1);
		expect(model.openItems).toBe(1);
		expect(model.stageCounts).toEqual({ queue: 1, agents: 0, ci: 0, review: 0, merged: 1 });
	});

	it('always labels checklist-complete items honestly as "ready for checks" even when agent turns are running', () => {
		const snapshot = makeSnapshot();
		const item1 = makeItem({
			id: 'w-ci-1',
			short_id: 'T-200',
			kind: 'task',
			status: 'in_progress',
			project_id: 'proj-b',
			assignee_id: 'kai',
			checklist: { total: 2, done: 2, required_left: 0 }
		});
		const item2 = makeItem({
			id: 'w-ci-2',
			short_id: 'T-201',
			kind: 'task',
			status: 'in_progress',
			project_id: 'proj-b',
			assignee_id: 'ada',
			checklist: { total: 2, done: 2, required_left: 0 }
		});
		const agentTurnRow: ActivityRow = {
			id: 'run-agent-turn',
			at: new Date(NOW - 60_000).toISOString(),
			actor: { id: 'kai', name: 'Kai Sato', kind: 'agent' },
			kind: 'run',
			action: 'run',
			text: 'drafting release notes on T-200',
			href: { path: '/work/T-200', label: 'T-200' },
			teamIds: [],
			projectIds: ['proj-b'],
			memberIds: ['kai'],
			run: {
				id: 'r-turn',
				memberId: 'kai',
				state: 'running',
				started_at: new Date(NOW - 120_000).toISOString(),
				trigger: 'Assigned T-200'
			}
		};

		const model = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems: [item1, item2],
			inboxItems: [],
			activityRows: [agentTurnRow],
			projectScope: 'proj-b',
			nowMs: NOW
		});

		expect(model.ciColumnSubLabel).toBe('2 ready for checks');
		expect(model.ciStationLabel).toBe('2 ready for checks');
		expect(model.milestones[0].lanes[0].subtitle).toContain('ready for checks');
		expect(model.milestones[0].lanes[0].subtitle).not.toContain('verifying in CI');
		expect(model.bottleneck.note).toBe('2 items ready for checks');
		expect(model.railAgents.find((a) => a.id === 'kai')?.statusLabel).toBe('Checks ready');
	});

	it('excludes historical rejected/handled decisions from "Needs you" when Inbox is empty', () => {
		const snapshot = makeSnapshot();
		const workItems: WorkItemSummary[] = [
			makeItem({
				id: 'w-req-rejected',
				short_id: 'R-404',
				kind: 'request',
				status: 'declined',
				project_id: 'proj-b'
			}),
			makeItem({
				id: 'w-task-blocked',
				short_id: 'T-500',
				kind: 'task',
				status: 'blocked',
				assignee_id: 'you',
				project_id: 'proj-b'
			}),
			makeItem({
				id: 'w-task-blocked-agent',
				short_id: 'T-501',
				kind: 'task',
				status: 'blocked',
				assignee_id: 'kai',
				project_id: 'proj-b'
			})
		];
		const activityRows: ActivityRow[] = [
			{
				id: 'act-hist-reject',
				at: new Date(NOW - 1_200_000).toISOString(),
				actor: { id: 'ada', name: 'Ada Lovelace', kind: 'agent' },
				kind: 'decision',
				action: 'decision',
				text: 'declined request R-404 after scope review',
				href: { path: '/work?request=w-req-rejected', label: 'R-404' },
				teamIds: [],
				projectIds: ['proj-b'],
				memberIds: ['ada']
			},
			{
				id: 'act-hist-approved',
				at: new Date(NOW - 900_000).toISOString(),
				actor: { id: 'ada', name: 'Ada Lovelace', kind: 'agent' },
				kind: 'decision',
				action: 'decision',
				text: 'approved splitting slice 2a.3.2b',
				teamIds: [],
				projectIds: ['proj-b'],
				memberIds: ['ada']
			},
			{
				id: 'act-current-blocked',
				at: new Date(NOW - 300_000).toISOString(),
				actor: { id: 'kai', name: 'Kai Sato', kind: 'agent' },
				kind: 'work',
				action: 'task',
				text: 'moved T-500 to Blocked',
				href: { path: '/work/T-500', label: 'T-500' },
				teamIds: [],
				projectIds: ['proj-b'],
				memberIds: ['kai']
			},
			{
				id: 'act-agent-blocked',
				at: new Date(NOW - 200_000).toISOString(),
				actor: { id: 'kai', name: 'Kai Sato', kind: 'agent' },
				kind: 'work',
				action: 'task',
				text: 'moved T-501 to Blocked on upstream vendor',
				href: { path: '/work/T-501', label: 'T-501' },
				teamIds: [],
				projectIds: ['proj-b'],
				memberIds: ['kai']
			}
		];

		const model = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems,
			inboxItems: [],
			activityRows,
			projectScope: 'proj-b',
			nowMs: NOW
		});

		const needsYouRows = filterStreamEntries(model.streamEntries, 'needs_you');
		expect(needsYouRows.map((e) => e.id)).toEqual(['act-current-blocked']);
		expect(model.streamNeedsYouCount).toBe(1);
		expect(model.streamEntries.find((e) => e.id === 'act-hist-reject')?.needsYou).toBe(false);
		expect(model.streamEntries.find((e) => e.id === 'act-hist-approved')?.needsYou).toBe(false);
		expect(model.streamEntries.find((e) => e.id === 'act-agent-blocked')?.needsYou).toBe(false);
	});

	it('infers project scope for Inbox items omitting item.project via referenced work item and handles unavailable Inbox honestly', () => {
		const snapshot = makeSnapshot();
		const workItems: WorkItemSummary[] = [
			makeItem({
				id: 'w-bug84',
				short_id: 'BUG-84',
				kind: 'bug',
				status: 'in_progress',
				project_id: 'proj-b'
			})
		];
		const inboxMissingProject: InboxItem = makeInbox({
			id: 'in-unscoped',
			kind: 'approval',
			title: 'Approve reroute for BUG-84',
			project: undefined,
			open: { path: '/work/BUG-84', label: 'Open BUG-84' }
		});

		const scoped = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems,
			inboxItems: [inboxMissingProject],
			activityRows: [],
			projectScope: 'proj-b',
			inboxPhase: 'ready',
			nowMs: NOW
		});

		expect(scoped.releaseGates.find((g) => g.id === 'approvals')?.count).toBe(1);
		expect(scoped.pendingApprovals).toEqual([
			{ id: 'in-unscoped', title: 'Approve reroute for BUG-84', href: '/inbox?item=in-unscoped' }
		]);
		expect(scoped.stageCounts.review).toBe(1);

		const unavailable = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems,
			inboxItems: [],
			activityRows: [],
			projectScope: 'proj-b',
			inboxPhase: 'error',
			nowMs: NOW
		});
		const gateErr = unavailable.releaseGates.find((g) => g.id === 'approvals');
		expect(gateErr?.statusLabel).toBe('Unavailable');
		expect(gateErr?.tone).toBe('warn');

		const loadingInbox = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems,
			inboxItems: [],
			activityRows: [],
			projectScope: 'proj-b',
			inboxPhase: 'loading',
			nowMs: NOW
		});
		const gateLoading = loadingInbox.releaseGates.find((g) => g.id === 'approvals');
		expect(gateLoading?.statusLabel).toBe('Loading…');
		expect(gateLoading?.tone).toBe('neutral');
	});

	it('gates work-derived throughput fallback on workPhase with source-specific messages and chart aria-labels', () => {
		const snapshot = makeSnapshot();
		const item = makeItem({
			id: 'w1',
			short_id: 'T-1',
			kind: 'task',
			status: 'todo',
			project_id: 'proj-b'
		});

		const readyFallback = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems: [item],
			inboxItems: [],
			activityRows: [],
			projectScope: 'proj-b',
			feedPhase: 'ready',
			workPhase: 'ready',
			nowMs: NOW
		});
		expect(readyFallback.throughputSource).toBe('work_updates');
		expect(readyFallback.throughputMeta).toBe('Items last updated (7d)');
		expect(readyFallback.throughputAriaLabel).toBe('7-day items last updated');

		const loadingWork = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems: [],
			inboxItems: [],
			activityRows: [],
			projectScope: 'proj-b',
			feedPhase: 'ready',
			workPhase: 'loading',
			nowMs: NOW
		});
		expect(loadingWork.throughputSource).toBe('loading');
		expect(loadingWork.throughputMeta).toBe('Loading…');
		expect(loadingWork.streamEntries).toEqual([]);

		const erroredWork = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems: [],
			inboxItems: [],
			activityRows: [],
			projectScope: 'proj-b',
			feedPhase: 'ready',
			workPhase: 'error',
			nowMs: NOW
		});
		expect(erroredWork.throughputSource).toBe('unavailable');
		expect(erroredWork.throughputMeta).toBe('Work data unavailable');
		expect(erroredWork.throughputUnavailableMessage).toBe(
			'Work data is unavailable for 7-day throughput.'
		);
		expect(erroredWork.streamEntries).toEqual([]);
	});

	it('caps incomplete completion percentages at 99% and requires exact done === total for 100%', () => {
		expect(computeCompletionPct(0, 0)).toBe(0);
		expect(computeCompletionPct(0, 10)).toBe(0);
		expect(computeCompletionPct(199, 200)).toBe(99);
		expect(computeCompletionPct(200, 200)).toBe(100);
		expect(computeCompletionPct(201, 200)).toBe(99);

		const snapshot = makeSnapshot();
		const items: WorkItemSummary[] = Array.from({ length: 200 }, (_, i) =>
			makeItem({
				id: `w-${i}`,
				short_id: `T-${i + 1}`,
				kind: 'task',
				status: i < 199 ? 'done' : 'todo',
				project_id: 'proj-b'
			})
		);
		const model = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems: items,
			inboxItems: [],
			activityRows: [],
			projectScope: 'proj-b',
			nowMs: NOW
		});

		expect(model.readinessPct).toBe(99);
		expect(model.isComplete).toBe(false);
		expect(model.milestones[0].pct).toBe(99);
		expect(model.milestones[0].isComplete).toBe(false);
		expect(model.milestones[0].lanes[0].pct).toBe(99);
		expect(model.milestones[0].lanes[0].isComplete).toBe(false);
	});

	it('extracts ONLY explicit work subject path when present, falling back to title refs otherwise', () => {
		const inboxWithExplicitPath = makeInbox({
			id: 'in-ref',
			title: 'Approve BUG-44 hotfix related to T-99',
			open: { path: '/work/T-99?tab=review', label: 'Open T-99' }
		});
		expect(extractInboxWorkRefs(inboxWithExplicitPath)).toEqual(['T-99']);

		const inboxTitleOnly = makeInbox({
			id: 'in-title-only',
			title: 'Approve BUG-44 hotfix',
			open: { path: '/projects/proj-a', label: 'Open project' }
		});
		expect(extractInboxWorkRefs(inboxTitleOnly)).toEqual(['BUG-44']);
	});

	it('excludes archived-only project rows, throughput, decisions, and approvals on All projects while retaining org-wide items', () => {
		const snapshot = makeSnapshot();
		const workItems: WorkItemSummary[] = [
			makeItem({
				id: 'w-act',
				short_id: 'T-10',
				kind: 'task',
				status: 'todo',
				project_id: 'proj-a'
			}),
			makeItem({
				id: 'w-arc',
				short_id: 'T-11',
				kind: 'task',
				status: 'todo',
				project_id: 'proj-old'
			})
		];
		const inboxItems: InboxItem[] = [
			makeInbox({
				id: 'in-arc',
				title: 'Old archived approval',
				project: { id: 'proj-old', name: 'Archived Legacy' }
			}),
			makeInbox({
				id: 'in-arc-ref',
				title: 'Archived approval via work ref',
				project: undefined,
				open: { path: '/work/T-11', label: 'Open T-11' }
			}),
			makeInbox({
				id: 'in-org',
				title: 'Org-wide policy approval',
				project: undefined
			})
		];
		const activityRows: ActivityRow[] = [
			{
				id: 'act-arc',
				at: new Date(NOW - 60_000).toISOString(),
				actor: { id: 'kai', name: 'Kai Sato', kind: 'agent' },
				kind: 'work',
				action: 'task',
				text: 'updated T-11',
				teamIds: [],
				projectIds: ['proj-old'],
				memberIds: ['kai']
			},
			{
				id: 'act-org',
				at: new Date(NOW - 30_000).toISOString(),
				actor: { id: 'ada', name: 'Ada Lovelace', kind: 'agent' },
				kind: 'org',
				action: 'member',
				text: 'updated org roster',
				teamIds: [],
				projectIds: [],
				memberIds: ['ada']
			},
			{
				id: 'act-mixed',
				at: new Date(NOW - 10_000).toISOString(),
				actor: { id: 'kai', name: 'Kai Sato', kind: 'agent' },
				kind: 'work',
				action: 'task',
				text: 'shared update across active and archived',
				teamIds: [],
				projectIds: ['proj-old', 'proj-a'],
				memberIds: ['kai']
			}
		];

		const model = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems,
			inboxItems,
			activityRows,
			projectScope: 'all',
			nowMs: NOW
		});

		expect(model.streamEntries.map((e) => e.id)).toEqual(['act-mixed', 'act-org']);
		expect(model.throughput7d[6].count).toBe(2);
		expect(model.openInboxCount).toBe(1);
		expect(model.pendingApprovals.map((a) => a.id)).toEqual(['in-org']);

		const decisions = deriveDecisionStreamEntries({
			snapshot,
			decisionRows: [
				{
					id: 'dec-arc',
					at: new Date(NOW - 60_000).toISOString(),
					actor: { id: 'ada', name: 'Ada Lovelace', kind: 'agent' },
					kind: 'decision',
					action: 'decision',
					text: 'old archived decision',
					teamIds: [],
					projectIds: ['proj-old'],
					memberIds: ['ada']
				},
				{
					id: 'dec-org',
					at: new Date(NOW - 30_000).toISOString(),
					actor: { id: 'ada', name: 'Ada Lovelace', kind: 'agent' },
					kind: 'decision',
					action: 'decision',
					text: 'org-wide decision',
					teamIds: [],
					projectIds: [],
					memberIds: ['ada']
				}
			],
			workItems,
			inboxItems,
			projectScope: 'all',
			decisionPhase: 'ready',
			workPhase: 'ready',
			inboxPhase: 'ready'
		});
		expect(decisions.map((d) => d.id)).toEqual(['dec-org']);
	});

	it('filters Release Gate approvals strictly to approval kind and deep-links to /inbox?item=<id>', () => {
		const snapshot = makeSnapshot();
		const inboxItems: InboxItem[] = [
			makeInbox({
				id: 'in-app-1',
				kind: 'approval',
				title: 'Approve release v2.0',
				project: { id: 'proj-b', name: 'Beta Release' }
			}),
			makeInbox({
				id: 'in-dec-1',
				kind: 'decision',
				title: 'Choose lock strategy',
				project: { id: 'proj-b', name: 'Beta Release' },
				actions: ['choose']
			}),
			makeInbox({
				id: 'in-q-1',
				kind: 'question',
				title: 'Clarify API naming',
				project: { id: 'proj-b', name: 'Beta Release' },
				actions: ['reply']
			})
		];

		const model = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems: [],
			inboxItems,
			activityRows: [],
			projectScope: 'proj-b',
			nowMs: NOW
		});

		const approvalGate = model.releaseGates.find((g) => g.id === 'approvals');
		const decisionGate = model.releaseGates.find((g) => g.id === 'decisions');
		expect(approvalGate?.count).toBe(1);
		expect(decisionGate?.count).toBe(1);
		expect(model.pendingApprovals).toEqual([
			{ id: 'in-app-1', title: 'Approve release v2.0', href: '/inbox?item=in-app-1' }
		]);
	});

	it('reports honest throughput labels and messages for capped events and feed errors', () => {
		const snapshot = makeSnapshot();
		const row: ActivityRow = {
			id: 'r1',
			at: new Date(NOW - 600_000).toISOString(),
			actor: { id: 'kai', name: 'Kai Sato', kind: 'agent' },
			kind: 'work',
			action: 'task',
			text: 'updated T-1',
			teamIds: [],
			projectIds: ['proj-b'],
			memberIds: ['kai']
		};

		const capped = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems: [],
			inboxItems: [],
			activityRows: [row],
			projectScope: 'proj-b',
			feedPhase: 'ready',
			feedMore: 25,
			nowMs: NOW
		});
		expect(capped.throughputSource).toBe('events_capped');
		expect(capped.throughputMeta).toBe('Recent events (capped)');
		expect(capped.throughputAriaLabel).toBe('Recent 7-day activity events (capped)');

		const errored = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems: [
				makeItem({ id: 'w1', short_id: 'T-1', kind: 'task', status: 'todo', project_id: 'proj-b' })
			],
			inboxItems: [],
			activityRows: [],
			projectScope: 'proj-b',
			feedPhase: 'error',
			nowMs: NOW
		});
		expect(errored.throughputSource).toBe('unavailable');
		expect(errored.throughputMeta).toBe('Feed unavailable');
		expect(errored.throughputUnavailableMessage).toBe(
			'Activity feed is unavailable for 7-day throughput.'
		);
		expect(errored.streamEntries).toEqual([]);
	});

	it('groups by areas when a project has multiple areas and by work track when single-area', () => {
		const snapshot = makeSnapshot();
		const workItems: WorkItemSummary[] = [
			makeItem({
				id: 'w1',
				short_id: 'T-1',
				kind: 'task',
				status: 'done',
				project_id: 'proj-a',
				area_id: 'area-api',
				assignee_id: 'kai'
			}),
			makeItem({
				id: 'w2',
				short_id: 'T-2',
				kind: 'task',
				status: 'in_progress',
				project_id: 'proj-a',
				area_id: 'area-ui',
				assignee_id: 'kai',
				checklist: { total: 2, done: 1, required_left: 1 }
			}),
			makeItem({
				id: 'w3',
				short_id: 'T-3',
				kind: 'task',
				status: 'in_progress',
				project_id: 'proj-b',
				area_id: 'area-b-gen',
				assignee_id: 'ada',
				checklist: { total: 2, done: 2, required_left: 0 }
			}),
			makeItem({
				id: 'w4',
				short_id: 'BUG-4',
				kind: 'bug',
				status: 'new',
				severity: 'critical',
				project_id: 'proj-b',
				area_id: 'area-b-gen'
			}),
			makeItem({
				id: 'w5',
				short_id: 'T-99',
				kind: 'task',
				status: 'done',
				project_id: 'proj-old'
			}),
			makeItem({
				id: 'w6',
				short_id: 'T-100',
				kind: 'task',
				status: 'todo',
				project_id: 'proj-a',
				deleted: true
			})
		];

		const model = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems,
			inboxItems: [],
			activityRows: [],
			projectScope: 'all',
			nowMs: NOW
		});

		expect(model.activeProjects.map((p) => p.id)).toEqual(['proj-a', 'proj-b']);
		expect(model.milestones.map((m) => m.name)).toEqual(['Alpha Core', 'Beta Release']);
		expect(model.milestones[0].summary).toBe('50% · 1/2 tasks · 1/2 tracks');
		expect(model.milestones[1].summary).toBe('0% · 0/2 tasks · 0/2 tracks');
		expect(model.milestones[0].lanes.map((l) => l.name)).toEqual(['API', 'Interface']);
		expect(model.milestones[0].lanes[1].stages.agents[0]?.agentFirstName).toBe('Kai');
		expect(model.milestones[1].lanes.map((l) => l.name)).toEqual(['Tasks', 'Bugs']);
		expect(model.totalItems).toBe(4);
		expect(model.mergedItems).toBe(1);
		expect(model.openItems).toBe(3);
		expect(model.readinessPct).toBe(25);
		expect(model.stageCounts).toEqual({ queue: 1, agents: 1, ci: 1, review: 0, merged: 1 });
		expect(model.busyAgentsCount).toBe(2);
		expect(model.activeAgentsCount).toBe(2);
		expect(model.releaseGates.find((g) => g.id === 'critical-bugs')?.count).toBe(1);
		expect(model.railAgents.map((a) => a.id)).not.toContain('former');
		expect(model.railAgents.find((a) => a.id === 'avery')?.status).toBe('paused');
	});

	it('scopes metrics, milestones, gates, and stream to a single selected project with inbox deep links', () => {
		const snapshot = makeSnapshot();
		const workItems: WorkItemSummary[] = [
			makeItem({
				id: 'w1',
				short_id: 'T-1',
				kind: 'task',
				status: 'done',
				project_id: 'proj-a',
				area_id: 'area-api'
			}),
			makeItem({
				id: 'w2',
				short_id: 'T-2',
				kind: 'task',
				status: 'blocked',
				project_id: 'proj-b',
				area_id: 'area-b-gen'
			})
		];
		const inboxItems: InboxItem[] = [
			makeInbox({
				id: 'in-a',
				title: 'Alpha sign-off',
				project: { id: 'proj-a', name: 'Alpha Core' }
			}),
			makeInbox({
				id: 'in-b',
				title: 'Beta unblock T-2',
				project: { id: 'proj-b', name: 'Beta Release' },
				open: { path: '/work/T-2', label: 'Open T-2' }
			})
		];
		const activityRows: ActivityRow[] = [
			{
				id: 'r1',
				at: new Date(NOW - 600_000).toISOString(),
				actor: { id: 'kai', name: 'Kai Sato', kind: 'agent' },
				kind: 'run',
				action: 'run',
				text: 'finished run on T-1',
				teamIds: [],
				projectIds: ['proj-a'],
				memberIds: ['kai']
			},
			{
				id: 'r2',
				at: new Date(NOW - 300_000).toISOString(),
				actor: { id: 'ada', name: 'Ada Lovelace', kind: 'agent' },
				kind: 'work',
				action: 'task',
				text: 'blocked T-2 waiting on review',
				href: { path: '/work/T-2', label: 'T-2' },
				teamIds: [],
				projectIds: ['proj-b'],
				memberIds: ['ada']
			}
		];

		const model = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems,
			inboxItems,
			activityRows,
			projectScope: 'proj-b',
			nowMs: NOW
		});

		expect(model.scopeTitle).toBe('Beta Release');
		expect(model.scopeEyebrow).toBe('V2.0 · PRODUCTION LINE');
		expect(model.milestones).toHaveLength(1);
		expect(model.milestones[0].lanes[0].stages.review[0]?.href).toBe('/inbox?item=in-b');
		expect(model.streamEntries).toHaveLength(1);
		expect(model.streamEntries[0].needsYou).toBe(true);
		expect(model.streamEntries[0].inboxHref).toBe('/inbox?item=in-b');
		expect(filterStreamEntries(model.streamEntries, 'needs_you')).toHaveLength(1);
		expect(filterStreamEntries(model.streamEntries, 'run')).toHaveLength(0);
	});

	it('computes bottleneck percentages summing to 100% when open items exist', () => {
		const b = computeBottleneck({ queue: 1, agents: 1, ci: 1, review: 0, merged: 5 });
		expect(b.agentsPct + b.ciPct + b.reviewPct + b.queuePct).toBe(100);
		expect(b.totalOpen).toBe(3);

		const empty = computeBottleneck({ queue: 0, agents: 0, ci: 0, review: 0, merged: 4 });
		expect(empty.totalOpen).toBe(0);
		expect(empty.agentsPct).toBe(0);
	});

	it('buckets 7-day throughput accurately and ignores timestamps outside the 7-day window', () => {
		const buckets = computeThroughput7d(
			[
				new Date(NOW - 10 * DAY).toISOString(),
				new Date(NOW - 6 * DAY).toISOString(),
				new Date(NOW - 2 * DAY).toISOString(),
				new Date(NOW - 2 * DAY + 3_600_000).toISOString(),
				new Date(NOW - 1_000).toISOString(),
				'not-a-date'
			],
			NOW
		);
		expect(buckets).toHaveLength(7);
		expect(buckets[0].count).toBe(1);
		expect(buckets[4].count).toBe(2);
		expect(buckets[4].heightPct).toBe(100);
		expect(buckets[6].count).toBe(1);
		expect(buckets[6].isToday).toBe(true);
		expect(buckets[6].label).toBe('Today');
	});

	it('handles an empty organization gracefully without NaN or crashes', () => {
		const emptySnapshot: OrgSnapshot = {
			position: { epoch: 1, seq: 0 },
			teams: [],
			members: [{ id: 'you', name: 'You', kind: 'person', owner: true, version: 1 }],
			projects: [],
			areas: []
		};
		const model = deriveActivityCommandCenter({
			snapshot: emptySnapshot,
			organizationName: 'Blank Org',
			workItems: [],
			inboxItems: [],
			activityRows: [],
			projectScope: 'all',
			nowMs: NOW
		});

		expect(model.readinessPct).toBe(0);
		expect(model.milestones).toEqual([]);
		expect(model.streamEntries).toEqual([]);
		expect(model.throughput7d.every((d) => d.count === 0)).toBe(true);
		expect(model.railAgents).toEqual([]);
		expect(model.needsYouCount).toBe(0);
	});

	it('merges live activity rows by upsert, filters via matchesQuery, and counts evicted rows in more', () => {
		const rowRunning: ActivityRow = {
			id: 'run-1',
			at: new Date(NOW - 120_000).toISOString(),
			actor: { id: 'kai', name: 'Kai Sato', kind: 'agent' },
			kind: 'run',
			action: 'run',
			text: 'started run on T-1',
			teamIds: [],
			projectIds: ['proj-a'],
			memberIds: ['kai'],
			run: {
				id: 'r1',
				memberId: 'kai',
				state: 'running',
				started_at: new Date(NOW - 120_000).toISOString(),
				trigger: 'Assigned T-1'
			}
		};
		const rowOlder: ActivityRow = {
			id: 'work-0',
			at: new Date(NOW - 300_000).toISOString(),
			actor: { id: 'ada', name: 'Ada Lovelace', kind: 'agent' },
			kind: 'work',
			action: 'task',
			text: 'created T-0',
			teamIds: [],
			projectIds: ['proj-a'],
			memberIds: ['ada']
		};
		const rowSucceeded: ActivityRow = {
			...rowRunning,
			at: new Date(NOW - 10_000).toISOString(),
			text: 'finished run on T-1',
			run: { ...rowRunning.run!, state: 'succeeded', outcome: 'Done' }
		};
		const noisyStatus: ActivityRow = {
			id: 'status-1',
			at: new Date(NOW - 5_000).toISOString(),
			actor: { id: 'kai', name: 'Kai Sato', kind: 'agent' },
			kind: 'work',
			action: 'status',
			text: 'went online',
			status: true,
			teamIds: [],
			projectIds: ['proj-a'],
			memberIds: ['kai']
		};
		const newWork: ActivityRow = {
			id: 'work-2',
			at: new Date(NOW - 1_000).toISOString(),
			actor: { id: 'ada', name: 'Ada Lovelace', kind: 'agent' },
			kind: 'work',
			action: 'task-done',
			text: 'completed T-2',
			teamIds: [],
			projectIds: ['proj-a'],
			memberIds: ['ada']
		};

		// Upsert replaces run-1 in place; status row is ignored by matchesQuery; limit 2 evicts work-0
		const merged = mergeLiveActivityRows(
			[rowRunning, rowOlder],
			[rowSucceeded, noisyStatus, newWork],
			{ kind: 'all', project: 'proj-a' },
			2,
			0
		);
		expect(merged.changed).toBe(true);
		expect(merged.rows.map((r) => r.id)).toEqual(['work-2', 'run-1']);
		expect(merged.rows[1].run?.state).toBe('succeeded');
		expect(merged.more).toBe(1);

		// Buffered older `running` row MUST NOT overwrite a newer terminal `succeeded` snapshot row
		const snapshotSucceeded: ActivityRow = {
			...rowSucceeded,
			at: new Date(NOW - 10_000).toISOString(),
			run: {
				...rowSucceeded.run!,
				state: 'succeeded',
				ended_at: new Date(NOW - 10_000).toISOString()
			}
		};
		const bufferedOlderRunning: ActivityRow = {
			...rowRunning,
			at: new Date(NOW - 10_000).toISOString(),
			run: { ...rowRunning.run!, state: 'running', ended_at: undefined }
		};
		const mergedNoDowngrade = mergeLiveActivityRows(
			[snapshotSucceeded],
			[bufferedOlderRunning],
			{ kind: 'all', project: 'proj-a' },
			200,
			0
		);
		expect(mergedNoDowngrade.changed).toBe(false);
		expect(mergedNoDowngrade.rows[0].run?.state).toBe('succeeded');
	});

	it('derives dedicated Decisions tab stream entries and never falls back to work items when empty', () => {
		const snapshot = makeSnapshot();
		const decisionRow: ActivityRow = {
			id: 'dec-1',
			at: new Date(NOW - 60_000).toISOString(),
			actor: { id: 'ada', name: 'Ada Lovelace', kind: 'agent' },
			kind: 'decision',
			action: 'decision',
			text: 'approved splitting codec work into two slices',
			teamIds: [],
			projectIds: ['proj-a'],
			memberIds: ['ada']
		};

		const entries = deriveDecisionStreamEntries({
			snapshot,
			decisionRows: [decisionRow],
			workItems: [],
			inboxItems: [],
			projectScope: 'proj-a',
			decisionPhase: 'ready',
			workPhase: 'ready',
			inboxPhase: 'ready'
		});
		expect(entries).toHaveLength(1);
		expect(entries[0].id).toBe('dec-1');
		expect(entries[0].needsYou).toBe(false);

		// Empty decision query must return [] even when new/scoped requests exist in workItems
		const emptyDecisions = deriveDecisionStreamEntries({
			snapshot,
			decisionRows: [],
			workItems: [
				makeItem({
					id: 'req-new',
					short_id: 'R-90',
					kind: 'request',
					status: 'new',
					project_id: 'proj-a'
				}),
				makeItem({
					id: 'req-scoped',
					short_id: 'R-91',
					kind: 'request',
					status: 'scoped',
					project_id: 'proj-a'
				})
			],
			inboxItems: [],
			projectScope: 'proj-a',
			decisionPhase: 'ready',
			workPhase: 'ready',
			inboxPhase: 'ready'
		});
		expect(emptyDecisions).toEqual([]);
	});

	it('matches stream row Inbox approvals strictly against explicit work href subject without falling back to mentioned text refs', () => {
		const snapshot = makeSnapshot();
		const inboxForT8 = makeInbox({
			id: 'in-t8',
			kind: 'approval',
			title: 'Approve T-8 rollout',
			open: { path: '/work/T-8', label: 'Open T-8' },
			project: { id: 'proj-a', name: 'Alpha Core' }
		});
		const rowLinkedToT9: ActivityRow = {
			id: 'row-t9',
			at: new Date(NOW - 30_000).toISOString(),
			actor: { id: 'kai', name: 'Kai Sato', kind: 'agent' },
			kind: 'work',
			action: 'task',
			text: 'unblocked T-9 after checking T-8',
			href: { path: '/work/T-9', label: 'Open T-9' },
			teamIds: [],
			projectIds: ['proj-a'],
			memberIds: ['kai']
		};
		const rowWithoutWorkHref: ActivityRow = {
			id: 'row-chat-t8',
			at: new Date(NOW - 20_000).toISOString(),
			actor: { id: 'ada', name: 'Ada Lovelace', kind: 'agent' },
			kind: 'conversation',
			action: 'message',
			text: 'please review T-8 when ready',
			href: { path: '/chat/thread-1', label: 'Open thread' },
			teamIds: [],
			projectIds: ['proj-a'],
			memberIds: ['ada']
		};

		const model = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems: [],
			inboxItems: [inboxForT8],
			activityRows: [rowLinkedToT9, rowWithoutWorkHref],
			projectScope: 'proj-a',
			ownerId: 'you',
			feedPhase: 'ready',
			workPhase: 'ready',
			inboxPhase: 'ready',
			nowMs: NOW
		});

		const entryT9 = model.streamEntries.find((e) => e.id === 'row-t9');
		const entryChatT8 = model.streamEntries.find((e) => e.id === 'row-chat-t8');
		expect(entryT9?.inboxHref).toBeUndefined();
		expect(entryT9?.needsYou).toBe(false);
		expect(entryChatT8?.inboxHref).toBe('/inbox?item=in-t8');
		expect(entryChatT8?.needsYou).toBe(true);
	});

	it('derives project telemetry (avgCycleHours, waitingOnPeopleHours, stationAgents, humanWaitingItems, lineEvents) and supports explicit archived project scope', () => {
		const snapshot = makeSnapshot();
		const workItems: WorkItemSummary[] = [
			makeItem({
				id: 'w-done-1',
				short_id: 'T-10',
				kind: 'task',
				status: 'done',
				project_id: 'proj-a',
				area_id: 'area-api',
				created_at: new Date(NOW - 10 * 3_600_000).toISOString(),
				updated_at: new Date(NOW - 4 * 3_600_000).toISOString() // 6h cycle
			}),
			makeItem({
				id: 'w-done-2',
				short_id: 'T-11',
				kind: 'task',
				status: 'done',
				project_id: 'proj-a',
				area_id: 'area-api',
				created_at: new Date(NOW - 16 * 3_600_000).toISOString(),
				updated_at: new Date(NOW - 4 * 3_600_000).toISOString() // 12h cycle -> avg 9.0h
			}),
			makeItem({
				id: 'w-wait',
				short_id: 'T-12',
				kind: 'task',
				status: 'blocked',
				project_id: 'proj-a',
				area_id: 'area-ui',
				created_at: new Date(NOW - 5 * 3_600_000).toISOString(),
				updated_at: new Date(NOW - 2 * 3_600_000).toISOString() // 2h waiting on people
			}),
			makeItem({
				id: 'w-old',
				short_id: 'T-99',
				kind: 'task',
				status: 'done',
				project_id: 'proj-old',
				created_at: new Date(NOW - 8 * 3_600_000).toISOString(),
				updated_at: new Date(NOW - 3 * 3_600_000).toISOString()
			})
		];

		const model = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems,
			inboxItems: [
				makeInbox({
					id: 'in-wait',
					title: 'Unblock T-12',
					project: { id: 'proj-a', name: 'Alpha Core' },
					open: { path: '/work/T-12', label: 'Open T-12' },
					created_at: new Date(NOW - 2 * 3_600_000).toISOString()
				})
			],
			activityRows: [
				{
					id: 'ev-1',
					at: new Date(NOW - 120_000).toISOString(),
					actor: { id: 'kai', name: 'Kai Sato', kind: 'agent' },
					kind: 'work',
					action: 'task-done',
					text: 'finished T-11',
					teamIds: [],
					projectIds: ['proj-a'],
					memberIds: ['kai']
				}
			],
			projectScope: 'proj-a',
			nowMs: NOW
		});

		expect(model.avgCycleHours).toBeCloseTo(9, 1);
		expect(formatTelemetryHours(model.avgCycleHours)).toBe('9.0 h');
		expect(formatTelemetryHours(null)).toBe('—');
		expect(model.waitingOnPeopleHours).toBeCloseTo(2, 1);
		expect(model.stationAgents.length).toBeGreaterThan(0);
		expect(model.humanWaitingItems.map((i) => i.id)).toContain('w-wait');
		expect(model.lineEvents).toHaveLength(1);
		expect(model.lineEvents[0].text).toContain('finished T-11');

		// Explicit archived project scope still resolves that archived project
		const archivedModel = deriveActivityCommandCenter({
			snapshot,
			organizationName: 'Acme Corp',
			workItems,
			inboxItems: [],
			activityRows: [],
			projectScope: 'proj-old',
			nowMs: NOW
		});
		expect(archivedModel.selectedProject?.id).toBe('proj-old');
		expect(archivedModel.scopeTitle).toBe('Archived Legacy');
		expect(archivedModel.totalItems).toBe(1);
		expect(archivedModel.avgCycleHours).toBeCloseTo(5, 1);
	});
});
