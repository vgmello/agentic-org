import { describe, expect, it } from 'vitest';
import {
	filterAndSortBreakdowns,
	filterAndSortOrgProjects,
	formatChangePercent,
	releaseCapText
} from './insights-helpers';
import type { InsightBreakdownRow, OrgInsightProjectRow, UsageTotals } from './types';

const makeTotals = (cost: number | null, tokens: number | null = 1000): UsageTotals => ({
	records: cost === null ? 1 : 5,
	knownCostMicroUsd: cost,
	knownTokens: tokens,
	cacheReadTokens: tokens ? Math.floor(tokens / 2) : null,
	tokenIncompleteRecords: 0,
	costIncompleteRecords: cost === null ? 1 : 0,
	unpricedRecords: 0,
	unknownCostRecords: cost === null ? 1 : 0,
	includesApiEquivalent: false
});

describe('filterAndSortBreakdowns', () => {
	const rows: InsightBreakdownRow[] = [
		{
			id: 'a',
			key: { dimension: 'team', teamId: 'platform' },
			label: 'Platform',
			totals: makeTotals(50_000_000, 5_000_000),
			itemsDone: 10,
			costPerTaskMicroUsd: 5_000_000,
			medianLeadSeconds: 7200,
			medianCycleSeconds: 3600,
			sampleCount: 10
		},
		{
			id: 'b',
			key: { dimension: 'team', teamId: 'web' },
			label: 'Web',
			totals: makeTotals(80_000_000, 2_000_000),
			itemsDone: 5,
			costPerTaskMicroUsd: 16_000_000,
			medianLeadSeconds: null,
			medianCycleSeconds: null,
			sampleCount: 3
		},
		{
			id: 'c',
			key: { dimension: 'team', notOnTask: true },
			label: 'Not on a task',
			overhead: true,
			totals: makeTotals(99_000_000, 9_000_000),
			itemsDone: null,
			costPerTaskMicroUsd: null,
			medianLeadSeconds: null,
			medianCycleSeconds: null,
			sampleCount: 0
		}
	];

	it('sorts by cost descending while pinning Not on a task last', () => {
		expect(filterAndSortBreakdowns(rows, '', 'cost', 'desc').map((r) => r.id)).toEqual([
			'b',
			'a',
			'c'
		]);
	});

	it('sorts unknown values last regardless of direction', () => {
		expect(filterAndSortBreakdowns(rows, '', 'medianLead', 'asc').map((r) => r.id)).toEqual([
			'a',
			'b',
			'c'
		]);
		expect(filterAndSortBreakdowns(rows, '', 'medianLead', 'desc').map((r) => r.id)).toEqual([
			'a',
			'b',
			'c'
		]);
	});

	it('filters case-insensitively by label', () => {
		expect(filterAndSortBreakdowns(rows, 'plat', 'cost', 'desc').map((r) => r.id)).toEqual(['a']);
	});
});

describe('filterAndSortOrgProjects', () => {
	const projects: OrgInsightProjectRow[] = [
		{
			projectId: 'atlas',
			projectNumber: 3,
			name: 'Atlas',
			accountableTeam: 'Platform',
			toDate: makeTotals(200_000_000),
			thisMonth: makeTotals(76_000_000),
			lastMonth: makeTotals(100_000_000),
			periodTotals: makeTotals(76_000_000),
			changePercent: -24,
			itemsDone: 12,
			medianLeadSeconds: 3600,
			sampleCount: 12
		},
		{
			projectId: 'beacon',
			projectNumber: 8,
			name: 'Beacon',
			accountableTeam: 'Web',
			toDate: makeTotals(90_000_000),
			thisMonth: makeTotals(25_000_000),
			lastMonth: makeTotals(10_000_000),
			periodTotals: makeTotals(25_000_000),
			changePercent: 150,
			itemsDone: 6,
			medianLeadSeconds: null,
			sampleCount: 3
		},
		{
			projectId: 'old',
			projectNumber: 1,
			name: 'Legacy',
			archived: true,
			accountableTeam: 'Platform',
			toDate: makeTotals(300_000_000),
			thisMonth: makeTotals(0),
			lastMonth: makeTotals(0),
			periodTotals: makeTotals(0),
			changePercent: null,
			itemsDone: 4,
			medianLeadSeconds: null,
			sampleCount: 4
		}
	];

	it('hides archived when includeArchived is false and sorts by change descending', () => {
		expect(
			filterAndSortOrgProjects(projects, '', false, 'change', 'desc').map((p) => p.projectId)
		).toEqual(['beacon', 'atlas']);
	});

	it('filters by project name, team, or P-number', () => {
		expect(
			filterAndSortOrgProjects(projects, 'P-8', true, 'name', 'asc').map((p) => p.projectId)
		).toEqual(['beacon']);
		expect(
			filterAndSortOrgProjects(projects, 'platform', true, 'toDate', 'desc').map((p) => p.projectId)
		).toEqual(['old', 'atlas']);
	});
});

describe('releaseCapText & formatChangePercent', () => {
	it('formats release cap with and without cap and release snapshot', () => {
		expect(
			releaseCapText({
				releaseId: 'r1',
				number: 1,
				title: 'v2.0',
				unit: 'usd',
				capLimit: 600_000_000,
				spent: 412_000_000,
				complete: true,
				releasedAt: '2026-11-14T11:00:00Z',
				spentAtRelease: 398_000_000
			})
		).toBe('Spent ≈ $412.00 of $600.00 cap · Spent at release ≈ $398.00');
		expect(
			releaseCapText({
				releaseId: 'r2',
				number: 2,
				title: 'v2.1',
				unit: 'usd',
				spent: 25_000_000,
				complete: true
			})
		).toBe('Spent ≈ $25.00');
	});

	it('formats signed change percents cleanly', () => {
		expect(formatChangePercent(18)).toBe('+18%');
		expect(formatChangePercent(-6)).toBe('−6%');
		expect(formatChangePercent(0)).toBe('0%');
		expect(formatChangePercent(null)).toBe('—');
	});
});
