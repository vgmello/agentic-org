// Reporting lines and the People view, as pure functions over an `OrgSnapshot`. Reporting lines are
// separate from teams: nothing here reads a team, and no team change moves anyone (REPORT-5).
import type { Member, OrgSnapshot, Team } from '$lib/proto';
import { byName, leadOf, membersOfTeam, nonOwners, owner, peopleOf } from './model';

export interface ReportNode {
	member: Member;
	children: ReportNode[];
}

/**
 * Who reports to whom, with the owner at the root. Anyone without a manager hangs directly under the
 * owner, and so does anyone whose manager is missing or who is caught in a loop, so everyone appears
 * exactly once (REPORT-1). Returns undefined when there is no owner.
 */
export function reportingTree(s: OrgSnapshot): ReportNode | undefined {
	const root = owner(s);
	if (!root) return undefined;
	const ids = new Set(s.members.map((m) => m.id));
	const placed = new Set<string>([root.id]);
	const managerKey = (m: Member) =>
		m.reports_to_member_id && ids.has(m.reports_to_member_id) && m.reports_to_member_id !== m.id ? m.reports_to_member_id : root.id;
	const byManager = new Map<string, Member[]>();
	for (const m of nonOwners(s)) byManager.set(managerKey(m), [...(byManager.get(managerKey(m)) ?? []), m]);

	const build = (m: Member): ReportNode => ({
		member: m,
		children: (byManager.get(m.id) ?? [])
			.filter((c) => !placed.has(c.id) && placed.add(c.id))
			.sort(byName)
			.map(build)
	});
	const tree = build(root);
	// Anyone left hangs below a loop with no way in from the owner. Walk up from them to the loop and break
	// it at its lowest name: that member goes under the owner with everyone below them, so nobody is lost
	// and nobody's subtree is flattened.
	const byId = new Map(s.members.map((m) => [m.id, m]));
	const loopRoot = (start: Member): Member => {
		const path: Member[] = [];
		let cur: Member | undefined = start;
		while (cur && !path.includes(cur)) (path.push(cur), (cur = byId.get(managerKey(cur))));
		return (cur ? path.slice(path.indexOf(cur)) : path).sort(byName)[0];
	};
	for (const m of nonOwners(s).sort(byName)) {
		if (placed.has(m.id)) continue;
		const top = loopRoot(m);
		placed.add(top.id);
		tree.children.push(build(top));
	}
	return tree;
}

/** Everyone in the tree, root first, each once. */
export function flattenReporting(node: ReportNode): Member[] {
	return [node.member, ...node.children.flatMap(flattenReporting)];
}

export const directReports = (node: ReportNode) => node.children.length;

/**
 * Everyone who reports to `memberId`, directly or through others (REPORT-3). Walks the reports-to links
 * themselves, not the drawn tree, so it is right even without an owner or inside a loop.
 */
export function allReportsOf(s: OrgSnapshot, memberId: string): Member[] {
	const direct = new Map<string, Member[]>();
	for (const m of s.members) {
		if (m.owner || !m.reports_to_member_id || m.reports_to_member_id === m.id) continue;
		direct.set(m.reports_to_member_id, [...(direct.get(m.reports_to_member_id) ?? []), m]);
	}
	const seen = new Set<string>([memberId]);
	const found: Member[] = [];
	const walk = (id: string) => {
		for (const r of direct.get(id) ?? []) if (!seen.has(r.id)) (seen.add(r.id), found.push(r), walk(r.id));
	};
	walk(memberId);
	return found.sort(byName);
}

/** A member's manager: the person or agent they report to, or the owner when they have none. */
export function managerOf(s: OrgSnapshot, member: Member): Member | undefined {
	if (member.owner) return undefined;
	const named = s.members.find((m) => m.id === member.reports_to_member_id && m.id !== member.id);
	return named ?? owner(s);
}

export interface ManagerOption {
	member: Member;
	disabled: boolean;
	/** Why it can't be picked. */
	reason?: string;
}

/**
 * The choices in "Change manager…" for `memberId` (REPORT-3, OWNER-3): the owner first, then everyone
 * else A–Z, never the member themselves, and anyone who reports to them, directly or through others,
 * disabled with the reason (picking one would make a loop). The owner reports to no one: no options.
 */
export function managerOptions(s: OrgSnapshot, memberId: string): ManagerOption[] {
	const me = s.members.find((m) => m.id === memberId);
	if (!me || me.owner) return [];
	const below = new Set(allReportsOf(s, memberId).map((m) => m.id));
	const options = s.members
		.filter((m) => m.id !== memberId)
		.map((m): ManagerOption => (below.has(m.id) ? { member: m, disabled: true, reason: `${m.name} reports to ${me.name}, directly or through others` } : { member: m, disabled: false }));
	return [...options.filter((o) => o.member.owner), ...options.filter((o) => !o.member.owner).sort((a, b) => byName(a.member, b.member))];
}

// ---- the People view -----------------------------------------------------------------------------

export interface PeopleCard {
	team: Team;
	people: Member[];
	/** The team's lead, a person or an agent. An agent appears on this card only to name the lead (PEOPLE-1). */
	lead?: Member;
}

/** What a team card shows in the People view: only people, plus the lead when the lead is an agent. */
export function peopleCard(s: OrgSnapshot, team: Team): PeopleCard {
	const lead = leadOf(s, team);
	return {
		team,
		people: peopleOf(membersOfTeam(s, team.id)),
		...(lead ? { lead } : {})
	};
}
