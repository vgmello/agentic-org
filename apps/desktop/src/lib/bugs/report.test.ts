import { describe, expect, it } from 'vitest';
import { createMockCopilot } from '$lib/copilot/mock';
import { COPILOT_LIMITS } from '$lib/copilot/types';
import {
	BUG_LIMITS,
	createMockBugs,
	emptyForm,
	formFromDraft,
	isBlank,
	stepLines,
	submitReport,
	validateForm,
	type BugApi,
	type BugForm
} from './report';

const good = (extra: Partial<BugForm> = {}): BugForm => ({
	title: 'Save fails',
	actual: 'Nothing happens.',
	expected: 'The file is saved.',
	steps: 'Open a file\nPress save',
	severity: 'high',
	...extra
});

describe('the form', () => {
	it('starts empty, and blank means nothing typed', () => {
		expect(isBlank(emptyForm())).toBe(true);
		expect(isBlank({ ...emptyForm(), severity: 'critical' })).toBe(true);
		expect(isBlank({ ...emptyForm(), title: 'x' })).toBe(false);
	});

	it('steps are one per line, trimmed, without blank lines', () => {
		expect(stepLines(' a \n\n  b\n')).toEqual(['a', 'b']);
	});

	it('a draft fills the form, and the form turns back into the same report', () => {
		const form = formFromDraft({ title: 'T', actual: 'A', expected: 'E', steps: ['one', 'two'], severity: 'low' });
		expect(form).toEqual({ title: 'T', actual: 'A', expected: 'E', steps: 'one\ntwo', severity: 'low' });
		const checked = validateForm(form);
		expect(checked.ok && checked.report).toEqual({ title: 'T', actual: 'A', expected: 'E', steps: ['one', 'two'], severity: 'low' });
	});
});

describe('a draft that does not fit the form as it is', () => {
	it('keeps a step with a line break in it as one step', () => {
		const form = formFromDraft({ title: 'T', actual: 'A', expected: '', steps: ['one\ntwo', 'three'], severity: 'low' });
		expect(stepLines(form.steps)).toEqual(['one two', 'three']);
		expect(validateForm(form).ok).toBe(true);
	});
});

describe('validateForm', () => {
	it('accepts a good form and trims it', () => {
		const checked = validateForm(good({ title: '  Save fails  ' }));
		expect(checked.ok && checked.report.title).toBe('Save fails');
	});

	it('needs a title and what happens, but not what was expected or steps', () => {
		const checked = validateForm(good({ title: ' ', actual: '', expected: '', steps: '' }));
		expect(checked.ok).toBe(false);
		expect(!checked.ok && Object.keys(checked.errors)).toEqual(['title', 'actual']);
		expect(validateForm(good({ expected: '', steps: '' })).ok).toBe(true);
	});

	it('holds the same caps as the copilot’s draft', () => {
		const errors = (f: BugForm) => {
			const c = validateForm(f);
			return c.ok ? [] : Object.keys(c.errors);
		};
		expect(errors(good({ title: 'x'.repeat(BUG_LIMITS.title + 1) }))).toEqual(['title']);
		expect(errors(good({ actual: 'x'.repeat(BUG_LIMITS.actual + 1) }))).toEqual(['actual']);
		expect(errors(good({ expected: 'x'.repeat(BUG_LIMITS.expected + 1) }))).toEqual(['expected']);
		expect(errors(good({ steps: Array.from({ length: 11 }, (_, i) => `s${i}`).join('\n') }))).toEqual(['steps']);
		expect(errors(good({ steps: 'x'.repeat(BUG_LIMITS.step + 1) }))).toEqual(['steps']);
		expect(errors(good({ severity: 'urgent' as never }))).toEqual(['severity']);
	});

	it('every draft the mock copilot makes passes the form', async () => {
		const copilot = createMockCopilot({ delayMs: 0, scenario: 'on' });
		const sentences = ['a'.repeat(COPILOT_LIMITS.sentenceMaxChars), 'It crashed!', "Can't save.\nSecond line."];
		for (const sentence of sentences) {
			const { draft } = await copilot.draft('bug.report', { sentence });
			expect(validateForm(formFromDraft(draft)).ok, sentence.slice(0, 20)).toBe(true);
		}
	});
});

describe('submitReport', () => {
	it('reports a good form after the person confirms, and returns the id', async () => {
		const api = createMockBugs({ delayMs: 0 });
		const sent = await submitReport(api, good());
		expect(sent).toMatchObject({ kind: 'sent', id: 'BUG-1' });
		const next = await submitReport(api, good());
		expect(next).toMatchObject({ kind: 'sent', id: 'BUG-2' });
	});

	it('does not call the api for an invalid form', async () => {
		let called = false;
		const api: BugApi = { sample: true, report: async () => ((called = true), { id: 'x' }) };
		const result = await submitReport(api, good({ title: '' }));
		expect(result).toEqual({ kind: 'invalid', errors: { title: 'Give the bug a title.' } });
		expect(called).toBe(false);
	});

	it('a failure ends in failed and never throws', async () => {
		const api: BugApi = { sample: true, report: () => Promise.reject({ source: 'api', code: 'storage_error', message: 'disk' }) };
		expect(await submitReport(api, good())).toEqual({
			kind: 'failed',
			error: { source: 'api', code: 'storage_error', message: 'disk' }
		});
	});

	it('says it is sample data', () => {
		expect(createMockBugs().sample).toBe(true);
	});
});
