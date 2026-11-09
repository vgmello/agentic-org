import { describe, expect, it } from 'vitest';
import { resolve } from './resolve';
import { SAMPLE_PROJECTS, SAMPLE_TEAMS, sampleProcessDocs } from './sample';
import type { EffectiveSet, ProcessDoc } from './types';

const docs = sampleProcessDocs();
const project = (key: string) => SAMPLE_PROJECTS.find((p) => p.key === key)!;
const run = (key: string | undefined, agentId?: string, extra: ProcessDoc[] = [], base: ProcessDoc[] = docs): EffectiveSet => resolve({ docs: [...base, ...extra], teams: SAMPLE_TEAMS, project: key ? project(key) : undefined, ...(agentId ? { agentId } : {}) });
const entry = (set: EffectiveSet, key: string) => set.entries.find((e) => e.key === key);
const names = (set: EffectiveSet) => set.entries.map((e) => e.key);

describe('PROC-2 the effective set is organization, the team chain, the project and the agent’s own', () => {
	it('with no current work it is the organization’s processes and the agent’s own', () => {
		const set = run(undefined, 'noor');
		expect(names(set)).toEqual(['branch-and-merge', 'code-review', 'my-notes', 'security-basics', 'weekly-status']);
		expect(entry(set, 'my-notes')).toMatchObject({ kind: 'addition', source: 'agent' });
		// Team and project processes join when it starts a task.
		expect(names(set)).not.toContain('incident-response');
	});

	it('a team’s processes come from its whole chain, outermost parent first', () => {
		const set = run('atl'); // Atlas belongs to Platform, which is under Engineering
		expect(entry(set, 'incident-response')).toMatchObject({ kind: 'addition', source: 'team', scopeName: 'Engineering' });
		expect(entry(set, 'release-checklist')).toMatchObject({ kind: 'addition', source: 'project', scopeName: 'Atlas' });
		expect(entry(set, 'branch-and-merge')).toMatchObject({ kind: 'inherited', source: 'organization' });
	});

	it('the lowest level that has a key wins, and the chain shows each layer', () => {
		const only = docs.filter((d) => d.location.level !== 'team' || d.location.scope !== 'design');
		const set = resolve({ docs: only, teams: SAMPLE_TEAMS, project: { key: 'web1', name: 'Web one', teamIds: ['web'] } });
		const review = entry(set, 'code-review')!;
		expect(review).toMatchObject({ kind: 'override', source: 'team', scopeName: 'Web' });
		expect(review.chain.map((l) => [l.level, l.outcome])).toEqual([
			['organization', 'overridden'],
			['team', 'applies']
		]);
		expect(review.chain[0].note).toBe('Overridden by team Web.');
	});
});

describe('PROC-9 drift', () => {
	it('an override written against an older parent says "Parent changed", with the versions', () => {
		const set = resolve({ docs, teams: SAMPLE_TEAMS, project: { key: 'web1', name: 'Web one', teamIds: ['web'] } });
		expect(entry(set, 'code-review')!.drift).toEqual({ wrote: 1, parentVersion: 2, parentLevel: 'organization', parentName: 'Organization' });
	});

	it('an override written against the current version does not drift', () => {
		const set = resolve({ docs, teams: SAMPLE_TEAMS, project: { key: 'des1', name: 'Design one', teamIds: ['design'] } });
		expect(entry(set, 'code-review')).toMatchObject({ source: 'team', scopeName: 'Design' });
		expect(entry(set, 'code-review')!.drift).toBeUndefined();
	});
});

describe('PROC-3/PROC-6 a shared project’s conflict', () => {
	it('two teams that both override the same key: the organization’s version applies and the conflict is listed', () => {
		const set = run('bea');
		expect(entry(set, 'code-review')).toMatchObject({ source: 'organization' });
		expect(set.conflicts).toEqual([expect.objectContaining({ key: 'code-review', teams: ['Design', 'Web'], applies: "The organization's version applies until the project decides" })]);
	});

	it('with no organization version neither override applies', () => {
		const noOrg = docs.filter((d) => !(d.location.level === 'organization' && d.key === 'code-review'));
		const set = run('bea', undefined, [], noOrg);
		expect(names(set)).not.toContain('code-review');
		expect(set.conflicts[0].applies).toBe('Neither team’s version applies until the project decides');
	});

	it('the project’s own version settles the conflict', () => {
		const settled = sampleProcessDocs().concat([
			{ ...sampleProcessDocs()[1], id: 'doc-settle', level: 'project', scope: 'bea', location: { level: 'project', scope: 'bea' }, name: 'Code review (Beacon)', version: 1, overrides: { key: 'code-review', version: 2 } }
		]);
		const set = run('bea', undefined, [], settled);
		expect(entry(set, 'code-review')).toMatchObject({ source: 'project', scopeName: 'Beacon' });
		expect(set.conflicts).toEqual([]);
	});

	it('when only one of the project’s teams has a version, it applies without a conflict', () => {
		const set = run('bea', undefined, [], docs.filter((d) => !(d.key === 'code-review' && d.location.scope === 'design')));
		expect(entry(set, 'code-review')).toMatchObject({ source: 'team', scopeName: 'Web' });
		expect(set.conflicts).toEqual([]);
	});
});

describe('PROC-3 sealed processes', () => {
	it('a team’s override of a key the organization sealed is refused, with the reason', () => {
		const set = run('atl');
		expect(entry(set, 'security-basics')).toMatchObject({ source: 'organization', sealed: true });
		const refused = set.problems.find((p) => p.kind === 'sealed_refused');
		expect(refused?.text).toBe('Security basics (Platform) is sealed by the organization. You can add a process instead.');
		expect(entry(set, 'security-basics')!.chain.map((l) => l.outcome)).toEqual(['applies', 'refused']);
	});

	it('a team seal binds its projects: the project’s file for that key is refused', () => {
		const sealed = docs.concat({ ...docs[1], id: 'doc-seal', key: 'release-checklist', name: 'Release checklist (Platform)', level: 'team', scope: 'platform', location: { level: 'team', scope: 'platform' }, overridable: false });
		const set = run('atl', undefined, [], sealed);
		expect(entry(set, 'release-checklist')).toMatchObject({ source: 'team', scopeName: 'Platform', sealed: true });
		expect(set.problems.some((p) => p.kind === 'sealed_refused' && p.doc.name === 'Release checklist')).toBe(true);
	});

	it('on a shared project a sibling team’s override of a team-sealed key is a conflict: the sealed version applies meanwhile', () => {
		const sealedByWeb = sampleProcessDocs().map((d) => (d.key === 'code-review' && d.location.scope === 'web' ? { ...d, overridable: false } : d));
		const set = run('bea', undefined, [], sealedByWeb);
		expect(entry(set, 'code-review')).toMatchObject({ source: 'team', scopeName: 'Web', sealed: true, sealedNote: 'Sealed by Web; applies until the project decides' });
		expect(set.conflicts[0].applies).toBe('Sealed by Web; applies until the project decides');
	});

	it('a file too broken to read that was sealed still seals its key (a seal fails closed)', () => {
		const broken = docs.concat({ ...docs[2], id: 'doc-broken', key: 'weekly-status', level: 'team', scope: 'platform', location: { level: 'team', scope: 'platform' }, overridable: false, problems: [{ field: 'version', message: 'not a number' }] });
		const lowered = broken.concat({ ...docs[2], id: 'doc-low', key: 'weekly-status', name: 'Weekly (Atlas)', level: 'project', scope: 'atl', location: { level: 'project', scope: 'atl' }, status: 'active' });
		const set = run('atl', undefined, [], lowered.filter((d) => !(d.key === 'weekly-status' && d.location.level === 'project' && d.status === 'off')));
		expect(set.problems.some((p) => p.kind === 'invalid' && p.doc.id === 'doc-broken')).toBe(true);
		expect(set.problems.some((p) => p.kind === 'sealed_refused' && p.doc.id === 'doc-low')).toBe(true);
	});
});

describe('PROC-4 a lower level turns a process off', () => {
	it('records "Turned off for Atlas: we report daily" and drops it from the set', () => {
		const set = run('atl');
		expect(names(set)).not.toContain('weekly-status');
		expect(set.turnedOff).toEqual([expect.objectContaining({ key: 'weekly-status', scopeName: 'Atlas', reason: 'we report daily' })]);
	});
});

describe('PROC-1/PROC-16 problems in the files', () => {
	it('a file with unreadable front matter is listed with its field and left out of the set', () => {
		const design = resolve({ docs, teams: SAMPLE_TEAMS, project: { key: 'des1', name: 'Design one', teamIds: ['design'] } });
		expect(design.problems.find((p) => p.kind === 'invalid')).toMatchObject({ key: 'design-handoff', text: 'Details not recognized: overridable.' });
		expect(names(design)).not.toContain('design-handoff');
	});

	it('two files with one key at one level and scope: neither applies, and both rows say so', () => {
		const set = run('bea');
		expect(names(set)).not.toContain('standup');
		const clashes = set.problems.filter((p) => p.kind === 'clash');
		expect(clashes).toHaveLength(2);
		expect(clashes[0].text).toBe('Two processes use the key `standup` in Beacon. Neither applies until one is renamed.');
	});

	it('a file whose level or scope differs from where it lives is a problem, not a process', () => {
		const lying = docs.concat({ ...docs[2], id: 'doc-lie', key: 'sneaky', name: 'Sneaky', level: 'organization', location: { level: 'team', scope: 'web' } });
		const set = run('bea', undefined, [], lying);
		expect(set.problems.find((p) => p.kind === 'mismatch')).toMatchObject({ key: 'sneaky' });
		expect(names(set)).not.toContain('sneaky');
	});

	it('drafts and retired files are not in the set', () => {
		const set = run('atl', undefined, [{ ...docs[2], id: 'doc-draft', key: 'draft-one', name: 'Draft one', status: 'draft' }]);
		expect(names(set)).not.toContain('draft-one');
	});
});

describe('PROC-5 private processes only add', () => {
	it('a private file that changes an inherited key is not applied, and is flagged', () => {
		const set = run('bea', 'noor');
		expect(names(set)).toContain('my-notes');
		const flagged = set.problems.find((p) => p.kind === 'private_only_adds');
		expect(flagged).toMatchObject({ key: 'code-review', text: "Private processes only add: My code review isn't applied because it changes an inherited process." });
		expect(entry(set, 'code-review')!.source).not.toBe('agent');
	});

	it('a private file that overrides or turns off is refused with the case named', () => {
		const base = docs.filter((d) => d.location.level !== 'agent');
		const mine = (over: Partial<ProcessDoc>): ProcessDoc => ({ ...docs[2], id: `doc-p-${over.key}`, level: 'agent', scope: 'noor', location: { level: 'agent', scope: 'noor' }, ...over });
		const set = resolve({ docs: [...base, mine({ key: 'a', name: 'A', overrides: { key: 'weekly-status', version: 1 } }), mine({ key: 'b', name: 'B', status: 'off' })], teams: SAMPLE_TEAMS, agentId: 'noor' });
		expect(set.problems.map((p) => p.text)).toEqual(["Private processes only add: they can't override a process.", "Private processes only add: they can't turn off a process."]);
	});

	it('another agent’s private processes are not in the set', () => {
		expect(names(run('atl', 'kai'))).not.toContain('my-notes');
	});
});
