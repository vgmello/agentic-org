import { describe, expect, it } from 'vitest';
import {
	appliesText,
	canOverride,
	checkDraft,
	clashText,
	conflictLatest,
	diffLines,
	diffSummary,
	documentHref,
	listHref,
	markdownBlocks,
	newDocumentHref,
	normalizeDraft,
	parseKind,
	parsePlace,
	placeLabel,
	placeParam,
	problemText,
	refusedField,
	relationText,
	sameDraft,
	skillFileText,
	skillRolesText,
	sourceText,
	validKey
} from './present';
import type { DocumentDraft, EffectiveRow, ProcessDocument } from './types';

const doc = (over: Partial<ProcessDocument> = {}): ProcessDocument => ({
	id: 'doc-1',
	key: 'security-basics',
	revision: 1,
	kind: 'process',
	title: 'Security basics',
	status: 'current',
	level: 'organization',
	overridable: true,
	bodyMarkdown: '',
	path: 'processes/doc-1.md',
	created: { at: '2026-11-01T00:00:00Z', byKind: 'system' },
	updated: { at: '2026-11-01T00:00:00Z', byKind: 'system' },
	...over
});
const row = (over: Partial<EffectiveRow> = {}): EffectiveRow => ({
	id: 'security-basics',
	document: doc(),
	applies: true,
	here: false,
	chain: [],
	...over
});

describe('places in the address (PROC-31, PROC-34)', () => {
	it('reads a team, project or agent and falls back to the organization', () => {
		expect(parsePlace('team:web')).toEqual({ kind: 'team', id: 'web' });
		expect(parsePlace('project:atlas')).toEqual({ kind: 'project', id: 'atlas' });
		expect(parsePlace('agent:noor')).toEqual({ kind: 'agent', id: 'noor' });
		expect(parsePlace(null)).toEqual({ kind: 'organization' });
		expect(parsePlace('office:x')).toEqual({ kind: 'organization' });
		expect(parsePlace('team:')).toEqual({ kind: 'organization' });
		expect(placeParam({ kind: 'organization' })).toBeUndefined();
	});

	it('labels each Show option as the spec does', () => {
		expect(placeLabel({ place: { kind: 'organization' }, name: 'Acme' })).toBe('Organization');
		expect(placeLabel({ place: { kind: 'team', id: 'web' }, name: 'Web' })).toBe('Web (team)');
		expect(placeLabel({ place: { kind: 'project', id: 'a' }, name: 'Atlas' })).toBe(
			'Atlas (project)'
		);
		expect(placeLabel({ place: { kind: 'agent', id: 'n' }, name: 'Noor Haddad' })).toBe(
			'Noor Haddad (agent)'
		);
	});

	it('builds the list, editor and override addresses', () => {
		expect(listHref({ kind: 'organization' })).toBe('/settings/processes');
		expect(listHref({ kind: 'team', id: 'web' }, 'standard')).toBe(
			'/settings/processes?place=team%3Aweb&kind=standard'
		);
		expect(newDocumentHref({ kind: 'team', id: 'web' }, 'process', { from: 'doc-1' })).toBe(
			'/settings/processes/documents/new?place=team%3Aweb&kind=process&from=doc-1'
		);
		expect(documentHref('doc 1', { kind: 'organization' }, 'process')).toBe(
			'/settings/processes/documents/doc%201'
		);
		expect(parseKind('standard')).toBe('standard');
		expect(parseKind('nope')).toBe('process');
	});
});

describe('rows (PROC-9, PROC-16)', () => {
	it('names the source with its team or project', () => {
		expect(sourceText(doc())).toBe('Organization');
		expect(sourceText(doc({ level: 'team', scopeName: 'Web' }))).toBe('Team Web');
		expect(sourceText(doc({ level: 'project', scopeName: 'Atlas' }))).toBe('Project Atlas');
	});

	it('says how a row relates to the level above', () => {
		expect(relationText(row({ replaces: doc() }))).toBe('replaces Organization');
		expect(relationText(row({ here: true }))).toBe('added here');
		expect(relationText(row())).toBe('inherited');
		expect(appliesText(row())).toBe('Applies');
		expect(appliesText(row({ applies: false }))).toBe('Not applied');
	});

	it('words a refused replacement, invalid details and a key clash as the spec does', () => {
		const platform = doc({ id: 'doc-2', level: 'team', scopeName: 'Platform' });
		expect(
			problemText(
				row({ document: platform, applies: false, problem: { kind: 'refused', by: doc() } })
			)
		).toBe(
			"Platform's version of Security basics isn't applied: the organization marked it not overridable. You can add a process instead."
		);
		expect(
			problemText(
				row({
					document: doc({ title: 'Design handoff' }),
					problem: { kind: 'invalid', field: 'overridable' }
				})
			)
		).toBe('Design handoff: details not recognized (overridable).');
		expect(clashText('process', 'review-and-merge', 'Checkout')).toBe(
			'Two processes use the key review-and-merge in Checkout. Neither applies until one is renamed.'
		);
	});

	it('offers Override only for an overridable, applying document from above a team or project', () => {
		const team = { kind: 'team', id: 'web' } as const;
		expect(canOverride(row(), team)).toBe(true);
		expect(canOverride(row(), { kind: 'organization' })).toBe(false);
		expect(canOverride(row(), { kind: 'agent', id: 'noor' })).toBe(false);
		expect(canOverride(row({ document: doc({ overridable: false }) }), team)).toBe(false);
		expect(canOverride(row({ here: true }), team)).toBe(false);
		expect(canOverride(row({ document: doc({ key: undefined }) }), team)).toBe(false);
	});
});

describe('the editor (PROC-1)', () => {
	const draft: DocumentDraft = {
		kind: 'process',
		title: '  Code review ',
		key: '  ',
		status: 'current',
		ownerId: 'sam',
		overridable: true,
		bodyMarkdown: 'Text\n\n\n'
	};

	it('trims the typed text and drops an empty key', () => {
		expect(normalizeDraft(draft)).toMatchObject({
			title: 'Code review',
			key: undefined,
			bodyMarkdown: 'Text\n'
		});
		expect(sameDraft(draft, { ...draft, title: 'Code review' })).toBe(true);
	});

	it('asks for a title, a valid key and an owner for a current document', () => {
		expect(checkDraft(normalizeDraft(draft))).toEqual({});
		expect(checkDraft({ ...draft, title: '' }).title).toBeDefined();
		expect(checkDraft({ ...draft, key: 'Code Review' }).key).toBeDefined();
		expect(checkDraft({ ...draft, ownerId: undefined }).ownerId).toBeDefined();
		expect(checkDraft({ ...draft, ownerId: undefined, status: 'draft' }).ownerId).toBeUndefined();
	});

	it('reads the refused field and a conflict from an error', () => {
		expect(refusedField({ field: 'key' })).toBe('key');
		expect(refusedField({ field: 'unknown' })).toBeUndefined();
		expect(conflictLatest({ code: 'conflict', latest: doc({ revision: 4 }) })?.revision).toBe(4);
		expect(conflictLatest({ code: 'invalid_request' })).toBeUndefined();
	});

	it('checks development process keys', () => {
		expect(validKey('dss-payments')).toBe(true);
		expect(validKey('DSS')).toBe(false);
		expect(validKey('a--b')).toBe(false);
		expect(validKey('a'.repeat(65))).toBe(false);
	});
});

describe('the difference from the parent', () => {
	it('marks added and removed lines and keeps the common ones', () => {
		const lines = diffLines('one\ntwo\nthree\n', 'one\nthree\nfour\n');
		expect(lines).toEqual([
			{ kind: 'same', text: 'one' },
			{ kind: 'removed', text: 'two' },
			{ kind: 'same', text: 'three' },
			{ kind: 'added', text: 'four' }
		]);
		expect(diffSummary(lines)).toBe('1 line added, 1 line removed');
		expect(diffSummary(diffLines('a', 'a'))).toBe('No differences');
	});
});

describe('reading Markdown as elements', () => {
	it('splits headings, ordered and unordered lists, and paragraphs', () => {
		expect(markdownBlocks('## Purpose\nWhy\nit is.\n\n1. one\n2. two\n- a\n')).toEqual([
			{ kind: 'heading', text: 'Purpose' },
			{ kind: 'paragraph', text: 'Why it is.' },
			{ kind: 'list', ordered: true, items: ['one', 'two'] },
			{ kind: 'list', ordered: false, items: ['a'] }
		]);
	});
});

describe('development process files', () => {
	it('writes a skill file with its front matter', () => {
		expect(
			skillFileText({
				key: 'staff',
				title: 'Staff the project',
				description: 'Use when: both are issued.',
				roles: ['product-lead', 'technical-lead'],
				bodyMarkdown: '## Steps\n'
			})
		).toBe(
			'---\nschema_version: 1\nkey: staff\ntitle: Staff the project\ndescription: "Use when: both are issued."\nroles: [product-lead, technical-lead]\n---\n## Steps\n'
		);
	});

	it('names the roles a skill is for', () => {
		const names = new Map([['product-lead', 'Product lead']]);
		expect(skillRolesText([], names)).toBe('every role');
		expect(skillRolesText(['product-lead', 'other'], names)).toBe('Product lead, other');
	});
});
