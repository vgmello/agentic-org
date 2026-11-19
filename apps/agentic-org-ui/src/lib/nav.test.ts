import { describe, expect, it } from 'vitest';
import { areaFor, NAV, navFor, navItems, sublistLinks } from './nav';

describe('main navigation', () => {
	it('lists the spec destinations in order, without Following', () => {
		expect(NAV.map((item) => item.name)).toEqual([
			'Home',
			'Inbox',
			'Projects',
			'Work',
			'Chat',
			'Activity',
			'Teams',
			'Organization settings'
		]);
	});

	it('hides Inbox until it has a source and badges only a nonzero open count', () => {
		const names = (open: number | null | undefined) =>
			navItems(open === undefined ? undefined : { inboxOpen: open, projects: [], teams: [] }).map(
				(i) => i.name
			);
		expect(names(undefined)).not.toContain('Inbox');
		expect(names(null)).not.toContain('Inbox');
		expect(names(0)).toContain('Inbox');
		expect(
			navItems({ inboxOpen: 0, projects: [], teams: [] }).find((i) => i.name === 'Inbox')?.badge
		).toBeUndefined();
		expect(
			navItems({ inboxOpen: 3, projects: [], teams: [] }).find((i) => i.name === 'Inbox')?.badge
		).toEqual({ text: '3', label: '3 open' });
	});

	it('marks the owning destination current, a member profile under Teams', () => {
		expect(navFor('/teams/members')?.name).toBe('Teams');
		expect(navFor('/members/m1')?.name).toBe('Teams');
		expect(navFor('/settings/offices')?.name).toBe('Organization settings');
		expect(navFor('/projects/p1/work')?.name).toBe('Projects');
		expect(navFor('/homework')).toBeUndefined();
		expect(areaFor('/search')).toBe('Search');
		expect(areaFor('/insights')).toBe('Insights');
	});

	it('lists the first five places A–Z', () => {
		const places = ['Zeta', 'alpha', 'Delta', 'beta', 'Echo', 'Gamma'].map((name, i) => ({
			id: `p${i}`,
			name
		}));
		expect(sublistLinks(places, '/projects').map((l) => l.name)).toEqual([
			'alpha',
			'beta',
			'Delta',
			'Echo',
			'Gamma'
		]);
		expect(sublistLinks(places, '/teams')[0].href).toBe('/teams/p1');
	});
});
