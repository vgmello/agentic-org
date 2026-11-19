import { describe, expect, it } from 'vitest';
import {
	HR_CANT_RUN,
	actorName,
	checkDraft,
	conflictLatest,
	emptyRolesText,
	employeesText,
	hasContent,
	invalidText,
	normalizeDraft,
	refusedField,
	roleBlocks,
	sortRoles,
	staleText,
	unchanged
} from './present';
import type { Role } from './types';

describe('the spec copy', () => {
	it('says the HR lead cannot run, word for word', () => {
		expect(HR_CANT_RUN).toBe("The HR lead can't run right now, so it can't draft roles.");
	});
	it('only offers an HR draft in the empty copy when the HR lead can run (ROLE-10)', () => {
		expect(emptyRolesText(true)).toBe(
			"Roles describe employees' work. Create one, or ask HR to draft one."
		);
		expect(emptyRolesText(false)).not.toMatch(/HR/);
	});
	it('names the stale copy and an invalid file', () => {
		expect(staleText('18 Nov, 09:40')).toBe(
			"Can't reach organization data. Showing the last copy from 18 Nov, 09:40."
		);
		expect(invalidText('QA Engineer', 'key')).toBe('QA Engineer: details not recognized (key).');
	});
	it('counts employees as the list does', () => {
		expect(employeesText(0)).toBe('no employees yet');
		expect(employeesText(1)).toBe('1 employee');
		expect(employeesText(4)).toBe('4 employees');
	});
});

describe('sortRoles', () => {
	it('orders by name without regard to case, then by key', () => {
		const sorted = sortRoles([
			{ id: '3', name: 'staff Engineer' },
			{ id: '2', name: 'QA Engineer', key: 'qa-b' },
			{ id: '1', name: 'QA Engineer', key: 'qa-a' },
			{ id: '4', name: 'Product Designer' }
		]);
		expect(sorted.map((r) => r.id)).toEqual(['4', '1', '2', '3']);
	});
});

describe('actorName', () => {
	it('uses the resolved name, else says what kind of writer it was', () => {
		expect(actorName({ byKind: 'employee', byId: 'you', byName: 'Alex Kim' })).toBe('Alex Kim');
		expect(actorName({ byKind: 'employee', byId: 'gone' })).toBe('A former employee');
		expect(actorName({ byKind: 'system' })).toBe('Agentic Org');
		expect(actorName({ byKind: 'unknown' })).toBe('Not recorded');
	});
});

describe('normalizeDraft and checkDraft', () => {
	it('accepts pasted spaces and a key typed with capitals and spaces', () => {
		expect(
			normalizeDraft({ name: '  Site   Reliability ', key: ' SRE Lead ', bodyMarkdown: 'x\n\n\n' })
		).toEqual({ name: 'Site Reliability', key: 'sre-lead', bodyMarkdown: 'x\n' });
	});
	it('drops an empty key rather than sending one', () => {
		expect(normalizeDraft({ name: 'A', key: '   ', bodyMarkdown: '' })).toEqual({
			name: 'A',
			bodyMarkdown: '\n'
		});
	});
	it('needs a name and a well-formed key', () => {
		expect(checkDraft({ name: '', bodyMarkdown: '' })).toEqual({ name: 'Give the role a name.' });
		expect(checkDraft({ name: 'A', key: 'qa--x', bodyMarkdown: '' }).key).toMatch(/single hyphens/);
		expect(checkDraft({ name: 'A', key: 'q_a', bodyMarkdown: '' }).key).toBeDefined();
		expect(checkDraft({ name: 'A', key: 'x'.repeat(65), bodyMarkdown: '' }).key).toMatch(/64/);
		expect(checkDraft({ name: 'A', key: 'qa-engineer-2', bodyMarkdown: '' })).toEqual({});
		expect(checkDraft({ name: 'x'.repeat(121), bodyMarkdown: '' }).name).toMatch(/120/);
	});
});

describe('unchanged', () => {
	const role = { name: 'QA', key: 'qa', bodyMarkdown: '## Mission\nTest.\n' };
	it('is true when only trailing space differs', () => {
		expect(unchanged(role, { name: 'QA', key: 'qa', bodyMarkdown: '## Mission\nTest.\n\n' })).toBe(
			true
		);
	});
	it('notices a removed key or a changed body', () => {
		expect(unchanged(role, { name: 'QA', bodyMarkdown: role.bodyMarkdown })).toBe(false);
		expect(unchanged(role, { name: 'QA', key: 'qa', bodyMarkdown: 'Other' })).toBe(false);
	});
});

describe('refusals', () => {
	it('reads the refused field', () => {
		expect(
			refusedField({ source: 'api', code: 'already_exists', message: 'x', field: 'key' })
		).toBe('key');
		expect(refusedField({ source: 'api', code: 'internal', message: 'x' })).toBeUndefined();
		expect(refusedField({ field: 'password' })).toBeUndefined();
		expect(refusedField(null)).toBeUndefined();
	});
	it('reads the newer role from a conflict only', () => {
		const latest = { id: 'r', revision: 4 } as Role;
		expect(conflictLatest({ source: 'api', code: 'conflict', message: 'x', latest })).toBe(latest);
		expect(
			conflictLatest({ source: 'api', code: 'not_found', message: 'x', latest })
		).toBeUndefined();
		expect(conflictLatest({ source: 'api', code: 'conflict', message: 'x' })).toBeUndefined();
	});
});

describe('roleBlocks', () => {
	it('reads headings, lists and paragraphs, and never HTML', () => {
		expect(
			roleBlocks(
				'## Mission\nBuild it\nwell.\n\n## Responsibilities\n- One\n* Two\n1. Three\n\n<b>bold</b>'
			)
		).toEqual([
			{ kind: 'heading', text: 'Mission' },
			{ kind: 'paragraph', text: 'Build it well.' },
			{ kind: 'heading', text: 'Responsibilities' },
			{ kind: 'list', items: ['One', 'Two', 'Three'] },
			{ kind: 'paragraph', text: '<b>bold</b>' }
		]);
	});
	it('drops closing hashes and empty headings, and reads Windows line ends', () => {
		expect(roleBlocks('## Mission ##\r\n##\r\nText')).toEqual([
			{ kind: 'heading', text: 'Mission' },
			{ kind: 'paragraph', text: 'Text' }
		]);
	});
	it('tells an untouched template from written content', () => {
		expect(hasContent('## Mission\n\n## Boundaries\n')).toBe(false);
		expect(hasContent('## Mission\nShip.')).toBe(true);
	});
});
