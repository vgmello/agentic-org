import { describe, expect, it } from 'vitest';
import { addTeam, below, filterRows, managerChoices, markRemoved, normalize, patchRow, removeTeam, renameHandle, rowNote, setParent } from './review';
import { SAMPLE_SCAN } from './mock';
import { applyHints } from './hints';
import { SAMPLE_HINTS } from './mock';
import { buildRows, type Row } from './rows';
import { suggestTeams } from './teams';
import type { TeamDraft } from './teams';

const { rows } = buildRows(SAMPLE_SCAN, { exclude: ['sample'] });

describe('the review table', () => {
	it('filters the rows by what still has to be done', () => {
		expect(filterRows(rows, 'all')).toHaveLength(10);
		expect(filterRows(rows, 'needs').map((r) => r.name).sort()).toEqual(['agy-senior-dev', 'app-frontend-dev', 'codex-dev', 'reviewer', 'server-dev']);
		expect(filterRows(rows, 'ready')).toHaveLength(5);
		const skipped = patchRow(rows, 'reviewer', { choice: 'skip' });
		expect(filterRows(skipped, 'skipped').map((r) => r.name)).toEqual(['reviewer']);
		expect(filterRows(skipped, 'needs')).toHaveLength(4);
	});

	it('says what is special about a row', () => {
		const note = (name: string, over: object = {}) => rowNote({ ...rows.find((r) => r.name === name)!, ...over }, SAMPLE_SCAN, 'Acme');
		expect(note('Riley')).toMatchObject({ tone: 'info' });
		expect(note('Riley')!.text).toMatch(/^No messages from Riley yet\./);
		expect(note('ada')).toBeUndefined();
		expect(note('ada', { existingMemberId: 'm', existingKind: 'agent' })).toEqual({ tone: 'info', text: 'Already in Acme.' });
		expect(note('ada', { existingMemberId: 'm', existingKind: 'person' })!.text).toMatch(/person in Acme/);
		expect(note('ada', { existingMemberId: 'm', existingKind: 'agent', existingOffboarded: true })!.text).toMatch(/Offboarded in Acme/);
		expect(note('ada', { messagingName: 'has space' })).toMatchObject({ tone: 'bad' });
		expect(note('ada', { messagingName: 'has space' })!.text).toMatch(/^Needs a messaging name\. Messaging names use letters/);
	});
});

describe('editing the teams', () => {
	const drafts: TeamDraft[] = [
		{ key: 'engineering', name: 'Engineering' },
		{ key: 'app', name: 'App', parentKey: 'engineering' },
		{ key: 'ui', name: 'UI', parentKey: 'app' }
	];

	it('adds a team with a name of its own and keeps its key unique', () => {
		expect(addTeam(drafts, 'Quality').at(-1)).toEqual({ key: 'quality', name: 'Quality' });
		expect(addTeam(drafts, 'app').at(-1)).toEqual({ key: 'app-2', name: 'app' });
		expect(addTeam(drafts, '   ')).toBe(drafts);
	});

	it('knows what a team owns, however deep', () => {
		expect([...below(drafts, 'engineering')].sort()).toEqual(['app', 'ui']);
		expect([...below(drafts, 'ui')]).toEqual([]);
	});

	it('removing a team frees what it owned and takes its members out of any team', () => {
		const people = patchRow(rows, 'ac-staff-engineer-ada', { teamKey: 'app' });
		const out = removeTeam(drafts, people, 'app');
		expect(out.drafts.map((d) => d.key)).toEqual(['engineering', 'ui']);
		expect(out.drafts.find((d) => d.key === 'ui')?.parentKey).toBeUndefined();
		expect(out.rows.find((r) => r.name === 'ada')?.teamKey).toBeUndefined();
	});

	it('does not put a team under itself or under what it owns', () => {
		expect(setParent(drafts, 'engineering', 'ui')).toBe(drafts);
		expect(setParent(drafts, 'app', 'app')).toBe(drafts);
		expect(setParent(drafts, 'ui', 'engineering').find((d) => d.key === 'ui')?.parentKey).toBe('engineering');
		expect(setParent(drafts, 'ui', undefined).find((d) => d.key === 'ui')?.parentKey).toBeUndefined();
	});
});

describe('what points at nothing after a change', () => {
	const filled = (): { rows: Row[]; drafts: TeamDraft[] } => {
		const { rows: all } = buildRows(SAMPLE_SCAN, { exclude: ['sample'] });
		for (const r of all) if (!r.runtime) Object.assign(r, { role: 'software engineer', title: 'Software Engineer', runtime: 'codex' });
		const f = applyHints(all, SAMPLE_HINTS, suggestTeams(all));
		return { rows: f.rows, drafts: f.drafts };
	};

	it('sends the people who reported to a skipped manager to the owner', () => {
		const { rows: r, drafts } = filled();
		const withLines = r.map((x) => (x.name === 'server-dev' ? { ...x, managerKey: 'ac-staff-engineer-ada' } : x));
		const skipped = patchRow(withLines, 'ac-staff-engineer-ada', { choice: 'skip' });
		expect(normalize(skipped, drafts).rows.find((x) => x.name === 'server-dev')!.managerKey).toBe('owner');
	});

	it('leaves a team with no lead when the lead is skipped or moved out of it', () => {
		const { rows: r, drafts } = filled();
		expect(drafts.find((d) => d.key === 'app')!.leadKey).toBe('app-frontend-dev');
		const skipped = patchRow(r, 'app-frontend-dev', { choice: 'skip' });
		expect(normalize(skipped, drafts).drafts.find((d) => d.key === 'app')!.leadKey).toBeUndefined();
		const moved = patchRow(r, 'app-frontend-dev', { teamKey: 'office' });
		expect(normalize(moved, drafts).drafts.find((d) => d.key === 'app')!.leadKey).toBeUndefined();
	});

	it('keeps a lead who sits in a team below the one they lead (the file puts them there)', () => {
		const { rows: r, drafts } = filled();
		const ada = patchRow(r, 'ac-staff-engineer-ada', { teamKey: 'app' });
		expect(normalize(ada, drafts).drafts.find((d) => d.key === 'engineering')!.leadKey).toBe('ac-staff-engineer-ada');
	});
});

describe('the manager choices and renaming a messaging name', () => {
	const { rows: all } = buildRows(SAMPLE_SCAN, { exclude: ['sample'] });

	it('leaves out a row whose own line leads back (a circle HQ would refuse), the row itself and skipped rows', () => {
		const lines = all.map((r) => (r.name === 'River' ? { ...r, managerKey: 'ac-staff-engineer-ada' } : r));
		const ada = lines.find((r) => r.name === 'ada')!;
		expect(managerChoices(lines, ada).map((r) => r.name)).not.toContain('River');
		expect(managerChoices(lines, ada).map((r) => r.name)).not.toContain('ada');
		const skipped = patchRow(lines, 'reviewer', { choice: 'skip' });
		expect(managerChoices(skipped, ada).map((r) => r.name)).not.toContain('reviewer');
	});

	it('refuses a name HQ would refuse, or another row’s name, and says why', () => {
		expect(renameHandle(all, 'server-dev', 'has space', []).error).toMatch(/letters, digits/);
		expect(renameHandle(all, 'server-dev', 'CODEX-dev', []).error).toBe('codex-dev already uses that name.');
		expect(renameHandle(all, 'server-dev', 'server-dev-2', []).rows.find((r) => r.key === 'server-dev')!.messagingName).toBe('server-dev-2');
	});

	it('makes the row “Already in” when the organization already has an agent or a person by that name', () => {
		const member = { id: 'm9', kind: 'agent', name: 'x', agent: { messaging_name: 'Taken', status: 'active' } } as unknown as import('$lib/proto').Member;
		const person = { id: 'p9', kind: 'person', name: 'Pat', owner: false } as unknown as import('$lib/proto').Member;
		const a = renameHandle(all, 'server-dev', 'taken', [member]).rows.find((r) => r.key === 'server-dev')!;
		expect(a).toMatchObject({ existingMemberId: 'm9', existingKind: 'agent' });
		const p = renameHandle(all, 'server-dev', 'pat', [person]).rows.find((r) => r.key === 'server-dev')!;
		expect(p).toMatchObject({ existingMemberId: 'p9', existingKind: 'person' });
		const clear = renameHandle([a, ...all.filter((r) => r.key !== 'server-dev')], 'server-dev', 'fresh-name', [member]).rows.find((r) => r.key === 'server-dev')!;
		expect(clear.existingMemberId).toBeUndefined();
	});
});

describe('a removed agent is not shown as a manager or a lead', () => {
	it('sends their reports to the owner and clears their teams’ leads, once marked removed', () => {
		const { rows: r } = buildRows(SAMPLE_SCAN, { exclude: ['sample'] });
		const f = applyHints(r, SAMPLE_HINTS, suggestTeams(r));
		const ledger = { teams: {}, agents: { 'ac-staff-engineer-ada': { id: 'gone', created: true, linked: true } } };
		const marked = markRemoved(f.rows, ledger, new Set(['other']));
		const fixed = normalize(marked, f.drafts);
		expect(marked.find((x) => x.name === 'ada')!.removedEarlier).toBe(true);
		expect(fixed.rows.find((x) => x.name === 'River')!.managerKey).toBe('owner');
		expect(fixed.drafts.find((d) => d.key === 'engineering')!.leadKey).toBeUndefined();
	});
});
