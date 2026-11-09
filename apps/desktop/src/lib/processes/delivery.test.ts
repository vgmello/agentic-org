import { describe, expect, it } from 'vitest';
import { changedSince, momentPrompts, sourceShort, startLine, taskChecklist } from './delivery';
import { resolve } from './resolve';
import { SAMPLE_PROJECTS, SAMPLE_TEAMS, sampleProcessDocs } from './sample';

const docs = sampleProcessDocs();
const set = resolve({ docs, teams: SAMPLE_TEAMS, project: SAMPLE_PROJECTS.find((p) => p.key === 'atl')!, agentId: 'kai' });
const branch = set.entries.find((e) => e.key === 'branch-and-merge')!;

describe('PROC-7 what an agent is handed', () => {
	it('names each process with where it comes from', () => {
		expect(sourceShort(branch)).toBe('org v3');
		expect(sourceShort(set.entries.find((e) => e.key === 'release-checklist')!)).toBe('Atlas v1');
	});

	it('lists the set, and what changed since the agent last read it', () => {
		const reads = Object.fromEntries(set.entries.map((e) => [e.key, { docId: e.doc.id, version: e.doc.version }]));
		expect(changedSince(set, reads)).toEqual([]);
		expect(startLine(set, reads, 'atl-T-17')).toBe(
			'Your processes for atl-T-17: Branch, review and merge (org v3), Code review (org v2), Incident response (Engineering v1), Release checklist (Atlas v1), Security basics (org v1).'
		);
		const stale = { ...reads, 'branch-and-merge': { docId: branch.doc.id, version: 2 } };
		expect(startLine(set, stale)).toContain('Changed since you last read: Branch, review and merge v2 → v3.');
		const none = startLine(set, {}, 'atl-T-17');
		expect(none).toContain('Changed since you last read: Branch, review and merge (new, v3)');
	});

	it('with no processes says so', () => {
		const empty = resolve({ docs: [], teams: [], agentId: 'kai' });
		expect(startLine(empty, {})).toBe('Your processes: none.');
	});
});

describe('PROC-12 moments and checklists', () => {
	it('a moment’s prompts come from the set, and only while the moment is On', () => {
		expect(momentPrompts(set, 'starting_task', true)).toEqual([{ name: 'Branch, review and merge', text: 'Check that your open branch is merged before you start new work.' }]);
		expect(momentPrompts(set, 'starting_task', false)).toEqual([]);
		expect(momentPrompts(set, 'finishing_task', true)[0].text).toBe('Ask the reviewer for READY on the exact commit.');
	});

	it('a new task takes the project’s default checklist, then the processes’ steps, merging duplicates with required winning', () => {
		const steps = taskChecklist(set, [
			{ text: 'Tests pass', required: true },
			{ text: 'own fresh review', required: false }
		]);
		expect(steps.map((s) => [s.text, s.required])).toEqual([
			['Tests pass', true],
			['own fresh review', true], // the same step as the process's: merged, and required wins
			['Reviewer’s READY for the exact commit', true],
			['The lead’s go', true],
			['Changelog written', true]
		]);
	});

	it('does not add a private process’s steps', () => {
		const mine = { ...docs[0], id: 'doc-pr', key: 'my-steps', name: 'My steps', level: 'agent' as const, scope: 'kai', location: { level: 'agent' as const, scope: 'kai' }, checklist: [{ text: 'Private step', required: true }] };
		const withPrivate = resolve({ docs: [...docs, mine], teams: SAMPLE_TEAMS, project: SAMPLE_PROJECTS[0], agentId: 'kai' });
		expect(taskChecklist(withPrivate, []).map((s) => s.text)).not.toContain('Private step');
	});
});
