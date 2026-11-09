import { describe, expect, it } from 'vitest';
import { resolve } from './resolve';
import type { ProcessDoc, ProjectRef, TeamRef } from './types';

// Small worlds, one rule each: Eng ← Web ← Ui; Eng ← Plat; Des. Files are made here so each test says what it needs.
const TEAMS: TeamRef[] = [
	{ id: 'eng', name: 'Eng' },
	{ id: 'web', name: 'Web', owner_team_id: 'eng' },
	{ id: 'ui', name: 'Ui', owner_team_id: 'web' },
	{ id: 'plat', name: 'Plat', owner_team_id: 'eng' },
	{ id: 'des', name: 'Des' }
];
let n = 0;
const file = (level: ProcessDoc['level'], scope: string | undefined, over: Partial<ProcessDoc> = {}): ProcessDoc => {
	n += 1;
	return { id: `doc-${n}`, key: 'x', name: `X ${level}${scope ? ` ${scope}` : ''}`, level, ...(scope ? { scope } : {}), location: { level, ...(scope ? { scope } : {}) }, version: 1, owner: 'you', summary: '', overridable: true, status: 'active', body: '', author: 'a', approvedBy: 'you', ...over };
};
const org = (over: Partial<ProcessDoc> = {}) => file('organization', undefined, over);
const team = (scope: string, over: Partial<ProcessDoc> = {}) => file('team', scope, over);
const proj = (scope: string, over: Partial<ProcessDoc> = {}) => file('project', scope, over);
const P = (teamIds: string[]): ProjectRef => ({ key: 'p', name: 'P', teamIds });
const run = (docs: ProcessDoc[], teamIds: string[], agentId?: string) => resolve({ docs, teams: TEAMS, project: P(teamIds), ...(agentId ? { agentId } : {}) });
const win = (set: ReturnType<typeof run>, key = 'x') => set.entries.find((e) => e.key === key);

describe('PROC-2/PROC-9 nested teams: the parent of a file is the nearest file above it', () => {
	it('a sub-team file overrides its parent team’s file: kind override, and drift against that file, not the organization’s', () => {
		const set = run([org({ version: 9 }), team('web', { version: 3 }), team('ui', { overrides: { key: 'x', version: 1 } })], ['ui']);
		const e = win(set)!;
		expect(e).toMatchObject({ kind: 'override', source: 'team', scopeName: 'Ui' });
		expect(e.drift).toEqual({ wrote: 1, parentVersion: 3, parentLevel: 'team', parentName: 'Web' });
		expect(e.chain.map((l) => [l.scopeName ?? l.level, l.outcome])).toEqual([
			['organization', 'overridden'],
			['Web', 'overridden'],
			['Ui', 'applies']
		]);
		expect(e.depth).toBe(2);
	});

	it('a team file with no file above it anywhere is an addition', () => {
		expect(win(run([team('web')], ['ui']))).toMatchObject({ kind: 'addition', source: 'team' });
	});

	it('a project file overriding a team file drifts against that team file', () => {
		const set = run([org({ version: 5 }), team('web', { version: 2 }), proj('p', { overrides: { key: 'x', version: 1 } })], ['web']);
		expect(win(set)).toMatchObject({ kind: 'override', source: 'project' });
		expect(win(set)!.drift).toEqual({ wrote: 1, parentVersion: 2, parentLevel: 'team', parentName: 'Web' });
	});

	it('an override that does not name its parent’s version has unknown drift', () => {
		const set = run([org(), team('web')], ['web']);
		expect(win(set)).toMatchObject({ kind: 'override', driftUnknown: true });
		expect(win(set)!.drift).toBeUndefined();
		const wrongKey = run([org(), team('web', { overrides: { key: 'other', version: 1 } })], ['web']);
		expect(win(wrongKey)!.driftUnknown).toBe(true);
	});

	it('a linked ancestor team is dropped: its files come once, through the chain of the team below it', () => {
		const set = run([team('eng', { key: 'a' }), team('ui', { key: 'b' })], ['eng', 'ui']);
		expect(set.entries.map((e) => e.key)).toEqual(['a', 'b']);
		expect(set.conflicts).toEqual([]);
	});

	it('a parent loop among the teams does not drop the team files', () => {
		const looping: TeamRef[] = [{ id: 'a', name: 'A', owner_team_id: 'b' }, { id: 'b', name: 'B', owner_team_id: 'a' }];
		const set = resolve({ docs: [file('team', 'a')], teams: looping, project: P(['a']) });
		expect(win(set)).toBeDefined();
	});
});

describe('PROC-3 seals in chains', () => {
	it('a seal in an ancestor team refuses a descendant’s file and the project’s file', () => {
		const set = run([team('eng', { overridable: false }), team('ui'), proj('p')], ['ui']);
		expect(win(set)).toMatchObject({ source: 'team', scopeName: 'Eng', sealed: true });
		expect(set.problems.filter((p) => p.kind === 'sealed_refused').map((p) => p.doc.level)).toEqual(['team', 'project']);
	});

	it('the organization’s seal refuses a project file, and a project file that turns the key off', () => {
		const set = run([org({ overridable: false }), proj('p', { status: 'off', reason: 'no' })], []);
		expect(win(set)).toMatchObject({ source: 'organization' });
		expect(set.turnedOff).toEqual([]);
		expect(set.problems.map((p) => p.kind)).toEqual(['sealed_refused']);
	});

	it('a seal left out by a clash or a level mismatch still seals', () => {
		const clash = run([team('plat', { overridable: false }), team('plat', { overridable: false }), proj('p')], ['plat']);
		expect(clash.problems.some((p) => p.kind === 'sealed_refused' && p.doc.level === 'project')).toBe(true);
		const lying = run([team('plat', { overridable: false, level: 'organization' }), proj('p')], ['plat']);
		expect(lying.problems.some((p) => p.kind === 'sealed_refused' && p.doc.level === 'project')).toBe(true);
	});

	it('a draft or retired file never seals, even when unreadable', () => {
		const set = run([team('plat', { overridable: false, status: 'draft', problems: [{ field: 'version', message: 'x' }] }), proj('p')], ['plat']);
		expect(win(set)).toMatchObject({ source: 'project' });
	});

	it('the project settles a sealed conflict with its own version', () => {
		const set = run([team('plat', { overridable: false }), team('des'), proj('p')], ['plat', 'des']);
		expect(win(set)).toMatchObject({ source: 'project' });
		expect(set.conflicts).toEqual([]);
	});

	it('a project file is refused by a team seal when no conflict opened the key', () => {
		const set = run([team('plat', { overridable: false }), proj('p')], ['plat']);
		expect(win(set)).toMatchObject({ source: 'team', scopeName: 'Plat' });
	});
});

describe('PROC-6 combining chains', () => {
	it('two different seals: the organization’s version, else the first by team id; an unsealed file never applies meanwhile', () => {
		const sealed = [team('plat', { overridable: false }), team('des', { overridable: false }), team('web')];
		const noOrg = run(sealed, ['plat', 'des', 'web']);
		expect(win(noOrg)).toMatchObject({ source: 'team', scopeName: 'Des' });
		expect(noOrg.conflicts[0].applies).toBe('Sealed by Des; applies until the project decides');
		const withOrg = run([org(), ...sealed], ['plat', 'des', 'web']);
		expect(win(withOrg)).toMatchObject({ source: 'organization' });
		expect(withOrg.conflicts[0].applies).toBe("The organization's version applies until the project decides");
	});

	it('the conflict names the team whose file applies whatever the order the teams are linked in', () => {
		const files = [team('plat', { overridable: false }), team('des', { overridable: false })];
		const a = run(files, ['plat', 'des']);
		const b = run(files, ['des', 'plat']);
		expect(a.conflicts[0].applies).toBe(b.conflicts[0].applies);
		expect(win(a)!.doc.id).toBe(win(b)!.doc.id);
		expect(a.entries.map((e) => e.key)).toEqual(b.entries.map((e) => e.key));
	});

	it('a sealed file that applies meanwhile says so in its entry, with the team’s name', () => {
		const set = run([team('plat', { overridable: false }), team('des')], ['plat', 'des']);
		expect(win(set)!.sealedNote).toBe('Sealed by Plat; applies until the project decides');
		expect(win(set)!.chain.map((l) => l.outcome).sort()).toEqual(['applies', 'conflict']);
	});

	it('a team off that wins alone turns the key off; in a conflict with no seal it does not apply and the organization’s version does', () => {
		const alone = run([org(), team('plat', { status: 'off', reason: 'we do not use it' })], ['plat']);
		expect(alone.turnedOff).toEqual([expect.objectContaining({ scopeName: 'Plat', reason: 'we do not use it' })]);
		expect(win(alone)).toBeUndefined();
		const conflict = run([org(), team('plat', { status: 'off', reason: 'x' }), team('des')], ['plat', 'des']);
		expect(win(conflict)).toMatchObject({ source: 'organization' });
		expect(conflict.turnedOff).toEqual([]);
	});

	it('does not depend on the order the files are given in', () => {
		const files = [org(), team('plat'), team('des'), proj('p', { key: 'y' })];
		const a = run(files, ['plat', 'des']);
		const b = run([...files].reverse(), ['des', 'plat']);
		expect(a.entries.map((e) => [e.key, e.doc.id])).toEqual(b.entries.map((e) => [e.key, e.doc.id]));
		expect(a.conflicts.map((c) => c.key)).toEqual(b.conflicts.map((c) => c.key));
	});
});

describe('PROC-5 a private file and a key in a file that is not applied', () => {
	it('a private file whose key is used by an inherited draft or unreadable file is still not applied', () => {
		for (const hidden of [{ status: 'draft' as const }, { problems: [{ field: 'version', message: 'x' }] }]) {
			const set = run([org(hidden), file('agent', 'noor', { key: 'x' })], [], 'noor');
			expect(set.problems.some((p) => p.kind === 'private_only_adds')).toBe(true);
			expect(win(set)).toBeUndefined();
		}
	});

	it('a flagged private file shows in the inherited entry’s chain as not applied, not as overridden', () => {
		const set = run([org(), file('agent', 'noor', { key: 'x' })], [], 'noor');
		expect(win(set)!.chain.map((l) => l.outcome)).toEqual(['applies', 'not_applied']);
	});
});

describe('the layers say what happened to a file', () => {
	it('a draft is left out with its reason, and is not a problem', () => {
		const set = run([org(), team('plat', { status: 'draft' })], ['plat']);
		expect(set.problems).toEqual([]);
		expect(win(set)!.chain.map((l) => [l.outcome, l.note])).toEqual([
			['applies', undefined],
			['left_out', 'Left out: it is a draft.']
		]);
	});
});
