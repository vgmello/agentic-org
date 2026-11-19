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
		expect(features.map((f) => f.id)).toEqual(['text.improve', 'bug.from_sentence']);
		expect(features.find((f) => f.id === 'bug.from_sentence')?.version).toBe(2);
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
		expect(await codeOf(mock().draft('text.improve', improve(tooLong)))).toBe('too_large');
		expect(await codeOf(mock().draft('text.improve', improve('ok', { instruction: 'y'.repeat(501) })))).toBe('too_large');
		expect(await codeOf(mock().draft('text.improve', { ...improve('ok'), field: '' }))).toBe('invalid_request');
		// Exactly at the cap is fine, and the result stays within it.
		const atCap = 'x'.repeat(COPILOT_LIMITS.textMaxChars);
		const r = await mock().draft('text.improve', improve(atCap));
		expect([...r.draft.text].length).toBeLessThanOrEqual(COPILOT_LIMITS.textMaxChars);
	});
});

describe('bug.from_sentence', () => {
	it('drafts the fields of the real bug report from a sentence', async () => {
		const r = await mock().draft('bug.from_sentence', { sentence: "the save button doesn't work after I rename a file. It just sits there" });
		expect(r.draft.title).toBe("The save button doesn't work after I rename a file");
		expect(r.draft.severity).toBe('major');
		expect(r.draft.what_happened).toMatch(/sits there/);
		// What the sentence doesn't say is left out, not invented.
		expect(r.draft).not.toHaveProperty('steps');
		expect(r.draft).not.toHaveProperty('expected');
		expect(r.draft).not.toHaveProperty('environment');
		expect(r.notes.join(' ')).toMatch(/no steps/);
	});

	it('uses the office’s severities, never the old scale', async () => {
		const sevs = await Promise.all(['It crashed', 'Search fails', 'A typo in the header', 'It feels odd'].map(async (sentence) => (await mock().draft('bug.from_sentence', { sentence })).draft.severity));
		expect(sevs).toEqual(['critical', 'major', 'trivial', 'minor']);
	});

	it('names an area only when the sentence does, and only one from the context', async () => {
		const context = [
			{ kind: 'area' as const, label: 'Files', text: 'area-files' },
			{ kind: 'area' as const, label: 'Billing', text: 'area-billing' }
		];
		const named = await mock().draft('bug.from_sentence', { sentence: 'Files screen shows nothing' }, context);
		expect(named.draft.area_id).toBe('area-files');
		const none = await mock().draft('bug.from_sentence', { sentence: 'Something is odd' }, context);
		expect(none.draft).not.toHaveProperty('area_id');
		const both = await mock().draft('bug.from_sentence', { sentence: 'Files and Billing disagree' }, context);
		expect(both.draft).not.toHaveProperty('area_id');
		const nowhere = await mock().draft('bug.from_sentence', { sentence: 'Files screen shows nothing' });
		expect(nowhere.draft).not.toHaveProperty('area_id');
	});

	it('shortens a long first sentence for the title and keeps every field within its cap', async () => {
		const r = await mock().draft('bug.from_sentence', { sentence: 'a'.repeat(COPILOT_LIMITS.sentenceMaxChars) });
		expect([...r.draft.title].length).toBeLessThanOrEqual(80);
		expect([...r.draft.what_happened].length).toBeLessThanOrEqual(2000);
	});

	it('guesses severity from a few words', () => {
		expect(guessSeverity('The app crashed')).toBe('critical');
		expect(guessSeverity('I lost my notes')).toBe('critical');
		expect(guessSeverity('Search fails')).toBe('major');
		expect(guessSeverity('A typo in the header')).toBe('trivial');
		expect(guessSeverity('It feels odd')).toBe('minor');
	});

	it('refuses an empty sentence, and an over-long one is too_large', async () => {
		expect(await codeOf(mock().draft('bug.from_sentence', { sentence: '  ' }))).toBe('invalid_request');
		expect(await codeOf(mock().draft('bug.from_sentence', { sentence: 'x'.repeat(1001) }))).toBe('too_large');
	});
});

describe('scenarios and limits', () => {
	it('a failed runtime is copilot_failed and a slow one is timeout', async () => {
		expect(await codeOf(mock('failed').draft('bug.from_sentence', { sentence: 'it broke' }))).toBe('copilot_failed');
		expect(await codeOf(mock('timeout').draft('bug.from_sentence', { sentence: 'it broke' }))).toBe('timeout');
	});

	it('a feature it does not offer is copilot_unavailable', async () => {
		// @ts-expect-error not a feature of this build
		expect(await codeOf(mock().draft('bug.report', { sentence: 'x' }))).toBe('copilot_unavailable');
	});

	it('allows one run of a feature at a time, and frees the slot when it ends', async () => {
		const api = createMockCopilot({ delayMs: 5, scenario: 'on' });
		const first = api.draft('bug.from_sentence', { sentence: 'one' });
		expect(await codeOf(api.draft('bug.from_sentence', { sentence: 'two' }))).toBe('copilot_busy');
		// Another feature is not blocked.
		const other = api.draft('text.improve', improve('hello'));
		await Promise.all([first, other]);
		await expect(api.draft('bug.from_sentence', { sentence: 'three' })).resolves.toBeDefined();
	});

	it('frees the slot after a failure too', async () => {
		const api = createMockCopilot({ delayMs: 0, scenario: 'failed' });
		expect(await codeOf(api.draft('bug.from_sentence', { sentence: 'a' }))).toBe('copilot_failed');
		expect(await codeOf(api.draft('bug.from_sentence', { sentence: 'a' }))).toBe('copilot_failed');
	});
});

describe('tidy', () => {
	it('does not add a full stop after punctuation, or past the cap', () => {
		expect(tidy('Is it done?')).toBe('Is it done?');
		expect(tidy('done')).toBe('Done.');
		expect(tidy('x'.repeat(COPILOT_LIMITS.textMaxChars)).endsWith('.')).toBe(false);
	});
});
