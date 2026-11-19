// The reporting tree (offices-and-teams-ux §7.8, REPORT-1): drawn only from explicit `reportsToId`, separate from
// the team hierarchy. Employees with no explicit manager are the roots, grouped under "No explicit manager"; no
// manager is invented. Every employee appears exactly once, even when a record is wrong: a manager who isn't an
// active employee, or a reporting loop, puts the employee under No explicit manager with the problem in words.
import type { ChartEmployee, OrgChart } from './types';

export interface TreeNode {
	employee: ChartEmployee;
	/** 1 for the roots under No explicit manager. */
	level: number;
	children: TreeNode[];
	/** Why a record that names a manager is shown at the top: said beside it, never hidden. */
	problem?: string;
	/** The manager's id, undefined at the top. */
	parentId?: string;
}

export interface ReportingTree {
	roots: TreeNode[];
	byId: Map<string, TreeNode>;
	/** How many nodes the tree has: every employee, once. */
	count: number;
}

/** Above this many nodes, levels below two start collapsed (§7.10, large organization). */
export const LARGE_TREE = 50;

const byName = (a: ChartEmployee, b: ChartEmployee) =>
	a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) || a.id.localeCompare(b.id);

export function reportingTree(employees: readonly ChartEmployee[]): ReportingTree {
	const known = new Map(employees.map((e) => [e.id, e]));
	const parent = new Map<string, string>();
	const problems = new Map<string, string>();
	for (const e of employees) {
		if (!e.reportsToId) continue;
		if (e.reportsToId === e.id) problems.set(e.id, 'Recorded as reporting to themselves');
		else if (!known.has(e.reportsToId))
			problems.set(e.id, "Recorded manager isn't in the organization");
		else parent.set(e.id, e.reportsToId);
	}
	// A loop has no top: break each one at its first member by name, and say so there.
	const done = new Set<string>();
	for (const e of [...employees].sort(byName)) {
		const path: string[] = [];
		const onPath = new Set<string>();
		let cur: string | undefined = e.id;
		while (cur && !done.has(cur) && !onPath.has(cur)) {
			path.push(cur);
			onPath.add(cur);
			cur = parent.get(cur);
		}
		if (cur && onPath.has(cur)) {
			const loop = path.slice(path.indexOf(cur)).map((id) => known.get(id)!);
			const first = [...loop].sort(byName)[0];
			parent.delete(first.id);
			const others = loop.filter((x) => x.id !== first.id).map((x) => x.name);
			problems.set(first.id, `Reporting line loops back through ${listText(others)}`);
		}
		for (const id of path) done.add(id);
	}

	const byId = new Map<string, TreeNode>();
	for (const e of employees)
		byId.set(e.id, {
			employee: e,
			level: 0,
			children: [],
			problem: problems.get(e.id),
			parentId: parent.get(e.id)
		});
	const roots: TreeNode[] = [];
	for (const node of byId.values()) {
		const up = node.parentId ? byId.get(node.parentId) : undefined;
		(up ? up.children : roots).push(node);
	}
	const sortNodes = (nodes: TreeNode[], level: number) => {
		nodes.sort((a, b) => byName(a.employee, b.employee));
		for (const n of nodes) {
			n.level = level;
			sortNodes(n.children, level + 1);
		}
	};
	sortNodes(roots, 1);
	return { roots, byId, count: byId.size };
}

/** "Sam", "Sam and Noor", "Sam, Noor and Kai". */
export function listText(names: readonly string[]): string {
	if (names.length <= 1) return names[0] ?? '';
	return `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`;
}

/** Everyone who reports to `id`, directly or below. */
export function reportsUnder(tree: ReportingTree, id: string): Set<string> {
	const out = new Set<string>();
	const walk = (n: TreeNode) => {
		for (const c of n.children) {
			out.add(c.employee.id);
			walk(c);
		}
	};
	const node = tree.byId.get(id);
	if (node) walk(node);
	return out;
}

/** In a large organization nodes below level two start collapsed; a small one starts fully open. */
export function initiallyCollapsed(tree: ReportingTree): string[] {
	if (tree.count <= LARGE_TREE) return [];
	return [...tree.byId.values()]
		.filter((n) => n.level >= 2 && n.children.length > 0)
		.map((n) => n.employee.id);
}

/** The nodes a person can reach now, in drawing order: a collapsed node's reports are skipped. */
export function visibleOrder(tree: ReportingTree, collapsed: ReadonlySet<string>): TreeNode[] {
	const out: TreeNode[] = [];
	const walk = (nodes: TreeNode[]) => {
		for (const n of nodes) {
			out.push(n);
			if (!collapsed.has(n.employee.id)) walk(n.children);
		}
	};
	walk(tree.roots);
	return out;
}

/** The managers who must be open for `id` to be shown. */
export function ancestorsOf(tree: ReportingTree, id: string): string[] {
	const out: string[] = [];
	const seen = new Set<string>();
	for (
		let up = tree.byId.get(id)?.parentId;
		up && !seen.has(up);
		up = tree.byId.get(up)?.parentId
	) {
		seen.add(up);
		out.push(up);
	}
	return out;
}

export interface ManagerChoice {
	employee: ChartEmployee;
	/** "Reports to Nadia": choosing them would make a loop (REPORT-3). */
	disabled?: string;
}

/**
 * Who `employeeId` can report to (REPORT-3): every active employee except themselves, people and agents alike, the
 * owner's person included. Those who report to them, directly or below, are listed but disabled with the reason.
 */
export function managerChoices(chart: OrgChart, tree: ReportingTree, employeeId: string) {
	const self = chart.employees.find((e) => e.id === employeeId);
	const under = reportsUnder(tree, employeeId);
	return chart.employees
		.filter((e) => e.id !== employeeId)
		.sort(byName)
		.map((e): ManagerChoice =>
			under.has(e.id)
				? { employee: e, disabled: `Reports to ${self?.name ?? 'them'}` }
				: { employee: e }
		);
}

/** Who can take over ownership (§7.1): active people with an account, other than the owner. */
export const handOverCandidates = (chart: OrgChart): ChartEmployee[] =>
	chart.employees
		.filter(
			(e) => e.kind === 'person' && e.canSignIn && e.status === 'active' && e.id !== chart.ownerId
		)
		.sort(byName);

/** "1 report", "3 reports"; nothing at zero. */
export const reportsText = (n: number): string | undefined =>
	n === 0 ? undefined : n === 1 ? '1 report' : `${n} reports`;

/**
 * The top-level team Add a person starts at (§7.4): the one the owner's home team sits in, else the first team with no
 * parent. Undefined when there are no teams.
 */
export function topLevelTeam(
	teams: readonly { id: string; owner_team_id?: string }[],
	fromTeamId: string | undefined
): string | undefined {
	const known = (id: string | undefined) => !!id && teams.some((t) => t.id === id);
	const seen = new Set<string>();
	let id = known(fromTeamId) ? fromTeamId : undefined;
	while (id && !seen.has(id)) {
		seen.add(id);
		const parent = teams.find((t) => t.id === id)?.owner_team_id;
		if (!known(parent)) return id;
		id = parent;
	}
	return id ?? teams.find((t) => !known(t.owner_team_id))?.id;
}
