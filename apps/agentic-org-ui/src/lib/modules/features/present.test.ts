import { describe, expect, it } from 'vitest';
import {
	checkFeatureSlug,
	defaultStatusFilters,
	featureProgressStats,
	featurePublicSlug,
	featureRef,
	groupFeatures,
	matchesFeatureFilter,
	slugifyFeature,
	validateRetirement
} from './present';
import type { FeatureRow } from './types';

const mkFeature = (over: Partial<FeatureRow>): FeatureRow => ({
	id: 'feat-1',
	projectId: 'beacon',
	projectNumber: 8,
	projectSlug: 'beacon',
	number: 6,
	localSlug: 'quick-capture',
	title: 'Quick capture of requests',
	status: 'in_progress',
	owner: { id: 'mira', name: 'Mira Castillo', kind: 'agent' },
	workType: 'build',
	progress: { done: 4, open: 2, blocked: 0, cancelled: 1 },
	origins: [],
	deliveredAt: null,
	retiredReason: null,
	replacedBy: null,
	updatedAt: '2026-11-18T10:00:00Z',
	updatedBy: { id: 'mira', name: 'Mira Castillo', kind: 'agent' },
	...over
});

describe('features identity, progress & filtering (FTR-1, FTR-10, FTR-13)', () => {
	it('formats F- references, pending number, and derived public slug', () => {
		expect(featureRef({ number: 6 })).toBe('F-6');
		expect(featureRef({ number: null })).toBe('Pending number');
		expect(featurePublicSlug({ projectSlug: 'beacon', localSlug: 'quick-capture' })).toBe(
			'beacon/quick-capture'
		);
		expect(slugifyFeature('Two-Tap Mobile Checkout!')).toBe('two-tap-mobile-checkout');
		expect(checkFeatureSlug('valid-slug')).toBeUndefined();
		expect(checkFeatureSlug('Bad--Slug')).toBeTruthy();
	});

	it('excludes cancelled tasks from progress denominator and never shows 100% for empty eligible set', () => {
		expect(featureProgressStats({ done: 4, open: 1, blocked: 1, cancelled: 3 })).toEqual({
			eligible: 6,
			done: 4,
			percent: 67,
			label: '4 of 6 tasks completed',
			cancelledNote: '3 cancelled (excluded from progress)'
		});
		expect(featureProgressStats({ done: 0, open: 0, blocked: 0, cancelled: 2 })).toEqual({
			eligible: 0,
			done: 0,
			percent: 0,
			label: 'No tasks yet',
			cancelledNote: '2 cancelled (excluded from progress)'
		});
	});

	it('hides Retired by default and searches by reference, title, and slug', () => {
		const defaults = defaultStatusFilters();
		expect(defaults.has('retired')).toBe(false);
		const row = mkFeature({});
		expect(matchesFeatureFilter(row, { statuses: defaults, query: 'F-6' })).toBe(true);
		expect(matchesFeatureFilter(row, { statuses: defaults, query: 'beacon/quick-capture' })).toBe(
			true
		);
		expect(
			matchesFeatureFilter(mkFeature({ status: 'retired' }), { statuses: defaults, query: '' })
		).toBe(false);
	});

	it('groups features in In progress, Planned, Live, Retired order', () => {
		const groups = groupFeatures([
			mkFeature({ id: 'a', status: 'live' }),
			mkFeature({ id: 'b', status: 'planned' }),
			mkFeature({ id: 'c', status: 'in_progress' })
		]);
		expect(groups.map((g) => g.status)).toEqual(['in_progress', 'planned', 'live']);
	});
});

describe('retirement validation (FTR-8)', () => {
	const list = [
		mkFeature({ id: 'f1', replacedBy: { id: 'f2', ref: 'F-2', title: 'F2' } }),
		mkFeature({ id: 'f2', replacedBy: null }),
		mkFeature({ id: 'f3', replacedBy: null })
	];

	it('requires a reason and prevents self-replacement or cycles', () => {
		expect(validateRetirement('', null, 'f2', list)).toMatch(/Enter a reason/);
		expect(validateRetirement('Superseded', 'f2', 'f2', list)).toMatch(/replace itself/);
		expect(validateRetirement('Superseded', 'f1', 'f2', list)).toMatch(/cycle/);
		expect(validateRetirement('Superseded', 'f3', 'f2', list)).toBeUndefined();
	});
});
