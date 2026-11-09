import { describe, expect, it } from 'vitest';
import { addTeam, below, filterRows, normalize, patchRow, removeTeam, rowNote, setParent } from './review';
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
