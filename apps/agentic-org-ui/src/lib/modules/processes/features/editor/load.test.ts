import { describe, expect, it } from 'vitest';
import { REVIEW_AND_MERGE } from '../../present';
import type { ProcessDocument } from '../../types';
import { scopeOf, startingDraft } from './load';

const parent: ProcessDocument = {
	id: 'doc-1',
	key: 'code-review',
	revision: 3,
	kind: 'process',
	title: 'Code review',
	status: 'current',
	owner: { id: 'sam', name: 'Sam Okafor' },
	level: 'organization',
	overridable: true,
	bodyMarkdown: '## Purpose\nReview.\n',
	path: 'processes/doc-1.md',
	created: { at: '2026-11-01T00:00:00Z', byKind: 'system' },
	updated: { at: '2026-11-01T00:00:00Z', byKind: 'system' }
};

describe('the editor starting draft', () => {
	it('places a new document at the team or project chosen, and nowhere for an agent', () => {
		expect(scopeOf({ kind: 'team', id: 'web' })).toEqual({ teamId: 'web' });
		expect(scopeOf({ kind: 'project', id: 'atlas' })).toEqual({ projectId: 'atlas' });
		expect(scopeOf({ kind: 'agent', id: 'noor' })).toEqual({});
		expect(scopeOf({ kind: 'organization' })).toEqual({});
	});

	it('starts an override from the inherited text, with the same key at the lower level', () => {
		const draft = startingDraft(
			{ kind: 'new', place: { kind: 'team', id: 'web' }, documentKind: 'process', from: 'doc-1' },
			{ document: parent }
		);
		expect(draft).toMatchObject({
			key: 'code-review',
			title: 'Code review',
			teamId: 'web',
			ownerId: 'sam',
			bodyMarkdown: parent.bodyMarkdown,
			overridable: true
		});
	});

	it('starts from the review-and-merge template when asked', () => {
		const draft = startingDraft({
			kind: 'new',
			place: { kind: 'organization' },
			documentKind: 'process',
			template: 'review-and-merge'
		});
		expect(draft.key).toBe('review-and-merge');
		expect(draft.bodyMarkdown).toBe(REVIEW_AND_MERGE.bodyMarkdown);
	});

	it('edits a document as it is', () => {
		const draft = startingDraft(
			{
				kind: 'edit',
				documentId: 'doc-1',
				place: { kind: 'organization' },
				documentKind: 'process'
			},
			undefined,
			{ document: parent }
		);
		expect(draft.ownerId).toBe('sam');
		expect(draft.status).toBe('current');
	});
});

describe('the editor address', () => {
	it('reads a new override and an edit from the URL', async () => {
		const { editorMode } = await import('./load');
		expect(
			editorMode(
				new URL('http://x/settings/processes/documents/new?place=team:web&kind=process&from=doc-1')
			)
		).toEqual({
			kind: 'new',
			place: { kind: 'team', id: 'web' },
			documentKind: 'process',
			from: 'doc-1',
			template: undefined
		});
		expect(editorMode(new URL('http://x/?kind=standard'), 'doc-9')).toEqual({
			kind: 'edit',
			documentId: 'doc-9',
			place: { kind: 'organization' },
			documentKind: 'standard'
		});
	});
});
