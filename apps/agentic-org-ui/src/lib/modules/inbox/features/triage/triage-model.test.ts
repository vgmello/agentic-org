import { describe, expect, it } from 'vitest';
import type { OrgSnapshot, WorkItemSummary } from '$lib/proto';
import type { InboxItem } from '../../types';
import {
	deriveTriageItems,
	filterTriageItems,
	flattenGroups,
	groupTriageItems,
	highlightSegments,
	isDeniedOutcome,
	neighborCandidatesAfterResolve,
	nextSelectionAfterResolve,
	openWaitingCount,
	parseTriageTab,
	splitHref,
	triageScopeOptions,
	triageTabCounts
} from './triage-model';

const NOW = Date.parse('2026-11-23T12:00:00Z');
const minsAgo = (m: number) => new Date(NOW - m * 60_000).toISOString();
const minsAhead = (m: number) => new Date(NOW + m * 60_000).toISOString();

const SNAPSHOT: OrgSnapshot = {
	position: { epoch: 1, seq: 10 },
	teams: [
		{ id: 'platform', name: 'Platform', version: 1 },
		{ id: 'app', name: 'App', version: 1 }
	],
	members: [
		{ id: 'owner', name: 'You', kind: 'person', owner: true, version: 1 },
		{
			id: 'ada',
			name: 'Ada',
			kind: 'agent',
			agent: {
				status: 'active',
				role: 'engineering_manager',
				title: 'Engineering Manager',
				messaging_name: 'ada',
				runtime: 'claude_code'
			},
			version: 1
		},
		{ id: 'maya', name: 'Maya Chen', kind: 'person', version: 1 }
	],
	memberships: [],
	projects: [
		{ id: 'v1', name: 'Agentic Org v1.0', key: 'v1.0', version: 1 },
		{ id: 'fed', name: 'Office federation', key: 'v0.9', version: 1 }
	],
	project_teams: [],
	areas: [
		{
			id: 'v1-gen',
			project_id: 'v1',
			name: 'General',
			general: true,
			team_id: 'platform',
			version: 1
		}
	]
};

const INBOX_ITEMS: InboxItem[] = [
	{
		id: 'in-urgent',
		kind: 'approval',
		source: 'Release sign-off',
		title: "Approve publishing Kai's sync update to v1.0",
		who: { id: 'ada', name: 'Ada', kind: 'agent' },
		team: { id: 'platform', name: 'Platform' },
		project: { id: 'v1', name: 'Agentic Org v1.0' },
		why: 'All 412 checks passed.',
		recommendation: 'Approve — review completed with zero open issues.',
		created_at: minsAgo(15),
		expires_at: minsAhead(180),
		actions: ['approve', 'deny'],
		open: { path: '/work/T-121', label: 'Open T-121' },
		state: 'open'
	},
	{
		id: 'in-today',
		kind: 'decision',
		source: 'Direction needed',
		title: 'Choose timed access or fixed lock',
		who: { id: 'maya', name: 'Maya Chen', kind: 'person' },
		team: { id: 'platform', name: 'Platform' },
		project: { id: 'fed', name: 'Office federation' },
		why: 'Offices edit shared manifests concurrently.',
		recommendation: 'Timed access reduces contention.',
		created_at: minsAgo(45),
		actions: ['choose'],
		choices: [
			{ id: 'timed', label: 'Use timed access' },
			{ id: 'fixed', label: 'Fixed lock' }
		],
		open: { path: '/work/T-118', label: 'Open T-118' },
		state: 'open'
	},
	{
		id: 'in-earlier',
		kind: 'question',
		source: 'Task question',
		title: 'Keep basket in server storage?',
		who: { id: 'ada', name: 'Ada', kind: 'agent' },
		team: { id: 'app', name: 'App' },
		project: { id: 'v1', name: 'Agentic Org v1.0' },
		why: 'Cross-device persistence.',
		created_at: minsAgo(36 * 60),
		actions: ['reply'],
		state: 'open'
	},
	{
		id: 'in-problem',
		kind: 'problem',
		source: 'Run crashed',
		title: 'Sync runner crashed on T-19',
		who: { id: 'ada', name: 'Ada', kind: 'agent' },
		team: { id: 'platform', name: 'Platform' },
		project: { id: 'fed', name: 'Office federation' },
		why: 'Permission denied on changelog.',
		created_at: minsAgo(90),
		actions: ['dismiss'],
		state: 'open'
	},
	{
		id: 'in-handled',
		kind: 'approval',
		source: 'Run approval',
		title: 'Approve the A-OC1 pins',
		who: { id: 'ada', name: 'Ada', kind: 'agent' },
		team: { id: 'platform', name: 'Platform' },
		project: { id: 'fed', name: 'Office federation' },
		why: 'Pins office contract versions.',
		created_at: minsAgo(48 * 60),
		actions: [],
		state: 'handled',
		handled: { outcome: 'Approved', by: 'you', at: minsAgo(120) }
	}
];

const WORK_ITEMS: WorkItemSummary[] = [
	{
		id: 'w-t121',
		short_id: 'T-121',
		kind: 'task',
		number: 121,
		title: 'Split codec 2a.3.2b',
		status: 'in_progress',
		project_id: 'v1',
		area_id: 'v1-gen',
		assignee_id: 'ada',
		created_by: 'ada',
		created_at: minsAgo(200),
		updated_at: minsAgo(10),
		severity_confirmed: false,
		fixes: [],
		fixed_by: [],
		checklist: { total: 3, done: 3, required_left: 0 },
		done_anyway: false,
		owner_office_id: 'office-hq',
		generation: 1,
		reopen_count: 0,
		version: 1,
		deleted: false
	},
	{
		id: 'w-req1',
		short_id: 'R-9',
		kind: 'request',
		number: 9,
		title: 'Scope audit log export',
		status: 'scoped',
		project_id: 'v1',
		area_id: 'v1-gen',
		assignee_id: 'maya',
		created_by: 'maya',
		created_at: minsAgo(100),
		updated_at: minsAgo(30),
		severity_confirmed: false,
		fixes: [],
		fixed_by: [],
		checklist: { total: 0, done: 0, required_left: 0 },
		done_anyway: false,
		owner_office_id: 'office-hq',
		generation: 1,
		reopen_count: 0,
		version: 1,
		deleted: false
	}
];

describe('highlightSegments', () => {
	it('returns single non-matching segment when query is blank', () => {
		expect(highlightSegments('Approve v1.0', '   ')).toEqual([
			{ text: 'Approve v1.0', match: false }
		]);
	});

	it('splits case-insensitively and escapes regex metacharacters safely', () => {
		expect(highlightSegments('Release (v1.0) & [v1.0] check', '(v1.0)')).toEqual([
			{ text: 'Release ', match: false },
			{ text: '(v1.0)', match: true },
			{ text: ' & [v1.0] check', match: false }
		]);
		expect(highlightSegments('Sync sync SYNC', 'sync')).toEqual([
			{ text: 'Sync', match: true },
			{ text: ' ', match: false },
			{ text: 'sync', match: true },
			{ text: ' ', match: false },
			{ text: 'SYNC', match: true }
		]);
	});
});

describe('triage model derivation, filtering, grouping and neighbour selection', () => {
	const allItems = deriveTriageItems(WORK_ITEMS, SNAPSHOT, 'owner', INBOX_ITEMS, NOW);

	it('derives both open inbox/work decisions and handled inbox items with enriched metadata', () => {
		expect(openWaitingCount(allItems)).toBe(5);
		const urgent = allItems.find((i) => i.id === 'in-urgent');
		expect(urgent?.workRef).toBe('T-121');
		expect(urgent?.workRefDisplay).toBe('T-121 · Split codec 2a.3.2b');
		expect(urgent?.actorRole).toBe('Engineering Manager · Agent');
		expect(urgent?.isAgent).toBe(true);

		const handled = allItems.find((i) => i.id === 'in-handled');
		expect(handled?.state).toBe('handled');
		expect(handled?.handled?.outcome).toBe('Approved');
		expect(handled?.handled?.by).toBe('you');
	});

	it('computes accurate tab counts under scope filters', () => {
		expect(triageTabCounts(allItems)).toEqual({
			all: 5,
			approvals: 1,
			decisions: 1,
			questions: 1,
			handled: 1,
			problems: 1,
			work: 1
		});
		expect(triageTabCounts(allItems, { project: 'fed' })).toEqual({
			all: 2,
			approvals: 0,
			decisions: 1,
			questions: 0,
			handled: 1,
			problems: 1,
			work: 0
		});
	});

	it('filters by tab, urgency, scope, and search query and sorts urgent first', () => {
		const urgentOnly = filterTriageItems(allItems, {
			tab: 'all',
			urgentOnly: true,
			scope: {},
			search: ''
		});
		expect(urgentOnly.map((i) => i.id)).toEqual(['in-urgent', 'in-problem']);

		const searchHit = filterTriageItems(allItems, {
			tab: 'all',
			urgentOnly: false,
			scope: {},
			search: 'codec'
		});
		expect(searchHit.map((i) => i.id)).toEqual(['in-urgent']);

		const workTab = filterTriageItems(allItems, {
			tab: 'work',
			urgentOnly: false,
			scope: {},
			search: ''
		});
		expect(workTab.map((i) => i.id)).toEqual(['w-req1']);

		const handledTab = filterTriageItems(allItems, {
			tab: 'handled',
			urgentOnly: true,
			scope: {},
			search: ''
		});
		expect(handledTab.map((i) => i.id)).toEqual(['in-handled']);
	});

	it('groups open items into Urgent, Today, Earlier and handled into Handled', () => {
		const visibleOpen = filterTriageItems(allItems, {
			tab: 'all',
			urgentOnly: false,
			scope: {},
			search: ''
		});
		const groups = groupTriageItems(visibleOpen, 'all', NOW);
		expect(groups.map((g) => g.key)).toEqual(['urgent', 'today', 'earlier']);
		expect(groups[0].items.map((i) => i.id)).toEqual(['in-urgent', 'in-problem']);
		expect(groups[1].items.map((i) => i.id)).toEqual(['w-req1', 'in-today']);
		expect(groups[2].items.map((i) => i.id)).toEqual(['in-earlier']);

		const handledVisible = filterTriageItems(allItems, {
			tab: 'handled',
			urgentOnly: false,
			scope: {},
			search: ''
		});
		expect(groupTriageItems(handledVisible, 'handled', NOW).map((g) => g.key)).toEqual(['handled']);
	});

	it('selects next then previous neighbour after resolving an item', () => {
		const visible = flattenGroups(
			groupTriageItems(
				filterTriageItems(allItems, { tab: 'all', urgentOnly: false, scope: {}, search: '' }),
				'all',
				NOW
			)
		);
		expect(nextSelectionAfterResolve(visible, visible[0].id)).toBe(visible[1].id);
		expect(nextSelectionAfterResolve(visible, visible[visible.length - 1].id)).toBe(
			visible[visible.length - 2].id
		);
		expect(nextSelectionAfterResolve([visible[0]], visible[0].id)).toBeUndefined();
		// Resolving middle item B in [A, B, C] prefers C then A.
		const triple = visible.slice(0, 3);
		expect(neighborCandidatesAfterResolve(triple, triple[1].id)).toEqual([
			triple[2].id,
			triple[0].id
		]);
	});

	it('extracts sorted scope options and parses tab aliases safely against prototype keys', () => {
		const opts = triageScopeOptions(allItems);
		expect(opts.teams.map((t) => t.name)).toEqual(['App', 'Platform']);
		expect(opts.projects.map((p) => p.name)).toEqual(['Agentic Org v1.0', 'Office federation']);
		expect(parseTriageTab('approval')).toBe('approvals');
		expect(parseTriageTab('Handled')).toBe('handled');
		expect(parseTriageTab('unknown')).toBe('all');
		expect(parseTriageTab('constructor')).toBe('all');
		expect(parseTriageTab('__proto__')).toBe('all');
		expect(parseTriageTab('toString')).toBe('all');
		expect(isDeniedOutcome('Denied: out of scope')).toBe(true);
		expect(isDeniedOutcome('Approved')).toBe(false);
		expect(splitHref('/work/T-121#review[notes]')).toEqual({
			pathname: '/work/T-121',
			suffix: '#review[notes]'
		});
		expect(splitHref('/work?request=r1#sec[1]')).toEqual({
			pathname: '/work',
			suffix: '?request=r1#sec[1]'
		});
		expect(splitHref('/work/T-121')).toEqual({ pathname: '/work/T-121', suffix: '' });
	});
});
