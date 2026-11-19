import { describe, expect, it } from 'vitest';
import { treeMove } from './keyboard';
import {
	ancestorsOf,
	handOverCandidates,
	initiallyCollapsed,
	listText,
	managerChoices,
	reportingTree,
	reportsText,
	reportsUnder,
	topLevelTeam,
	visibleOrder
} from './tree';
import type { ChartEmployee, OrgChart } from './types';

const e = (id: string, name: string, extra: Partial<ChartEmployee> = {}): ChartEmployee => ({
	id,
	kind: 'person',
	name,
	handle: id,
	status: 'active',
	leads: [],
	canSignIn: false,
	revision: 1,
	...extra
});

const staff = [
	e('alex', 'Alex Kim', { canSignIn: true }),
	e('avery', 'Avery Stone', { canSignIn: true }),
	e('hana', 'Hana Weiss', { reportsToId: 'avery' }),
	e('leo', 'Leo Brandt', { reportsToId: 'hana', canSignIn: true }),
	e('sam', 'Sam Okafor', { kind: 'agent', reportsToId: 'avery' }),
	e('noor', 'Noor Haddad', { kind: 'agent', reportsToId: 'sam' })
];
const chart = (employees = staff): OrgChart => ({
	organization: { id: 'o', name: 'Lumen Labs' },
	ownerId: 'alex',
	viewerId: 'alex',
	employees
});
const names = (nodes: { employee: ChartEmployee }[]) => nodes.map((n) => n.employee.name);

describe('reportingTree', () => {
	it('draws explicit lines only, roots under No explicit manager, everyone once', () => {
		const tree = reportingTree(staff);
		expect(names(tree.roots)).toEqual(['Alex Kim', 'Avery Stone']);
		expect(names(tree.byId.get('avery')!.children)).toEqual(['Hana Weiss', 'Sam Okafor']);
		expect(tree.byId.get('noor')!.level).toBe(3);
		expect(tree.count).toBe(staff.length);
		expect(names(visibleOrder(tree, new Set()))).toHaveLength(staff.length);
	});

	it('keeps someone whose manager is not an active employee at the top, saying why', () => {
		const tree = reportingTree([
			e('a', 'Ana', { reportsToId: 'gone' }),
			e('b', 'Bo', { reportsToId: 'b' })
		]);
		expect(names(tree.roots)).toEqual(['Ana', 'Bo']);
		expect(tree.byId.get('a')!.problem).toBe("Recorded manager isn't in the organization");
		expect(tree.byId.get('b')!.problem).toBe('Recorded as reporting to themselves');
	});

	it('breaks a reporting loop at its first member by name and shows everyone once', () => {
		const tree = reportingTree([
			e('c', 'Cy', { reportsToId: 'a' }),
			e('a', 'Ana', { reportsToId: 'b' }),
			e('b', 'Bo', { reportsToId: 'c' }),
			e('d', 'Di', { reportsToId: 'b' })
		]);
		expect(names(tree.roots)).toEqual(['Ana']);
		expect(tree.byId.get('a')!.problem).toBe('Reporting line loops back through Bo and Cy');
		expect(names(visibleOrder(tree, new Set()))).toEqual(['Ana', 'Cy', 'Bo', 'Di']);
	});

	it('collapses below level two only in a large organization', () => {
		expect(initiallyCollapsed(reportingTree(staff))).toEqual([]);
		const big = [e('top', 'Top'), e('mid', 'Mid', { reportsToId: 'top' })];
		for (let i = 0; i < 50; i++) big.push(e(`x${i}`, `X ${i}`, { reportsToId: 'mid' }));
		expect(initiallyCollapsed(reportingTree(big))).toEqual(['mid']);
	});

	it('lists reports under someone and the managers above', () => {
		const tree = reportingTree(staff);
		expect([...reportsUnder(tree, 'avery')].sort()).toEqual(['hana', 'leo', 'noor', 'sam']);
		expect(ancestorsOf(tree, 'noor')).toEqual(['sam', 'avery']);
	});
});

describe('managerChoices', () => {
	it('omits the employee, keeps the owner, disables those who report to them', () => {
		const tree = reportingTree(staff);
		const choices = managerChoices(chart(), tree, 'avery');
		expect(choices.map((c) => c.employee.id)).not.toContain('avery');
		expect(choices.find((c) => c.employee.id === 'alex')?.disabled).toBeUndefined();
		expect(choices.find((c) => c.employee.id === 'noor')?.disabled).toBe('Reports to Avery Stone');
		expect(choices.find((c) => c.employee.id === 'leo')?.disabled).toBe('Reports to Avery Stone');
	});
});

describe('handOverCandidates', () => {
	it('offers active people with an account, never the owner or an agent', () => {
		const list = [...staff, e('pat', 'Pat', { canSignIn: true, status: 'paused' })];
		expect(handOverCandidates(chart(list)).map((c) => c.id)).toEqual(['avery', 'leo']);
	});
});

describe('treeMove', () => {
	const tree = reportingTree(staff);
	const none = new Set<string>();
	it('moves up and down through what is shown, and to the ends', () => {
		expect(treeMove('ArrowDown', 'alex', tree, none)).toEqual({ focus: 'avery' });
		expect(treeMove('ArrowUp', 'avery', tree, none)).toEqual({ focus: 'alex' });
		expect(treeMove('ArrowUp', 'alex', tree, none)).toBeUndefined();
		expect(treeMove('End', 'alex', tree, none)).toEqual({ focus: 'noor' });
		expect(treeMove('Home', 'noor', tree, none)).toEqual({ focus: 'alex' });
		expect(treeMove('ArrowDown', 'avery', tree, new Set(['avery']))).toBeUndefined();
	});
	it('opens and closes a manager, or goes to the first report and back to the manager', () => {
		expect(treeMove('ArrowRight', 'avery', tree, none)).toEqual({ focus: 'hana' });
		expect(treeMove('ArrowRight', 'avery', tree, new Set(['avery']))).toEqual({ expand: 'avery' });
		expect(treeMove('ArrowLeft', 'avery', tree, none)).toEqual({ collapse: 'avery' });
		expect(treeMove('ArrowLeft', 'noor', tree, none)).toEqual({ focus: 'sam' });
		expect(treeMove('ArrowRight', 'leo', tree, none)).toBeUndefined();
	});
});

it('words', () => {
	expect(listText(['Sam'])).toBe('Sam');
	expect(listText(['Sam', 'Noor', 'Kai'])).toBe('Sam, Noor and Kai');
	expect(reportsText(0)).toBeUndefined();
	expect(reportsText(1)).toBe('1 report');
	expect(reportsText(2)).toBe('2 reports');
});

it("topLevelTeam starts Add a person at the owner's top-level team", () => {
	const teams = [
		{ id: 'design' },
		{ id: 'eng' },
		{ id: 'web', owner_team_id: 'eng' },
		{ id: 'loopy', owner_team_id: 'loopy' }
	];
	expect(topLevelTeam(teams, 'web')).toBe('eng');
	expect(topLevelTeam(teams, undefined)).toBe('design');
	expect(topLevelTeam(teams, 'gone')).toBe('design');
	expect(topLevelTeam([], 'web')).toBeUndefined();
	expect(topLevelTeam(teams, 'loopy')).toBe('loopy');
});
