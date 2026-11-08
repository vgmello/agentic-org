import { describe, expect, it } from 'vitest';
import { applyDraft, BUG_ENVIRONMENT_MAX_CHARS, clippedFields, prefillFromDraft, undoDraft, type BugFields } from './bug-prefill';
import { createMockCopilot } from './mock';
import type { BugDraft } from './types';
import { WORK_LIMITS } from '$lib/proto';

const limits = { title: WORK_LIMITS.title, text: WORK_LIMITS.text, environment: BUG_ENVIRONMENT_MAX_CHARS };
const areas = [{ id: 'general' }, { id: 'files' }];
const mine: BugFields = {
	title: 'my title',
	happened: 'my words',
	severity: '',
	area: 'general',
	expected: 'my expectation',
	steps: 'my steps',
	environment: 'This Mac (HQ) · Agentic Org 0.1'
};
const draft = (extra: Partial<BugDraft> = {}): BugDraft => ({ title: 'Save fails', what_happened: 'Nothing happens.', severity: 'major', ...extra });

describe('prefillFromDraft', () => {
	it('always sets the title, what happened and the severity from the draft', () => {
		const f = prefillFromDraft(mine, draft(), areas, limits);
		expect([f.title, f.happened, f.severity]).toEqual(['Save fails', 'Nothing happens.', 'major']);
	});

	it('leaves what the draft does not say as the person (or the app) had it', () => {
		const f = prefillFromDraft(mine, draft(), areas, limits);
		expect([f.expected, f.steps, f.environment, f.area]).toEqual(['my expectation', 'my steps', 'This Mac (HQ) · Agentic Org 0.1', 'general']);
	});

	it('takes optional parts when the draft has them, steps as one string', () => {
		const f = prefillFromDraft(mine, draft({ steps: 'Open a file\nPress save', expected: 'It saves', environment: 'macOS 15', area_id: 'files' }), areas, limits);
		expect(f).toMatchObject({ steps: 'Open a file\nPress save', expected: 'It saves', environment: 'macOS 15', area: 'files' });
	});

	it('ignores an area that is not one of the project’s, and blank optional parts', () => {
		const f = prefillFromDraft(mine, draft({ area_id: 'invented', steps: '  ', expected: '' }), areas, limits);
		expect([f.area, f.steps, f.expected]).toEqual(['general', 'my steps', 'my expectation']);
	});

	it('fits a long draft to the field limits', () => {
		const f = prefillFromDraft(mine, draft({ title: 'x'.repeat(500) }), areas, limits);
		expect(f.title.length).toBe(WORK_LIMITS.title);
	});

	it('fills the form from what the mock copilot drafts, naming an area only from the context', async () => {
		const copilot = createMockCopilot({ delayMs: 0, scenario: 'on' });
		const context = [
			{ kind: 'area' as const, label: 'Files', text: 'files' },
			{ kind: 'area' as const, label: 'General', text: 'general' }
		];
		const { draft: d } = await copilot.draft('bug.from_sentence', { sentence: "Saving in Files doesn't work" }, context);
		const f = prefillFromDraft(mine, d, areas, limits);
		expect(f).toMatchObject({ area: 'files', severity: 'major', title: "Saving in Files doesn't work" });
	});
});

describe('Undo', () => {
	it('brings back the person’s own fields', () => {
		const first = applyDraft(undefined, mine, draft(), areas, limits);
		expect(first.fields.title).toBe('Save fails');
		expect(undoDraft(first.before, first.fields)).toEqual(mine);
	});

	it('after a second draft still brings back the person’s own text, not the first draft', () => {
		const first = applyDraft(undefined, mine, draft(), areas, limits);
		const second = applyDraft(first.before, first.fields, draft({ title: 'Second try' }), areas, limits);
		expect(second.fields.title).toBe('Second try');
		expect(undoDraft(second.before, second.fields)).toEqual(mine);
	});

	it('does nothing when there was no draft', () => {
		expect(undoDraft(undefined, mine)).toBe(mine);
	});
});

describe('clippedFields', () => {
	it('names the fields whose draft was cut to fit, and nothing when all fit', () => {
		expect(clippedFields(draft(), limits)).toEqual([]);
		const long = draft({ title: 'x'.repeat(WORK_LIMITS.title + 1), steps: 'y'.repeat(WORK_LIMITS.text + 1) });
		expect(clippedFields(long, limits)).toEqual(['Title', 'Steps to reproduce']);
		expect(applyDraft(undefined, mine, long, areas, limits).clipped).toEqual(['Title', 'Steps to reproduce']);
	});
});
