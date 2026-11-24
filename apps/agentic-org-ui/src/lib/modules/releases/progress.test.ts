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

	it('prefers authoritative backend release_progress when supplied', () => {
		const detail = makeReleaseDetail({
			progress: { finished: 4, total: 5, isDone: false },
			tasks: [{ id: 't1', ref: 'T-A38', type: 'task', title: 'Subset', status: 'Done' }]
		});
		expect(computeReleaseDetailReadiness(detail)).toEqual({
			finished: 4,
			total: 5,
			pct: 80,
			scopeLabel: 'Release readiness'
		});

		const notDoneBecauseEmptyMilestone = makeReleaseDetail({
			progress: { finished: 2, total: 2, isDone: false }
		});
		expect(computeReleaseDetailReadiness(notDoneBecauseEmptyMilestone)).toEqual({
			finished: 2,
			total: 2,
			pct: 99,
			scopeLabel: 'Release readiness'
		});
	});

	it('computes 100% readiness for a release whose explicit tasks have status "Done"', () => {
		const detail = makeReleaseDetail({
			tasks: [
				{ id: 't1', ref: 'T-A38', type: 'task', title: 'Receipt layout', status: 'Done' },
				{ id: 't2', ref: 'T-A39', type: 'task', title: 'Idempotency schema', status: 'Done' }
			]
		});
		expect(computeReleaseDetailReadiness(detail)).toEqual({
			finished: 2,
			total: 2,
			pct: 100,
			scopeLabel: 'Release readiness'
		});
	});

	it('deduplicates strictly by stable milestoneId so cross-release M-1 manifest tasks are never dropped', () => {
		// Beacon R-2: M-1 (ms-b2-1) has 1 done + 2 open; explicit tasks belong to ms-b2-1.
		const beaconR2 = makeReleaseDetail({
			milestones: [
				{
					id: 'ms-b2-1',
					number: 1,
					title: 'Split checkout & saved cards',
					status: 'in_progress',
					tasksDone: 1,
					tasksOpen: 2,
					tasksCancelled: 0
				}
			],
			tasks: [
				{
					id: 't40',
					ref: 'T-A40',
					type: 'task',
					title: 'Split checkout',
					status: 'In progress',
					milestoneId: 'ms-b2-1',
					milestoneRef: 'M-1'
				},
				{
					id: 't41',
					ref: 'T-A41',
					type: 'task',
					title: 'Saved cards',
					status: 'To do',
					milestoneId: 'ms-b2-1',
					milestoneRef: 'M-1'
				}
			]
		});
		expect(computeReleaseDetailReadiness(beaconR2)).toEqual({
			finished: 1,
			total: 3,
			pct: 33,
			scopeLabel: 'Release readiness'
		});

		// Cross-release manifest task: R-2 has M-1 (ms-r2-1, 1 done) AND explicitly includes an open
		// task belonging to R-1's M-1 (ms-r1-1). Must NOT be skipped -> 1/2 = 50%.
		const crossReleaseM1 = makeReleaseDetail({
			milestones: [
				{
					id: 'ms-r2-1',
					number: 1,
					title: 'R-2 Milestone 1',
					status: 'completed',
					tasksDone: 1,
					tasksOpen: 0,
					tasksCancelled: 0
				}
			],
			tasks: [
				{
					id: 't-from-r1',
					ref: 'T-A99',
					type: 'task',
					title: 'Carry-over open task from R-1 M-1',
					status: 'In progress',
					milestoneId: 'ms-r1-1',
					milestoneRef: 'M-1'
				}
			]
		});
		expect(computeReleaseDetailReadiness(crossReleaseM1)).toEqual({
			finished: 1,
			total: 2,
			pct: 50,
			scopeLabel: 'Release readiness'
		});

		// Ambiguous payload (milestoneRef present without milestoneId while milestones exist):
		// falls back honestly to 'Milestone readiness' instead of guessing or silently dropping.
		const ambiguousRef = makeReleaseDetail({
			milestones: crossReleaseM1.milestones,
			tasks: [
				{
					id: 't-ambig',
					ref: 'T-A98',
					type: 'task',
					title: 'Task with only release-scoped ref',
					status: 'In progress',
					milestoneRef: 'M-1'
				}
			]
		});
		expect(computeReleaseDetailReadiness(ambiguousRef)).toEqual({
			finished: 1,
			total: 1,
			pct: 100,
			scopeLabel: 'Milestone readiness'
		});
	});

	it('counts cancelled tasks as finished and prevents 100% when an empty milestone exists', () => {
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
			finished: 4,
			total: 5,
			pct: 80,
			scopeLabel: 'Release readiness'
		});

		// Per doc-76: a release with an empty milestone is not done (cannot report 100%).
		const withEmptyMilestone = makeReleaseDetail({
			tasks: [],
			milestones: [
				cancelledOnly,
				{
					id: 'ms-empty',
					number: 3,
					title: 'Unplanned follow-up',
					status: 'planned',
					tasksDone: 0,
					tasksOpen: 0,
					tasksCancelled: 0
				}
			]
		});
		expect(computeReleaseDetailReadiness(withEmptyMilestone)).toEqual({
			finished: 2,
			total: 2,
			pct: 99,
			scopeLabel: 'Release readiness'
		});
	});
});
