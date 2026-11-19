import { describe, expect, it } from 'vitest';
import type { HireRequest } from '../../types';
import { HireFlowState } from './flow.svelte';

const request = (over: Partial<HireRequest> = {}): HireRequest => ({
	id: 'h9',
	state: 'drafting',
	requestedBy: { id: 'you', name: 'Alex Kim' },
	byViewer: true,
	homeTeam: { id: 'web', name: 'Web', ref: 'TEAM-2' },
	createdAt: '2026-11-18T12:00:00Z',
	updatedAt: '2026-11-18T12:00:00Z',
	reason: 'A tester',
	doneSteps: [],
	drafting: { state: 'reading', startedAt: '2026-11-18T12:00:00Z' },
	...over
});
const answer = request({
	drafting: { state: 'ready', startedAt: '2026-11-18T12:00:00Z' },
	proposal: { name: 'Marlowe', handle: 'qa-engineer-marlowe', homeTeamId: 'web' }
});

describe('the hire flow', () => {
	it("applies the HR lead's answer while it waits for it", () => {
		const flow = new HireFlowState();
		flow.asked(request());
		expect(flow.hrAnswered(answer)).toBe(true);
		expect(flow.step).toBe('proposal');
		expect(flow.manual).toBe(false);
		expect(flow.fields.name).toBe('Marlowe');
	});

	it('ignores a late HR answer once the hirer fills in the details themselves', () => {
		const flow = new HireFlowState();
		flow.asked(request());
		flow.fillIn('web');
		flow.fields.name = 'Juniper';
		expect(flow.hrAnswered(answer)).toBe(false);
		expect(flow.manual).toBe(true);
		expect(flow.fields.name).toBe('Juniper');
		expect(flow.fields.handle).toBe('');
	});

	it('ignores an HR answer after going back to Describe, or for another request', () => {
		const flow = new HireFlowState();
		flow.asked(request());
		flow.back();
		expect(flow.hrAnswered(answer)).toBe(false);
		expect(flow.step).toBe('describe');
		flow.asked(request());
		expect(flow.hrAnswered({ ...answer, id: 'other' })).toBe(false);
	});

	it('keeps what was filled in by hand across Back, with the home team chosen on Describe', () => {
		const flow = new HireFlowState();
		flow.fillIn('web');
		flow.fields.name = 'Juniper';
		flow.fields.handle = 'juniper';
		flow.back();
		flow.fillIn('platform');
		expect(flow.fields).toMatchObject({
			name: 'Juniper',
			handle: 'juniper',
			homeTeamId: 'platform'
		});
	});

	it('fills in by hand with nothing invented', () => {
		const flow = new HireFlowState();
		flow.fillIn('web');
		expect(flow.fields).toEqual({
			name: '',
			handle: '',
			executionProfileId: '',
			model: '',
			officeId: '',
			homeTeamId: 'web'
		});
	});

	it('returns to an unchanged draft instead of asking again', () => {
		const flow = new HireFlowState();
		flow.reason = 'A tester';
		flow.asked(request());
		flow.back();
		expect(flow.unchanged('web')).toBe('drafting');
		expect(flow.unchanged('platform')).toBeUndefined();
		flow.reason = 'Someone else';
		expect(flow.unchanged('web')).toBeUndefined();
	});

	it('opens a picked-up request at its step', () => {
		const flow = new HireFlowState();
		flow.open(answer);
		expect(flow.step).toBe('proposal');
		flow.open(request({ state: 'withdrawn', drafting: undefined, closedReason: 'Withdrawn' }));
		expect(flow.step).toBe('closed');
		flow.open(request({ state: 'failed', drafting: undefined, doneSteps: ['record'] }));
		expect(flow.step).toBe('hiring');
	});

	it('returns to the hand-filled form after Back with an unchanged Describe, never to a wait that cannot end', () => {
		const flow = new HireFlowState();
		flow.reason = 'A tester';
		flow.asked(request());
		flow.fillIn('web');
		flow.fields.name = 'Juniper';
		flow.back();
		const next = flow.unchanged('web');
		expect(next).toBe('proposal');
		flow.step = next!;
		expect(flow.hrAnswered(answer)).toBe(false);
		expect(flow.step).toBe('proposal');
		expect(flow.manual).toBe(true);
		expect(flow.fields.name).toBe('Juniper');
	});

	it("opens someone else's draft read-only", () => {
		const flow = new HireFlowState();
		flow.open({ ...answer, byViewer: false });
		expect(flow.step).toBe('closed');
	});
});
