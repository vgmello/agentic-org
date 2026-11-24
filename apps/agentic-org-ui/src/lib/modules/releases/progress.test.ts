import { describe, expect, it } from 'vitest';
import {
	computeReleaseDetailReadiness,
	computeReleaseMilestonesAggregatePct,
	computeReleaseMilestoneStats,
	isReleaseTaskFinished
} from './progress';
import type { ReleaseDetail, ReleaseMilestone } from './types';

function makeReleaseDetail(overrides: Partial<ReleaseDetail>): ReleaseDetail {
	return {
		id: 'rel-1',
		projectId: 'beacon',
		projectNumber: 8,
		number: 1,
		versionLabel: '2.0.0',
		title: 'Checkout v2 foundation',
		status: 'released',
		owner: null,
		milestoneCount: 0,
		featureCount: 0,
		taskCount: 0,
		deploymentCount: 0,
		updatedAt: '2026-11-14T11:15:00Z',
		revision: 1,
		path: 'projects/beacon/releases/rel-1/release.md',
		bodyMarkdown: '',
		createdAt: '2026-11-03T10:00:00Z',
		createdBy: { id: 'avery', name: 'Avery Stone', kind: 'person' },
		updatedBy: { id: 'avery', name: 'Avery Stone', kind: 'person' },
		milestones: [],
		features: [],
		tasks: [],
		flagSettings: [],
		deployments: [],
		supportingDocs: [],
		canEdit: true,
		...overrides
	};
}

describe('release & milestone progress helpers (organization-data 02)', () => {
	it('normalizes title-cased and lowercase task statuses and counts cancelled as finished', () => {
		expect(isReleaseTaskFinished('Done')).toBe(true);
		expect(isReleaseTaskFinished('done')).toBe(true);
		expect(isReleaseTaskFinished('Closed')).toBe(true);
		expect(isReleaseTaskFinished('Cancelled')).toBe(true);
		expect(isReleaseTaskFinished('In progress')).toBe(false);
		expect(isReleaseTaskFinished('Blocked')).toBe(false);
	});

	it('computes 100% readiness for a release whose fixture tasks have status "Done"', () => {
		const detail = makeReleaseDetail({
			tasks: [
				{ id: 't1', ref: 'T-A38', type: 'task', title: 'Receipt layout', status: 'Done' },
				{ id: 't2', ref: 'T-A39', type: 'task', title: 'Idempotency schema', status: 'Done' }
			]
		});
		expect(computeReleaseDetailReadiness(detail)).toEqual({
			merged: 2,
			total: 2,
			pct: 100,
			scopeLabel: 'Release readiness'
		});
	});

	it('counts cancelled tasks as finished in both milestone stats and dock aggregate', () => {
		const cancelledOnly: ReleaseMilestone = {
			id: 'ms-cancel',
			number: 1,
			title: 'Retired scope',
			status: 'completed',
			tasksDone: 0,
			tasksOpen: 0,
			tasksCancelled: 2
		};
		expect(computeReleaseMilestoneStats(cancelledOnly)).toEqual({
			finished: 2,
			total: 2,
			pct: 100
		});

		const mixed: ReleaseMilestone = {
			id: 'ms-mix',
			number: 2,
			title: 'Active rollout',
			status: 'in_progress',
			tasksDone: 1,
			tasksOpen: 1,
			tasksCancelled: 1
		};
		expect(computeReleaseMilestoneStats(mixed)).toEqual({
			finished: 2,
			total: 3,
			pct: 67
		});
		// Aggregate: (2 + 2) finished / (2 + 3) total = 4 / 5 = 80%
		expect(computeReleaseMilestonesAggregatePct([cancelledOnly, mixed])).toBe(80);

		const detailFromMilestones = makeReleaseDetail({
			tasks: [],
			milestones: [cancelledOnly, mixed]
		});
		expect(computeReleaseDetailReadiness(detailFromMilestones)).toEqual({
			merged: 4,
			total: 5,
			pct: 80,
			scopeLabel: 'Release readiness'
		});
	});
});
