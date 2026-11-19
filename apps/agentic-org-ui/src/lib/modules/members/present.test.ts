import { describe, expect, it } from 'vitest';
import { durationText, headerPresence, matchesOutcome, outcomeText, rowPresence, startedByText, turnDuration } from './present';
import type { Turn } from './types';

const turn = (over: Partial<Turn>): Turn => ({
	id: 't',
	startedAt: '2026-11-18T09:00:00Z',
	startedBy: { kind: 'run' },
	state: 'succeeded',
	attempts: [],
	hasLog: true,
	...over
});

describe('presence', () => {
	it('omits unknown in rows and shows it once in an agent header only', () => {
		expect(rowPresence({ presence: { id: 'unknown' } })).toBeUndefined();
		expect(headerPresence({ kind: 'agent' })).toEqual({ id: 'unknown' });
		expect(headerPresence({ kind: 'person' })).toBeUndefined();
	});
});

describe('turns', () => {
	it('names who started a turn', () => {
		expect(startedByText({ kind: 'run', byViewer: true })).toBe('Run now by you');
		expect(startedByText({ kind: 'message', by: { id: 'm', name: 'Mira' } })).toBe('Message from Mira');
		expect(startedByText({ kind: 'recovery' })).toBe('Recovery');
	});
	it('says a failure with its reason', () => {
		expect(outcomeText(turn({ state: 'failed', failure: 'Codex isn’t signed in' }))).toBe('Failed: Codex isn’t signed in');
		expect(outcomeText(turn({ summary: 'Fixed it' }))).toBe('Fixed it');
	});
	it('rounds durations down and marks open turns', () => {
		expect(durationText(59_999)).toBe('59 s');
		expect(durationText(65 * 60_000)).toBe('1 h 5 min');
		expect(turnDuration({ startedAt: '2026-11-18T09:00:00Z' }, Date.parse('2026-11-18T09:02:30Z'))).toBe('running 2 min…');
	});
	it('filters by outcome', () => {
		expect(matchesOutcome(turn({ state: 'running' }), 'open')).toBe(true);
		expect(matchesOutcome(turn({ state: 'blocked' }), 'other')).toBe(true);
		expect(matchesOutcome(turn({ state: 'failed' }), 'succeeded')).toBe(false);
	});
});
