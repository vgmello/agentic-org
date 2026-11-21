import { describe, expect, it } from 'vitest';
import {
	branchProblem,
	briefBlocks,
	bugsText,
	doneText,
	featuresText,
	keepWords,
	openText,
	projectTabs,
	slugFrom,
	slugProblem,
	sortActivityNewestFirst,
	sortProjects,
	tabAt,
	type ServedTabs
} from './present';

const ALL: ServedTabs = {
	work: true,
	processTabs: true,
	features: true,
	releases: true,
	docs: true,
	settings: true
};
const noFeatures = { planned: 0, inProgress: 0, live: 0 };
const labels = (tabs: { label: string }[]) => tabs.map((t) => t.label);

describe('projectTabs (PKG-15)', () => {
	it('with no development process, Features and Releases sit between Work and Docs', () => {
		expect(
			labels(
				projectTabs(
					{ developmentProcess: null, processTabs: [], features: noFeatures, releases: 0 },
					ALL
				)
			)
		).toEqual([
			'Overview',
			'Work',
			'Features',
			'Releases',
			'Docs',
			'Team',
			'Activity',
			'Conversations',
			'Settings'
		]);
	});

	it("puts the process's tabs in order and adds Features only when held and not given", () => {
		const tabs = projectTabs(
			{
				developmentProcess: { id: 'p', name: 'DSS' },
				processTabs: [
					{ key: 'product', title: 'Product', view: 'documents' },
					{ key: 'releases', title: 'Releases', view: 'releases' }
				],
				features: { planned: 1, inProgress: 0, live: 0 },
				releases: 2
			},
			ALL
		);
		expect(labels(tabs)).toEqual([
			'Overview',
			'Work',
			'Product',
			'Releases',
			'Features',
			'Docs',
			'Team',
			'Activity',
			'Conversations',
			'Settings'
		]);
	});

	it('shows only the served tabs today (PRJ-1)', () => {
		const tabs = projectTabs({
			developmentProcess: null,
			processTabs: [],
			features: { planned: 1, inProgress: 0, live: 0 },
			releases: 1
		});
		expect(labels(tabs)).toEqual([
			'Overview',
			'Work',
			'Team',
			'Activity',
			'Conversations',
			'Settings'
		]);
	});

	it('finds the tab of a path, Overview for anything else', () => {
		const tabs = projectTabs({
			developmentProcess: null,
			processTabs: [],
			features: noFeatures,
			releases: 0
		});
		expect(tabAt('/projects/beacon/team', 'beacon', tabs).id).toBe('team');
		expect(tabAt('/projects/beacon/work/x', 'beacon', tabs).id).toBe('work');
		expect(tabAt('/projects/beacon', 'beacon', tabs).id).toBe('overview');
		expect(tabAt('/projects/beacon/teams', 'beacon', tabs).id).toBe('overview');
	});
});

describe('counts', () => {
	const t = { todo: 3, inProgress: 1, blocked: 1, done: 4, cancelled: 2 };
	it('says open and blocked, and done of the delivered-or-open total', () => {
		expect(openText(t)).toBe('5 open · 1 blocked');
		expect(openText({ ...t, blocked: 0, todo: 4 })).toBe('5 open');
		expect(openText({ todo: 0, inProgress: 0, blocked: 0, done: 2, cancelled: 0 })).toBe(
			'No open tasks'
		);
		expect(doneText(t)).toBe('4 of 9 tasks done');
		expect(doneText({ todo: 0, inProgress: 0, blocked: 0, done: 0, cancelled: 3 })).toBe(
			'No tasks yet'
		);
	});
	it('counts open bugs by severity with Not set last (PRJ-5)', () => {
		expect(bugsText({ critical: 1, major: 0, minor: 0, trivial: 0, unset: 2 })).toBe(
			'3 open bugs: 1 critical · 2 not set'
		);
		expect(bugsText({ critical: 0, major: 1, minor: 0, trivial: 0, unset: 0 })).toBe(
			'1 open bug: 1 major'
		);
		expect(bugsText({ critical: 0, major: 0, minor: 0, trivial: 0, unset: 0 })).toBe(
			'No open bugs'
		);
	});
	it('words the features line', () => {
		expect(featuresText({ planned: 1, inProgress: 2, live: 5 })).toBe(
			'2 in progress · 1 planned · 5 live'
		);
		expect(featuresText(noFeatures)).toBe('No features yet');
		// Wraps only after a dot, so no dot starts a line and no part splits.
		expect(keepWords('2 in progress · 1 planned · 5 live')).toBe(
			'2\u00a0in\u00a0progress\u00a0· 1\u00a0planned\u00a0· 5\u00a0live'
		);
	});
	it('sorts A–Z with archived projects last', () => {
		expect(
			sortProjects([
				{ name: 'beacon', archived: false },
				{ name: 'Atlas', archived: true },
				{ name: 'Zed', archived: false }
			]).map((p) => p.name)
		).toEqual(['beacon', 'Zed', 'Atlas']);
	});
	it('sorts project activity newest first with invalid timestamps last', () => {
		const input = [
			{ id: 'old', at: '2026-11-18T08:00:00Z' },
			{ id: 'bad', at: 'not-a-date' },
			{ id: 'newest', at: '2026-11-20T12:00:00Z' },
			{ id: 'mid', at: '2026-11-19T10:30:00Z' }
		];
		expect(sortActivityNewestFirst(input).map((r) => r.id)).toEqual([
			'newest',
			'mid',
			'old',
			'bad'
		]);
		// Pure: does not mutate the caller's array.
		expect(input.map((r) => r.id)).toEqual(['old', 'bad', 'newest', 'mid']);
	});
});

describe('fields', () => {
	it('suggests and checks slugs', () => {
		expect(slugFrom('  Onboarding kit — v2!  ')).toBe('onboarding-kit-v2');
		expect(slugFrom('Café Ünïcode')).toBe('cafe-unicode');
		expect(slugProblem('reservations')).toBeUndefined();
		expect(slugProblem('')).toMatch(/Enter a slug/);
		expect(slugProblem('Bad--slug')).toMatch(/lowercase/);
		expect(slugProblem('-x')).toMatch(/lowercase/);
	});
	it('checks integration branch names', () => {
		expect(branchProblem('main')).toBeUndefined();
		expect(branchProblem('release/1.4')).toBeUndefined();
		expect(branchProblem('')).toMatch(/Enter/);
		expect(branchProblem('a..b')).toMatch(/not a valid/);
		expect(branchProblem('has space')).toMatch(/not a valid/);
		expect(branchProblem('feature/')).toMatch(/not a valid/);
		expect(branchProblem('x.lock')).toMatch(/not a valid/);
	});
});

describe('briefBlocks', () => {
	it('reads headings, lists and paragraphs, never HTML', () => {
		expect(
			briefBlocks(
				'# Goal\nBook a table\nin two taps.\n\n- One\n- Two\n  more\n\n1. First\n<b>x</b>'
			)
		).toEqual([
			{ kind: 'heading', text: 'Goal' },
			{ kind: 'paragraph', text: 'Book a table in two taps.' },
			{ kind: 'list', ordered: false, items: ['One', 'Two more'] },
			{ kind: 'list', ordered: true, items: ['First <b>x</b>'] }
		]);
		expect(briefBlocks('  \n ')).toEqual([]);
	});
});
