import { describe, expect, it } from 'vitest';
import {
	checkDocumentDraft,
	checkSectionPath,
	deriveCanonicalPath,
	diffLines,
	diffSummary,
	expectedSectionsFor,
	groupDocsByKind,
	markdownBlocks,
	matchesDocFilter,
	parseDocumentSource,
	serializeDocumentSource,
	slugifyFileName
} from './present';
import type { DocumentDraft, ProjectDocumentRow } from './types';

const baseDraft: DocumentDraft = {
	kind: 'decision',
	title: 'Storage engine for offices',
	key: 'ADR-0002',
	summary: 'SQLite with one writer per office.',
	status: 'current',
	ownerId: 'sam',
	sectionPath: 'decisions',
	fileName: 'storage-engine.md',
	scope: { kind: 'project' },
	bodyMarkdown: '## Context\nNeed durable local storage.\n\n## Decision\nUse SQLite.\n'
};

describe('docs canonical path & validation', () => {
	it('derives canonical paths by scope and kind (DEC-1, TPL-8)', () => {
		expect(deriveCanonicalPath('beacon', 'doc-1', 'brief', { kind: 'project' }, null, null)).toBe(
			'projects/beacon/brief.md'
		);
		expect(
			deriveCanonicalPath(
				'beacon',
				'doc-2',
				'decision',
				{ kind: 'project' },
				'decisions',
				'adr-0002.md'
			)
		).toBe('projects/beacon/docs/decisions/adr-0002.md');
		expect(
			deriveCanonicalPath(
				'beacon',
				'doc-3',
				'design',
				{ kind: 'feature', featureId: 'feat-1', ref: 'F-6', title: 'Checkout' },
				null,
				null
			)
		).toBe('projects/beacon/features/feat-1/designs/doc-3.md');
		expect(
			deriveCanonicalPath(
				'beacon',
				'doc-4',
				'changelog',
				{ kind: 'release', releaseId: 'rel-1', ref: 'R-1', title: 'v1.0' },
				null,
				null
			)
		).toBe('projects/beacon/releases/rel-1/changelog.md');
	});

	it('requires an owner for Current status and validates tree filename/section', () => {
		expect(checkDocumentDraft(baseDraft)).toEqual({});
		expect(checkDocumentDraft({ ...baseDraft, ownerId: null })).toHaveProperty('ownerId');
		expect(checkDocumentDraft({ ...baseDraft, title: '   ' })).toHaveProperty('title');
		expect(checkDocumentDraft({ ...baseDraft, fileName: '../escape.md' })).toHaveProperty(
			'fileName'
		);
		expect(checkSectionPath('product/checkout')).toBeUndefined();
		expect(checkSectionPath('Bad//Path')).toBeTruthy();
	});

	it('slugifies titles to valid markdown filenames', () => {
		expect(slugifyFileName('Checkout retry policy!')).toBe('checkout-retry-policy.md');
		expect(slugifyFileName('   ')).toBe('untitled.md');
	});
});

describe('docs front-matter synchronization (TPL-2)', () => {
	it('round-trips Details fields through Source mode without losing immutable identity', () => {
		const source = serializeDocumentSource(baseDraft, {
			id: '0193d000-0000-7000-8000-000000000002',
			revision: 3,
			projectId: 'beacon',
			path: 'projects/beacon/docs/decisions/storage-engine.md'
		});
		const edited = source
			.replace('title: Storage engine for offices', 'title: "Storage: SQLite, one writer"')
			.replace('status: current', 'status: draft')
			.replace('Use SQLite.', 'Use SQLite in WAL mode.');
		const parsed = parseDocumentSource(edited, baseDraft);
		expect(parsed.ok).toBe(true);
		if (parsed.ok) {
			expect(parsed.draft.title).toBe('Storage: SQLite, one writer');
			expect(parsed.draft.status).toBe('draft');
			expect(parsed.draft.bodyMarkdown).toContain('Use SQLite in WAL mode.');
			expect(parsed.draft.kind).toBe('decision');
		}
	});

	it('rejects invalid lifecycle values or missing front matter delimiters', () => {
		const badStatus = serializeDocumentSource(baseDraft, {
			id: 'id-1',
			revision: 1,
			projectId: 'beacon',
			path: 'projects/beacon/docs/decisions/a.md'
		}).replace('status: current', 'status: accepted');
		expect(parseDocumentSource(badStatus, baseDraft).ok).toBe(false);
		expect(parseDocumentSource('## Just body', baseDraft).ok).toBe(false);
	});
});

describe('docs grouping, expected sections, markdown & diff', () => {
	const mkRow = (over: Partial<ProjectDocumentRow>): ProjectDocumentRow => ({
		id: 'd1',
		projectId: 'beacon',
		key: null,
		kind: 'decision',
		title: 'Decision one',
		summary: '',
		status: 'current',
		owner: { id: 'sam', name: 'Sam Okafor', kind: 'agent' },
		path: 'projects/beacon/docs/decisions/one.md',
		sectionPath: 'decisions',
		scope: { kind: 'project' },
		revision: 1,
		version: null,
		issuedRevision: null,
		hasUnissuedChanges: false,
		createdAt: '2026-11-10T10:00:00Z',
		createdBy: { id: 'sam', name: 'Sam Okafor', kind: 'agent' },
		updatedAt: '2026-11-12T10:00:00Z',
		updatedBy: { id: 'sam', name: 'Sam Okafor', kind: 'agent' },
		...over
	});

	it('pins Brief, groups active docs by kind, and collapses Deprecated under Older (DEC-1, DEC-8)', () => {
		const grouped = groupDocsByKind(
			[
				mkRow({ id: 'b1', kind: 'brief', title: 'Beacon brief', path: 'projects/beacon/brief.md' }),
				mkRow({ id: 'd1', kind: 'decision', key: 'ADR-0001', title: 'Active ADR' }),
				mkRow({
					id: 'd2',
					kind: 'decision',
					key: 'ADR-0000',
					title: 'Old ADR',
					status: 'deprecated'
				}),
				mkRow({ id: 'r1', kind: 'report', title: 'Eval report' })
			],
			[]
		);
		expect(grouped.brief?.id).toBe('b1');
		expect(grouped.groups.map((g) => g.label)).toEqual(['Decisions', 'Reports']);
		expect(grouped.deprecated.map((d) => d.id)).toEqual(['d2']);
	});

	it('filters by status, owner, and query matching title, key, ID, or body preview', () => {
		const row = mkRow({
			id: '0193d000-abc',
			key: 'ADR-0004',
			title: 'Idempotent payment retries',
			bodyPreview: 'Use client-generated operation IDs.'
		});
		expect(matchesDocFilter(row, { status: 'all', ownerId: '', query: 'ADR-0004' })).toBe(true);
		expect(matchesDocFilter(row, { status: 'all', ownerId: '', query: 'operation IDs' })).toBe(
			true
		);
		expect(matchesDocFilter(row, { status: 'draft', ownerId: '', query: '' })).toBe(false);
	});

	it('marks missing expected sections and renders safe markdown blocks', () => {
		const sections = expectedSectionsFor('decision', '## Context\nWhy.\n\n## Consequences\n');
		expect(sections.find((s) => s.heading === 'Context')?.empty).toBe(false);
		expect(sections.find((s) => s.heading === 'Consequences')?.empty).toBe(true);
		expect(sections.find((s) => s.heading === 'Decision')?.present).toBe(false);

		const blocks = markdownBlocks('## Goal\nShip fast.\n- Step one\n```ts\nconst x = 1;\n```');
		expect(blocks.map((b) => b.kind)).toEqual(['heading', 'paragraph', 'list', 'code']);
	});

	it('computes line diffs and readable summaries', () => {
		const lines = diffLines('Line 1\nOld line\nLine 3', 'Line 1\nNew line\nLine 3\nExtra');
		expect(diffSummary(lines)).toBe('2 lines added, 1 line removed');
		expect(diffSummary(diffLines('Same', 'Same'))).toBe('No differences');
	});
});
