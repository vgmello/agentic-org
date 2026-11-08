import { describe, expect, it } from 'vitest';
import type { AppError } from '$lib/proto';
import { copilotFeatures, describeCopilotFailure, forgetCopilotFeatures, offers, type CopilotApi } from './api';
import { createDraftFlow, isDrafting, type DraftPhase } from './draft-flow';
import { createMockCopilot, type CopilotScenario } from './mock';
import type { BugDraft, CopilotFeature, TextDraft } from './types';

const mock = (scenario: CopilotScenario = 'on', delayMs = 0) => createMockCopilot({ delayMs, scenario });
const kinds = (phases: DraftPhase<unknown>[]) => phases.map((p) => p.kind);

describe('createDraftFlow', () => {
	it('walks drafting → ready with the draft and notes', async () => {
		const phases: DraftPhase<TextDraft>[] = [];
		const flow = createDraftFlow(mock(), 'text.improve', (p) => phases.push(p));
		await flow.run({ mode: 'improve', field: 'Title', text: 'fix the thing' });
		expect(kinds(phases)).toEqual(['drafting', 'ready']);
		const ready = phases.at(-1);
		expect(ready?.kind === 'ready' && ready.response.draft.text).toBe('Fix the thing.');
	});

	it('a failure ends in failed, with the error unchanged and nothing thrown', async () => {
		const phases: DraftPhase<BugDraft>[] = [];
		const flow = createDraftFlow(mock('timeout'), 'bug.report', (p) => phases.push(p));
		await flow.run({ sentence: 'it broke' });
		expect(kinds(phases)).toEqual(['drafting', 'failed']);
		const last = phases.at(-1);
		expect(last?.kind === 'failed' && last.error.code).toBe('timeout');
	});

	it('turns anything thrown into an app error', async () => {
		const api: CopilotApi = { ...mock(), draft: () => Promise.reject(new Error('boom')) };
		const phases: DraftPhase<BugDraft>[] = [];
		await createDraftFlow(api, 'bug.report', (p) => phases.push(p)).run({ sentence: 'x' });
		const last = phases.at(-1);
		expect(last?.kind === 'failed' && last.error).toEqual({ source: 'app', code: 'internal', message: 'boom' });
	});

	it('ignores a second run in the same tick', async () => {
		let calls = 0;
		const real = mock();
		const api: CopilotApi = { ...real, draft: (f, i) => (calls++, real.draft(f, i)) };
		const flow = createDraftFlow(api, 'bug.report', () => {});
		await Promise.all([flow.run({ sentence: 'a' }), flow.run({ sentence: 'a' })]);
		expect(calls).toBe(1);
	});

	it('can run again after it finished', async () => {
		const phases: DraftPhase<BugDraft>[] = [];
		const flow = createDraftFlow(mock(), 'bug.report', (p) => phases.push(p));
		await flow.run({ sentence: 'a' });
		await flow.run({ sentence: 'b' });
		expect(kinds(phases)).toEqual(['drafting', 'ready', 'drafting', 'ready']);
	});

	it('drops the answer of an ask that was cancelled', async () => {
		const phases: DraftPhase<BugDraft>[] = [];
		const flow = createDraftFlow(mock('on', 10), 'bug.report', (p) => phases.push(p));
		const pending = flow.run({ sentence: 'a' });
		flow.cancel();
		await pending;
		expect(kinds(phases)).toEqual(['drafting', 'idle']);
	});

	it('isDrafting is true only while drafting', () => {
		expect(isDrafting({ kind: 'drafting' })).toBe(true);
		expect(isDrafting({ kind: 'idle' })).toBe(false);
	});
});

describe('which smart buttons show', () => {
	it('offers a feature only when the list has it', () => {
		const some: CopilotFeature[] = [{ id: 'bug.report', version: 1, title: 'x' }];
		expect(offers(some, 'bug.report')).toBe(true);
		expect(offers(some, 'text.improve')).toBe(false);
		expect(offers([], 'bug.report')).toBe(false);
		expect(offers(undefined, 'bug.report')).toBe(false);
	});

	it('asks once per api, and a failure means none and is asked again next time', async () => {
		let asked = 0;
		const real = mock();
		const api: CopilotApi = { ...real, features: () => (asked++, asked === 1 ? Promise.reject(new Error('down')) : real.features()) };
		expect(await copilotFeatures(api)).toEqual([]);
		expect((await copilotFeatures(api)).length).toBe(2);
		expect((await copilotFeatures(api)).length).toBe(2);
		expect(asked).toBe(2);
		forgetCopilotFeatures(api);
		await copilotFeatures(api);
		expect(asked).toBe(3);
	});

	it('with the copilot off, nothing is offered', async () => {
		const features = await copilotFeatures(mock('off'));
		expect(offers(features, 'text.improve')).toBe(false);
		expect(offers(features, 'bug.report')).toBe(false);
	});
});

describe('describeCopilotFailure', () => {
	const err = (code: string, message = 'm'): AppError => ({ source: 'app', code, message });
	it.each([
		['copilot_unavailable', "The copilot isn't available"],
		['copilot_failed', "The copilot couldn't draft this"],
		['timeout', 'The copilot took too long'],
		['copilot_busy', 'Still working on your last request'],
		['invalid_request', "The copilot can't draft this"],
		['something_else', 'Something went wrong']
	])('%s', (code, title) => {
		expect(describeCopilotFailure(err(code)).title).toBe(title);
	});

	it('shows the office’s own words for a request it refused', () => {
		expect(describeCopilotFailure(err('invalid_request', 'Keep it shorter.')).hint).toBe('Keep it shorter.');
	});
});
