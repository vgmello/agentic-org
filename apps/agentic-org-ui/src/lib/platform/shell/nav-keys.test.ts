import { describe, expect, it } from 'vitest';
import { matchNavCandidate } from './nav-keys.svelte';

describe('matchNavCandidate', () => {
	const items = [
		{ label: 'Home' },
		{ label: 'Inbox' },
		{ label: 'Work' },
		{ label: 'Projects' },
		{ label: 'People' },
		{ label: 'Processes' },
		{ label: 'Activity' }
	];

	it('matches first item starting with letter case-insensitively', () => {
		expect(matchNavCandidate(items, 'w', -1)).toBe(2);
		expect(matchNavCandidate(items, 'W', -1)).toBe(2);
		expect(matchNavCandidate(items, 'i', -1)).toBe(1);
	});

	it('cycles through multiple items sharing the same initial letter', () => {
		const first = matchNavCandidate(items, 'p', -1);
		expect(first).toBe(3); // Projects
		const second = matchNavCandidate(items, 'p', first);
		expect(second).toBe(4); // People
		const third = matchNavCandidate(items, 'p', second);
		expect(third).toBe(5); // Processes
		const wrapped = matchNavCandidate(items, 'p', third);
		expect(wrapped).toBe(3); // wraps back to Projects
	});

	it('returns -1 for non-letters or unmatched letters', () => {
		expect(matchNavCandidate(items, 'z', -1)).toBe(-1);
		expect(matchNavCandidate(items, '1', -1)).toBe(-1);
		expect(matchNavCandidate(items, '', -1)).toBe(-1);
	});

	it('cycles repeated initials across team and project page tab rows', () => {
		const teamTabs = [
			{ label: 'Overview' },
			{ label: 'Members' },
			{ label: 'Work' },
			{ label: 'Projects' },
			{ label: 'Activity' },
			{ label: 'Conversations' },
			{ label: 'Processes' }
		];
		const firstP = matchNavCandidate(teamTabs, 'p', -1);
		expect(firstP).toBe(3); // Projects
		const secondP = matchNavCandidate(teamTabs, 'p', firstP);
		expect(secondP).toBe(6); // Processes
		expect(matchNavCandidate(teamTabs, 'p', secondP)).toBe(3); // wraps to Projects
		expect(matchNavCandidate(teamTabs, 'm', -1)).toBe(1); // Members
		expect(matchNavCandidate(teamTabs, 'w', -1)).toBe(2); // Work
	});
});
