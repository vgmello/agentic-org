import { describe, expect, it } from 'vitest';
import type { Member } from '$lib/proto';
import { SAMPLE_SCAN } from './mock';
import { buildRows, chooseRole, takesPart, chooseTitle, counts, handleProblem, hasNoMessages, isReady, isValidHandle, missingOf, OWNER, roleWords, suggestRole } from './rows';
import type { HrAgent, Scan } from './types';

const rowsOf = (scan: Scan = SAMPLE_SCAN, exclude = ['sample']) => buildRows(scan, { exclude });
const byName = (name: string) => rowsOf().rows.find((r) => r.name === name)!;

describe('IMP-2 one row per agent, merged across the sources by name', () => {
	it('merges an HR record with its inbox, and keeps the single-source agents as they are', () => {
		const { rows } = rowsOf();
		expect(rows).toHaveLength(10);
		expect(byName('ada').source).toBe('both');
		expect(byName('server-dev').source).toBe('messaging');
		expect(byName('Riley').source).toBe('hr');
		expect(rows.map((r) => r.messagingName)).toContain('ac-staff-engineer-ada');
	});

	it('leaves out the owner’s own inbox, the reserved handles and the other capitals of the same name', () => {
		const scan: Scan = { messaging: { path: '/s', names: ['sample', 'Owner', 'hr', 'office', 'Reviewer', 'reviewer'] } };
		expect(buildRows(scan, { exclude: ['Sample'] }).rows.map((r) => r.messagingName)).toEqual(['Reviewer']);
	});

	it('ignores a source that could not be read and still lists the other', () => {
		const scan: Scan = { hr: { path: '/h', error: 'no', agents: [], roles: [], skipped: [] }, messaging: SAMPLE_SCAN.messaging };
		expect(buildRows(scan, { exclude: ['sample'] }).rows).toHaveLength(9);
	});

	it('lists agents offboarded in HR apart, never as rows', () => {
		const scan: Scan = { hr: { ...SAMPLE_SCAN.hr!, agents: [...SAMPLE_SCAN.hr!.agents, { id: 'ac-gone', status: 'offboarded' }] } };
		const review = buildRows(scan);
		expect(review.offboarded.map((a) => a.id)).toEqual(['ac-gone']);
		expect(review.rows.some((r) => r.messagingName === 'ac-gone')).toBe(false);
	});

	it('marks an agent that is already in the organization, whatever the capitals', () => {
		const member = { id: 'm1', kind: 'agent', agent: { messaging_name: 'Server-Dev' } } as unknown as Member;
		const { rows } = buildRows(SAMPLE_SCAN, { exclude: ['sample'], members: [member] });
		expect(rows.find((r) => r.messagingName === 'server-dev')?.existingMemberId).toBe('m1');
		expect(counts(rows).alreadyIn).toBe(1);
	});
});

describe('what a row takes from HR', () => {
	it('reads role, title, runtime (Claude Code), home and department, and drops "inherit" as no model', () => {
		const ada = byName('ada');
		expect(ada).toMatchObject({
			role: 'staff engineer',
			title: 'Staff Engineer',
			runtime: 'claude_code',
			teamKey: 'Engineering',
			home: '/Users/sample/projects/team/agents/projects/ac/agents/ac-staff-engineer-ada'
		});
		expect(ada.model).toBeUndefined();
	});

	it('follows HR’s reports_to to another row, or to the owner; messaging-only agents report to the owner', () => {
		expect(byName('ada').managerKey).toBe(OWNER);
		expect(byName('River').managerKey).toBe('ac-staff-engineer-ada');
		expect(byName('server-dev').managerKey).toBe(OWNER);
	});

	it('words a role slug', () => {
		expect(roleWords('product-designer')).toBe('product designer');
		expect(roleWords('')).toBeUndefined();
	});
});

describe('IMP-3 Bring in counts only the rows that are complete', () => {
	it('says what a messaging-only agent still needs, and what Bring in counts', () => {
		expect(missingOf(byName('server-dev'))).toEqual(['role', 'runtime']);
		expect(missingOf(byName('ada'))).toEqual([]);
		expect(counts(rowsOf().rows)).toEqual({ ready: 5, needDetails: 5, alreadyIn: 0, paused: 0 });
	});

	it('counts a row once it is filled in, and not a skipped one', () => {
		const { rows } = rowsOf();
		const server = rows.find((r) => r.name === 'server-dev')!;
		Object.assign(server, { role: 'software engineer', title: 'software engineer', runtime: 'codex' });
		expect(isReady(server)).toBe(true);
		expect(counts(rows).ready).toBe(6);
		server.choice = 'skip';
		expect(counts(rows)).toMatchObject({ ready: 5, needDetails: 4 });
	});

	it('counts the rows brought in paused', () => {
		const { rows } = rowsOf();
		rows.find((r) => r.name === 'Riley')!.choice = 'paused';
		expect(counts(rows)).toMatchObject({ ready: 5, paused: 1 });
	});
});

describe('IMP-4 an HR agent with no messaging inbox', () => {
	it('is told so only when the messaging source was read', () => {
		const riley = byName('Riley');
		expect(hasNoMessages(riley, SAMPLE_SCAN)).toBe(true);
		expect(hasNoMessages(byName('ada'), SAMPLE_SCAN)).toBe(false);
		expect(hasNoMessages(riley, { hr: SAMPLE_SCAN.hr })).toBe(false);
	});
});

describe('records that need care', () => {
	const hr = (agents: HrAgent[]): Scan => ({ hr: { path: '/h', agents, roles: [], skipped: [] } });

	it('does not bring in an agent offboarded in HR even when its inbox is still there', () => {
		const scan: Scan = { ...hr([{ id: 'ac-gone', status: 'offboarded' }]), messaging: { path: '/s', names: ['ac-gone', 'ac-here'] } };
		expect(buildRows(scan).rows.map((r) => r.messagingName)).toEqual(['ac-here']);
		expect(buildRows(scan).offboarded).toHaveLength(1);
	});

	it('merges the same HR id in other capitals or in two projects into one agent, the first record winning', () => {
		const { rows } = buildRows(hr([{ id: 'Ada', title: 'First' }, { id: 'ada', title: 'Second' }]));
		expect(rows).toHaveLength(1);
		expect(rows[0].title).toBe('First');
	});

	it('merges an HR record with an inbox whose name differs only in capitals', () => {
		const scan: Scan = { ...hr([{ id: 'Foo', title: 'T' }]), messaging: { path: '/s', names: ['foo'] } };
		const { rows } = buildRows(scan);
		expect(rows).toHaveLength(1);
		expect(rows[0]).toMatchObject({ source: 'both', messagingName: 'foo' });
	});

	it('does not make a row of an HR record whose id is a reserved handle or the owner’s', () => {
		const { rows } = buildRows(hr([{ id: 'owner' }, { id: 'HR' }, { id: 'vgmello' }, { id: 'ac-ok' }]), { exclude: ['vgmello'] });
		expect(rows.map((r) => r.key)).toEqual(['ac-ok']);
	});

	it('breaks a reporting cycle: the row that closes it reports to the owner', () => {
		const { rows } = buildRows(hr([{ id: 'a', reports_to: 'b' }, { id: 'b', reports_to: 'a' }, { id: 'c', reports_to: 'a' }]));
		const manager = (key: string) => rows.find((r) => r.key === key)!.managerKey;
		// Whatever row was chosen to close it, nobody is left reporting in a circle.
		for (const row of rows) {
			const seen = new Set<string>();
			for (let r: typeof row | undefined = row; r; r = rows.find((x) => x.key === r!.managerKey)) {
				expect(seen.has(r.key)).toBe(false);
				seen.add(r.key);
			}
		}
		expect([manager('a'), manager('b')]).toContain(OWNER);
		expect(manager('c')).toBe('a');
	});

	it('keeps the line of an agent that only reports into a cycle, and cuts just the row that closes it', () => {
		// a sorts first and reports to b; b and c report to each other.
		const { rows } = buildRows(hr([{ id: 'a', reports_to: 'b' }, { id: 'b', reports_to: 'c' }, { id: 'c', reports_to: 'b' }]));
		const manager = (key: string) => rows.find((r) => r.key === key)!.managerKey;
		expect(manager('a')).toBe('b');
		expect([manager('b'), manager('c')].filter((m) => m === OWNER)).toHaveLength(1);
	});
});

describe('a messaging name HQ would refuse is not ready', () => {
	it('mirrors org-proto’s handle rules', () => {
		for (const ok of ['ada', 'ac-staff-engineer-ada', 'a.b_c-d', '9lives', 'x'.repeat(64)]) expect(isValidHandle(ok)).toBe(true);
		for (const bad of ['', '-dash', 'has space', 'run:x', 'Ünïcode', 'x'.repeat(65), '01ARZ3NDEKTSV4RRFFQ69G5FAV', '.dot']) expect(isValidHandle(bad)).toBe(false);
	});

	it('asks for a name instead of counting the row as ready', () => {
		const { rows } = buildRows({ hr: { path: '/h', agents: [{ id: 'has space', role: 'x', title: 'y' }], roles: [], skipped: [] } });
		expect(missingOf(rows[0])).toEqual(['name']);
		expect(counts(rows)).toMatchObject({ ready: 0, needDetails: 1 });
	});
});

describe('IMP-3 (rev 14) role, title and messaging name', () => {
	const roles = SAMPLE_SCAN.hr!.roles;

	it('says why a messaging name is refused, in plain words', () => {
		expect(handleProblem('ada')).toBeUndefined();
		expect(handleProblem('x'.repeat(65))).toBe('Messaging names are at most 64 characters.');
		expect(handleProblem('has space')).toMatch(/letters, digits/);
		expect(handleProblem('01ARZ3NDEKTSV4RRFFQ69G5FAV')).toBe('This looks like a member id. Choose another name.');
		expect(handleProblem('Owner')).toBe('Owner is reserved.');
		expect(handleProblem('office')).toMatch(/reserved/);
	});

	it('the title follows the chosen role until the owner gives their own', () => {
		const row = { ...rowsOf().rows.find((r) => r.name === 'server-dev')! };
		const withRole = chooseRole(row, 'software engineer');
		expect(withRole).toMatchObject({ role: 'software engineer', title: 'Software Engineer' });
		const custom = chooseTitle(withRole, 'Senior Backend Engineer', roles);
		expect(custom.title).toBe('Senior Backend Engineer');
		expect(chooseRole(custom, 'staff engineer')).toMatchObject({ role: 'staff engineer', title: 'Senior Backend Engineer' });
	});

	it('suggests the catalog role closest to a custom title when there is one clear winner, marked Suggested, and counts it as filled', () => {
		expect(suggestRole('Staff Engineer', roles)).toBe('staff engineer');
		expect(suggestRole('Senior Backend Engineer', roles)).toBeUndefined(); // several roles share "engineer"
		expect(suggestRole('Chef', roles)).toBeUndefined();
		const row = { ...rowsOf().rows.find((r) => r.name === 'server-dev')!, runtime: 'codex' as const };
		const custom = chooseTitle(row, 'Product Designer', roles);
		expect(custom).toMatchObject({ role: 'product designer', roleSuggested: true });
		expect(missingOf(custom)).toEqual([]);
		// With no match the role stays empty and the row still needs details.
		expect(missingOf(chooseTitle(row, 'Chef', roles))).toEqual(['role']);
	});

	it('does not replace a role the owner already chose', () => {
		const row = chooseRole({ ...rowsOf().rows.find((r) => r.name === 'server-dev')! }, 'qa engineer');
		expect(chooseTitle(row, 'Staff Engineer', roles).role).toBe('qa engineer');
	});

	it('treats a person already in the organization with the same name as taken, as HQ does (the owner apart)', () => {
		const people = [
			{ id: 'p1', kind: 'person', name: 'Server-Dev', owner: false },
			{ id: 'me', kind: 'person', name: 'codex-dev', owner: true }
		] as unknown as import('$lib/proto').Member[];
		const { rows } = buildRows(SAMPLE_SCAN, { exclude: ['sample'], members: people });
		expect(rows.find((r) => r.messagingName === 'server-dev')).toMatchObject({ existingMemberId: 'p1', existingKind: 'person' });
		expect(rows.find((r) => r.messagingName === 'codex-dev')?.existingMemberId).toBeUndefined();
	});
});

describe('what HQ would refuse in HR’s text', () => {
	const one = (over: Record<string, unknown>) => buildRows({ hr: { path: '/h', agents: [{ id: 'ac-x', role: 'software-engineer', title: 'Engineer', ...over }], roles: [], skipped: [] } }).rows[0];

	it('takes control characters out of names and titles, and trims the way HQ does', () => {
		const row = one({ name: ' Ada\u0007 L\n ', title: 'Staff\u0000 Engineer', focus: 'a\tb' });
		expect(row.name).toBe('Ada  L');
		expect(row.title).toBe('Staff  Engineer');
		expect(row.focus).toBe('a b');
	});

	it('cuts focus and model to 200 characters by character, never through an emoji', () => {
		const row = one({ focus: '😀'.repeat(300), model: 'm'.repeat(300) });
		expect([...row.focus!].length).toBe(200);
		expect(row.focus!.endsWith('…')).toBe(true);
		expect([...row.focus!].every((c) => c === '😀' || c === '…')).toBe(true);
		expect([...row.model!].length).toBe(200);
	});

	it('asks for a role when the title is longer than the record allows', () => {
		expect(missingOf({ ...one({}), runtime: 'codex' })).toEqual([]);
		expect(missingOf({ ...one({ title: 't'.repeat(201) }), runtime: 'codex' })).toEqual(['role']);
	});

	it('does not take an agent offboarded in the organization into the team, and says so', () => {
		const member = { id: 'm1', kind: 'agent', name: 'x', agent: { messaging_name: 'ac-x', status: 'offboarded' } } as unknown as Member;
		const row = buildRows({ hr: { path: '/h', agents: [{ id: 'ac-x', role: 'r', title: 't' }], roles: [], skipped: [] } }, { members: [member] }).rows[0];
		expect(row).toMatchObject({ existingMemberId: 'm1', existingOffboarded: true });
		expect(takesPart(row)).toBe(false);
	});

	it('takes control characters out of a typed title', () => {
		const row = chooseTitle({ ...rowsOf().rows.find((r) => r.name === 'server-dev')! }, 'Staff\u0007 Engineer\n', SAMPLE_SCAN.hr!.roles);
		expect(row.title).toBe('Staff  Engineer');
	});
});
