import { describe, expect, it } from 'vitest';
import { toAppError } from '$lib/office/api';
import { createMockCopilot, guessSeverity, tidy, type CopilotScenario } from './mock';
import { COPILOT_LIMITS } from './types';

const mock = (scenario: CopilotScenario = 'on') => createMockCopilot({ delayMs: 0, scenario });
const codeOf = async (p: Promise<unknown>) => toAppError(await p.catch((e) => e)).code;
const improve = (text: string, extra: object = {}) => ({ mode: 'improve' as const, field: 'Description', text, ...extra });

describe('features', () => {
	it('lists both features when a runtime is set up', async () => {
		const features = await mock().features();
		expect(features.map((f) => f.id)).toEqual(['text.improve', 'bug.report']);
	});

	it('is empty without a runtime, and a draft is copilot_unavailable', async () => {
		const off = mock('off');
		expect(await off.features()).toEqual([]);
		expect(await codeOf(off.draft('text.improve', improve('hi')))).toBe('copilot_unavailable');
	});

	it('says it is sample data', () => {
		expect(mock().sample).toBe(true);
	});
});

describe('text.improve', () => {
	it('tidies spacing, capital and ending, and says so', async () => {
		const r = await mock().draft('text.improve', improve('  the  save button\n\n\n\ndoes nothing '));
		expect(r.draft.text).toBe('The save button\n\ndoes nothing.');
		expect(r.notes[0]).toMatch(/Kept your meaning/);
	});

	it('leaves text that is already fine alone', async () => {
		const r = await mock().draft('text.improve', improve('All good.'));
		expect(r.draft.text).toBe('All good.');
		expect(r.notes).toEqual(['Nothing to fix: it already reads well.']);
	});

	it('drafts from an instruction, marking what it cannot know', async () => {
		const r = await mock().draft('text.improve', { mode: 'draft', field: 'Goal', text: '', instruction: 'explain the release plan' });
		expect(r.draft.text).toBe("Explain the release plan. [?] Add the details the copilot can't know.");
		expect(r.notes.join(' ')).toMatch(/Sample data/);
	});

	it('refuses to improve nothing, and to draft from nothing', async () => {
		expect(await codeOf(mock().draft('text.improve', improve('   ')))).toBe('invalid_request');
		expect(await codeOf(mock().draft('text.improve', { mode: 'draft', field: 'Goal', text: ' ' }))).toBe('invalid_request');
	});

	it('enforces the same caps as the office', async () => {
		const tooLong = 'x'.repeat(COPILOT_LIMITS.textMaxChars + 1);
		expect(await codeOf(mock().draft('text.improve', improve(tooLong)))).toBe('invalid_request');
		expect(await codeOf(mock().draft('text.improve', improve('ok', { instruction: 'y'.repeat(501) })))).toBe('invalid_request');
		expect(await codeOf(mock().draft('text.improve', { ...improve('ok'), field: '' }))).toBe('invalid_request');
		// Exactly at the cap is fine, and the result stays within it.
		const atCap = 'x'.repeat(COPILOT_LIMITS.textMaxChars);
		const r = await mock().draft('text.improve', improve(atCap));
		expect([...r.draft.text].length).toBeLessThanOrEqual(COPILOT_LIMITS.textMaxChars);
	});
});

describe('bug.report', () => {
	it('drafts a report from a sentence', async () => {
		const r = await mock().draft('bug.report', { sentence: "the save button doesn't work after I rename a file. It just sits there" });
		expect(r.draft.title).toBe("The save button doesn't work after I rename a file");
		expect(r.draft.severity).toBe('high');
		expect(r.draft.steps).toEqual([]);
		expect(r.draft.expected).toBe('Not stated.');
		expect(r.notes.join(' ')).toMatch(/no steps/);
	});

	it('shortens a long first sentence for the title and keeps every field within its cap', async () => {
		const r = await mock().draft('bug.report', { sentence: 'a'.repeat(COPILOT_LIMITS.sentenceMaxChars) });
		expect([...r.draft.title].length).toBeLessThanOrEqual(80);
		expect([...r.draft.actual].length).toBeLessThanOrEqual(800);
	});

	it('guesses severity from a few words', () => {
		expect(guessSeverity('The app crashed')).toBe('critical');
		expect(guessSeverity('I lost my notes')).toBe('critical');
		expect(guessSeverity('Search fails')).toBe('high');
		expect(guessSeverity('A typo in the header')).toBe('low');
		expect(guessSeverity('It feels odd')).toBe('medium');
	});

	it('refuses an empty or over-long sentence', async () => {
		expect(await codeOf(mock().draft('bug.report', { sentence: '  ' }))).toBe('invalid_request');
		expect(await codeOf(mock().draft('bug.report', { sentence: 'x'.repeat(1001) }))).toBe('invalid_request');
	});
});

describe('scenarios and limits', () => {
	it('a failed runtime is copilot_failed and a slow one is timeout', async () => {
		expect(await codeOf(mock('failed').draft('bug.report', { sentence: 'it broke' }))).toBe('copilot_failed');
		expect(await codeOf(mock('timeout').draft('bug.report', { sentence: 'it broke' }))).toBe('timeout');
	});

	it('allows one run of a feature at a time, and frees the slot when it ends', async () => {
		const api = createMockCopilot({ delayMs: 5, scenario: 'on' });
		const first = api.draft('bug.report', { sentence: 'one' });
		expect(await codeOf(api.draft('bug.report', { sentence: 'two' }))).toBe('copilot_busy');
		// Another feature is not blocked.
		const other = api.draft('text.improve', improve('hello'));
		await Promise.all([first, other]);
		await expect(api.draft('bug.report', { sentence: 'three' })).resolves.toBeDefined();
	});

	it('frees the slot after a failure too', async () => {
		const api = createMockCopilot({ delayMs: 0, scenario: 'failed' });
		expect(await codeOf(api.draft('bug.report', { sentence: 'a' }))).toBe('copilot_failed');
		expect(await codeOf(api.draft('bug.report', { sentence: 'a' }))).toBe('copilot_failed');
	});
});

describe('tidy', () => {
	it('does not add a full stop after punctuation, or past the cap', () => {
		expect(tidy('Is it done?')).toBe('Is it done?');
		expect(tidy('done')).toBe('Done.');
		expect(tidy('x'.repeat(COPILOT_LIMITS.textMaxChars)).endsWith('.')).toBe(false);
	});
});
