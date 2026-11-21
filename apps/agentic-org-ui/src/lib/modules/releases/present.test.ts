import { describe, expect, it } from 'vitest';
import {
	matchesReleaseFilter,
	milestoneProgressText,
	milestoneRef,
	releaseRef,
	shortSha
} from './present';
import type { ReleaseRow } from './types';

describe('releases present helpers', () => {
	const row: ReleaseRow = {
		id: 'rel-1',
		projectId: 'beacon',
		projectNumber: 8,
		number: 3,
		versionLabel: '2.1.0',
		title: 'Mobile checkout rollout',
		status: 'in_progress',
		owner: { id: 'noor', name: 'Noor Haddad', kind: 'agent' },
		milestoneCount: 2,
		featureCount: 2,
		taskCount: 5,
		deploymentCount: 1,
		updatedAt: '2026-11-18T10:00:00Z'
	};

	it('formats release and milestone references and short SHAs', () => {
		expect(releaseRef(row)).toBe('R-3');
		expect(milestoneRef({ number: 2 })).toBe('M-2');
		expect(shortSha('aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa')).toBe('aaaaaaaaaa');
	});

	it('filters releases by status, ref, version label, and title', () => {
		expect(matchesReleaseFilter(row, 'all', 'R-3')).toBe(true);
		expect(matchesReleaseFilter(row, 'all', '2.1.0')).toBe(true);
		expect(matchesReleaseFilter(row, 'in_progress', 'checkout')).toBe(true);
		expect(matchesReleaseFilter(row, 'released', '')).toBe(false);
	});

	it('excludes cancelled tasks from milestone eligible denominator', () => {
		expect(
			milestoneProgressText({
				id: 'm1',
				number: 1,
				title: 'M1',
				status: 'in_progress',
				tasksDone: 3,
				tasksOpen: 1,
				tasksCancelled: 2
			})
		).toBe('3 of 4 tasks done · 2 cancelled');
	});
});
