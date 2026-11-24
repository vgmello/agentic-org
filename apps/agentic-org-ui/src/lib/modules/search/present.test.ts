import { describe, expect, it } from 'vitest';
import { coverageStatement, extractSearchTerms, groupKnowledgeHits, isWhyQuery } from './present';
import type { KnowledgeHit } from './types';

describe('knowledge search helpers (WHY-4)', () => {
	it('detects "Why is…" questions and extracts meaningful terms', () => {
		expect(isWhyQuery('Why is SQLite used for office storage?')).toBe(true);
		expect(isWhyQuery('Why did we split checkout?')).toBe(true);
		expect(isWhyQuery('ADR-0002')).toBe(false);
		expect(extractSearchTerms('Why is SQLite used for office storage?')).toEqual([
			'sqlite',
			'office',
			'storage'
		]);
		expect(extractSearchTerms('ADR-0002')).toEqual(['adr-0002']);
	});

	it('groups knowledge hits by record type with Decisions and Evaluations first', () => {
		const hit = (over: Partial<KnowledgeHit>): KnowledgeHit => ({
			id: '1',
			recordType: 'decision',
			kindLabel: 'Decision',
			key: 'ADR-0002',
			title: 'SQLite storage',
			scopeLabel: 'Beacon',
			status: 'Current',
			updatedAt: '2026-11-15T10:00:00Z',
			excerpt: null,
			href: '/projects/beacon/docs/1',
			...over
		});
		const groups = groupKnowledgeHits([
			hit({ id: 'f1', recordType: 'feature', kindLabel: 'Feature', title: 'Checkout' }),
			hit({ id: 'd1', recordType: 'decision', kindLabel: 'Decision', title: 'SQLite' }),
			hit({ id: 'e1', recordType: 'evaluation', kindLabel: 'Evaluation', title: 'Turso vs SQLite' })
		]);
		expect(groups.map((g) => g.label)).toEqual(['Decisions', 'Evaluations', 'Features']);
	});

	it('states title-only and full-body coverage honestly and names exclusions', () => {
		expect(coverageStatement({ scope: 'title_only', indexedAt: '2026-11-20T10:00:00Z' })).toContain(
			'Title-only search'
		);
		expect(
			coverageStatement({
				scope: 'full_body',
				indexedAt: '2026-11-20T10:00:00Z',
				truncatedBodies: true
			})
		).toContain('Private task notes, protected chat and inaccessible records are excluded');
	});
});
