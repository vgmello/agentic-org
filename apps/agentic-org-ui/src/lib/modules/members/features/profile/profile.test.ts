import { describe, expect, it } from 'vitest';
import { roleBlocks } from '../role/role-body';
import { expiryText, expiryUntil } from '../sign-in-access/expiry';
import { tabHref, tabOf, tabsFor } from './tabs';

describe('profile tabs', () => {
	const viewer = { isSelf: false, isOwnerOrAdmin: true, costVisible: true };
	it('gives a person the colleague tabs only', () => {
		expect(tabsFor({ kind: 'person', viewer })).toEqual([
			'overview',
			'work',
			'conversations',
			'activity'
		]);
	});
	it('gives an agent Runs, Processes, Role, Notes and memory, and Cost only when amounts are visible', () => {
		expect(tabsFor({ kind: 'agent', viewer })).toContain('cost');
		const tabs = tabsFor({ kind: 'agent', viewer: { ...viewer, costVisible: false } });
		expect(tabs).toEqual([
			'overview',
			'work',
			'conversations',
			'activity',
			'runs',
			'processes',
			'role',
			'notes'
		]);
	});
	it('maps tabs to URLs and back', () => {
		expect(tabHref('noor', 'overview')).toBe('/members/noor');
		expect(tabHref('a b', 'runs')).toBe('/members/a%20b/runs');
		expect(tabOf('/members/noor/runs', 'noor')).toBe('runs');
		expect(tabOf('/members/noor', 'noor')).toBe('overview');
		expect(tabOf('/members/noor/nonsense', 'noor')).toBe('overview');
	});
});

describe('roleBlocks', () => {
	it('reads headings, bullet lists and paragraphs', () => {
		expect(
			roleBlocks('## Mission\nKeep it\nshipping.\n\n## Boundaries\n- Never merges\n- Never deploys')
		).toEqual([
			{ kind: 'heading', text: 'Mission' },
			{ kind: 'paragraph', text: 'Keep it shipping.' },
			{ kind: 'heading', text: 'Boundaries' },
			{ kind: 'list', items: ['Never merges', 'Never deploys'] }
		]);
	});
	it('keeps markup as text', () => {
		expect(roleBlocks('<b>x</b>')).toEqual([{ kind: 'paragraph', text: '<b>x</b>' }]);
	});
});

describe('expiryText', () => {
	const now = new Date(2026, 10, 18, 14, 2);
	it('says today, tomorrow or the date, in 24-hour time', () => {
		expect(expiryText(new Date(2026, 10, 18, 20, 0).toISOString(), now)).toMatch(
			/^at 20.00 today$/
		);
		expect(expiryText(new Date(2026, 10, 19, 14, 2).toISOString(), now)).toMatch(
			/^at 14.02 tomorrow$/
		);
		expect(expiryText(new Date(2026, 10, 21, 9, 5).toISOString(), now)).toMatch(/^at 09.05 on /);
	});
	it('gives the moment alone for "the link works until …"', () => {
		expect(expiryUntil(new Date(2026, 10, 19, 14, 2).toISOString(), now)).toMatch(
			/^14.02 tomorrow$/
		);
	});
});
