import { describe, expect, it } from 'vitest';
import {
	axisLevel,
	contiguousRuns,
	niceTicks,
	smoothAreaPath,
	smoothCurvePath
} from './chart-math';

describe('chart-math', () => {
	it('computes clean rounded niceTicks from 0 to a rounded top', () => {
		expect(niceTicks(0)).toEqual([0, 1]);
		expect(niceTicks(18, true)).toEqual([0, 5, 10, 15, 20]);
		expect(niceTicks(3.7)).toEqual([0, 1, 2, 3, 4]);
		expect(niceTicks(120, true)).toEqual([0, 50, 100, 150]);
	});

	it('assigns nested axisLevel visibility with the latest point always few', () => {
		expect(axisLevel(7, 8)).toBe('few');
		expect(axisLevel(0, 1)).toBe('few');
		expect(axisLevel(5, 8)).toBe('some');
		expect(axisLevel(6, 8)).toBe('many');
	});

	it('splits contiguousRuns cleanly across missing values', () => {
		const runs = contiguousRuns([1, 2, null, 4, 5, 6, null, 8], (v) => v !== null);
		expect(runs).toEqual([[1, 2], [4, 5, 6], [8]]);
	});

	it('produces clamped Bezier curve and closed area paths that never overshoot', () => {
		const pts = [
			{ x: 0, y: 100 },
			{ x: 50, y: 0 },
			{ x: 100, y: 0 },
			{ x: 150, y: 100 }
		];
		const d = smoothCurvePath(pts);
		expect(d).toContain('M 0.00 100.00');
		expect(d).toContain('C ');
		expect(d).not.toMatch(/-\d/);
		const area = smoothAreaPath(pts, 100);
		expect(area.endsWith('L 150.00 100.00 L 0.00 100.00 Z')).toBe(true);
	});
});
