import { describe, expect, it } from 'vitest';
import type { Member } from '$lib/proto';
import { SAMPLE_SCAN } from './mock';
import { buildRows, counts, hasNoMessages, isReady, missingOf, OWNER, roleWords } from './rows';
import type { Scan } from './types';

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
