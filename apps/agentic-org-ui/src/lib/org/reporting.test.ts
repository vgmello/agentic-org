import { describe, expect, it } from 'vitest';
import { memberById, notInATeam } from './model';
import { allReportsOf, directReports, flattenReporting, managerOf, managerOptions, peopleCard, reportingTree } from './reporting';
import { emptyOrg, sampleOrg } from './sample';

const s = sampleOrg('office-1');
const tree = reportingTree(s)!;
const find = (id: string) => memberById(s, id)!;

describe('REPORT-1 the Reporting lines tree', () => {
	it('has the owner at the root and everyone exactly once', () => {
		expect(tree.member.owner).toBe(true);
		const ids = flattenReporting(tree).map((m) => m.id);
		expect(new Set(ids).size).toBe(ids.length);
		expect(ids.sort()).toEqual(s.members.map((m) => m.id).sort());
	});

	it('anyone without a manager hangs directly under the owner', () => {
		// Quinn has no manager in the sample.
		expect(tree.children.map((c) => c.member.id)).toContain('quinn');
		expect(managerOf(s, find('quinn'))?.owner).toBe(true);
	});

	it('follows reports_to: Noor reports to Sam, who reports to Avery', () => {
		expect(managerOf(s, find('noor'))?.id).toBe('sam');
		expect(managerOf(s, find('sam'))?.id).toBe('avery');
		const avery = tree.children.find((c) => c.member.id === 'avery')!;
		expect(avery.children.map((c) => c.member.name)).toEqual(['Hana Weiss', 'Juno Park', 'Sam Okafor']);
	});

	it('still draws everyone once if data has a loop, a missing manager, or a self-report', () => {
		const odd = {
			...s,
			members: s.members.map((m) =>
				m.id === 'sam' ? { ...m, reports_to_member_id: 'noor' } // a loop: sam -> noor -> sam
				: m.id === 'kai' ? { ...m, reports_to_member_id: 'ghost' }
				: m.id === 'quinn' ? { ...m, reports_to_member_id: 'quinn' }
				: m
			)
		};
		const t = reportingTree(odd)!;
		const ids = flattenReporting(t).map((m) => m.id);
		expect(new Set(ids).size).toBe(ids.length);
		expect(ids.length).toBe(odd.members.length);
		const direct = t.children.map((c) => c.member.id);
		expect(direct).toContain('kai'); // a missing manager means the owner
		expect(direct).toContain('quinn'); // so does a self-report
	});

	it('a member with a missing manager keeps the people who report to them', () => {
		const odd = { ...s, members: s.members.map((m) => (m.id === 'sam' ? { ...m, reports_to_member_id: 'ghost' } : m)) };
		const sam = reportingTree(odd)!.children.find((c) => c.member.id === 'sam')!;
		expect(sam.children.map((c) => c.member.id).sort()).toEqual(['kai', 'mira', 'noor']);
	});

	it('a loop is broken at the lowest name and its subtree stays together', () => {
		// sam <-> noor report to each other; kai and mira report to sam. Nothing reaches the owner.
		const odd = {
			...s,
			members: s.members.map((m) => (m.id === 'sam' ? { ...m, reports_to_member_id: 'noor' } : m.id === 'noor' ? { ...m, reports_to_member_id: 'sam' } : m))
		};
		const t = reportingTree(odd)!;
		const noor = t.children.find((c) => c.member.id === 'noor')!; // lowest name of the loop
		expect(noor.children.map((c) => c.member.id)).toEqual(['sam']);
		expect(noor.children[0].children.map((c) => c.member.id).sort()).toEqual(['kai', 'mira']);
		expect(flattenReporting(t).length).toBe(odd.members.length);
	});

	it('is undefined without an owner, and just the owner for an empty organization', () => {
		expect(reportingTree({ ...s, members: s.members.filter((m) => !m.owner) })).toBeUndefined();
		expect(flattenReporting(reportingTree(emptyOrg())!).map((m) => m.id)).toEqual(['you']);
	});
});

describe('REPORT-2 each node shows its number of reports', () => {
	it('counts direct reports; all reports are available too', () => {
		const sam = tree.children.find((c) => c.member.id === 'avery')!.children.find((c) => c.member.id === 'sam')!;
		expect(directReports(sam)).toBe(3);
		expect(allReportsOf(s, 'avery').map((m) => m.id).sort()).toEqual(['hana', 'ines', 'juno', 'kai', 'leo', 'mira', 'noor', 'pax', 'sam']);
		expect(allReportsOf(s, 'kai')).toEqual([]);
	});
});

describe('REPORT-3 the manager choices stay right when the data is odd', () => {
	it('without an owner, people below a member are still disabled', () => {
		const noOwner = { ...s, members: s.members.filter((m) => !m.owner) };
		const disabled = managerOptions(noOwner, 'sam').filter((o) => o.disabled).map((o) => o.member.id).sort();
		expect(disabled).toEqual(['kai', 'mira', 'noor']);
	});

	it('inside a loop, the other member of the loop is disabled', () => {
		const loop = { ...s, members: s.members.map((m) => (m.id === 'sam' ? { ...m, reports_to_member_id: 'noor' } : m.id === 'noor' ? { ...m, reports_to_member_id: 'sam' } : m)) };
		expect(managerOptions(loop, 'sam').find((o) => o.member.id === 'noor')?.disabled).toBe(true);
	});
});

describe('REPORT-3 and OWNER-3 the manager choices', () => {
	it('lists the owner first, never the member, and disables everyone below them with the reason', () => {
		const options = managerOptions(s, 'sam');
		expect(options[0].member.owner).toBe(true);
		expect(options.some((o) => o.member.id === 'sam')).toBe(false);
		const disabled = options.filter((o) => o.disabled).map((o) => o.member.id).sort();
		expect(disabled).toEqual(['kai', 'mira', 'noor']);
		expect(options.find((o) => o.member.id === 'noor')?.reason).toMatch(/reports to Sam Okafor/);
		expect(options.filter((o) => !o.member.owner && !o.disabled).map((o) => o.member.name)).toEqual([
			'Avery Stone', 'Hana Weiss', 'Inês Duarte', 'Juno Park', 'Leo Brandt', 'Pax Moreau', 'Quinn Adeyemi'
		]);
	});

	it('the owner reports to no one: no options, and no manager', () => {
		expect(managerOptions(s, 'you')).toEqual([]);
		expect(managerOf(s, find('you'))).toBeUndefined();
		expect(managerOptions(s, 'ghost')).toEqual([]);
	});

	it('picking an enabled option never creates a loop', () => {
		for (const m of s.members.filter((x) => !x.owner)) {
			const below = new Set(allReportsOf(s, m.id).map((x) => x.id));
			for (const o of managerOptions(s, m.id).filter((x) => !x.disabled)) expect(below.has(o.member.id)).toBe(false);
		}
	});
});

describe('REPORT-5 reporting lines are separate from teams', () => {
	it('moving someone between teams changes nobody’s manager', () => {
		const moved = { ...s, memberships: s.memberships.filter((m) => m.member_id !== 'noor') };
		expect(managerOf(moved, memberById(moved, 'noor')!)?.id).toBe('sam');
		expect(flattenReporting(reportingTree(moved)!).map((m) => m.id).sort()).toEqual(flattenReporting(tree).map((m) => m.id).sort());
		expect(notInATeam(moved).map((m) => m.id)).toContain('noor'); // but teams did change
	});

	it('changing a team’s lead changes nobody’s manager', () => {
		const relead = { ...s, teams: s.teams.map((t) => (t.id === 'web' ? { ...t, lead_member_id: 'mira' } : t)) };
		expect(managerOf(relead, memberById(relead, 'mira')!)?.id).toBe('sam');
		expect(managerOf(relead, memberById(relead, 'noor')!)?.id).toBe('sam');
	});
});

describe('PEOPLE-1 the People view cards', () => {
	it('shows only people, and an agent only when it is the lead', () => {
		const people = peopleCard(s, s.teams.find((t) => t.id === 'people')!);
		expect(people.people.map((m) => m.name)).toEqual(['Hana Weiss', 'Leo Brandt']);
		expect(people.lead?.name).toBe('Hana Weiss'); // a person who leads the team is named too
		const eng = peopleCard(s, s.teams.find((t) => t.id === 'eng')!);
		expect(eng.people).toEqual([]);
		expect(eng.lead?.name).toBe('Sam Okafor'); // an agent lead is named
		const web = peopleCard(s, s.teams.find((t) => t.id === 'web')!);
		expect(web.people).toEqual([]);
		expect(web.lead?.name).toBe('Noor Haddad');
		expect(peopleCard(s, { ...s.teams[0], lead_member_id: undefined }).lead).toBeUndefined();
	});
});
