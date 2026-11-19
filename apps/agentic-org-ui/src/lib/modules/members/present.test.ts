import { describe, expect, it } from 'vitest';
import {
	busyText,
	durationFact,
	durationText,
	headerPresence,
	matchesOutcome,
	outcomeText,
	reportedText,
	rowPresence,
	startedByText,
	turnDuration,
	turnLine
} from './present';
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
		expect(startedByText({ kind: 'message', by: { id: 'm', name: 'Mira' } })).toBe(
			'Message from Mira'
		);
		expect(startedByText({ kind: 'recovery' })).toBe('Recovery');
	});
	it('says a failure with its reason', () => {
		expect(outcomeText(turn({ state: 'failed', failure: 'Codex isn’t signed in' }))).toBe(
			'Failed: Codex isn’t signed in'
		);
		expect(outcomeText(turn({ summary: 'Fixed it' }))).toBe('Fixed it');
	});
	it('rounds durations down and marks open turns', () => {
		expect(durationText(59_999)).toBe('59 s');
		expect(durationText(65 * 60_000)).toBe('1 h 5 min');
		expect(
			turnDuration({ startedAt: '2026-11-18T09:00:00Z' }, Date.parse('2026-11-18T09:02:30Z'))
		).toBe('running 2 min…');
	});
	it('filters by outcome', () => {
		expect(matchesOutcome(turn({ state: 'running' }), 'open')).toBe(true);
		expect(matchesOutcome(turn({ state: 'blocked' }), 'other')).toBe(true);
		expect(matchesOutcome(turn({ state: 'failed' }), 'succeeded')).toBe(false);
	});
});

describe('a requested turn is never called running', () => {
	const now = Date.parse('2026-11-18T09:02:00Z');
	const at = '2026-11-18T09:00:00Z';
	it('words the Busy reason by the turn state', () => {
		expect(busyText('Kai', { startedAt: at, state: 'accepted' }, now)).toBe(
			'Kai already has a run waiting for the office to start it (requested 2 min ago). It takes new input when that turn ends; nothing is queued.'
		);
		expect(busyText('Kai', { startedAt: at, state: 'running' }, now)).toContain(
			'Kai is already running a turn (started 2 min ago)'
		);
	});
	it('words the Turn line and the row time by the turn state', () => {
		expect(turnLine('accepted', { startedAt: at, state: 'accepted' }, now)).toBe(
			'Requested · waiting for the office to start it (requested 2 min ago)'
		);
		expect(turnLine('running', { startedAt: at, state: 'running' }, now)).toBe(
			'Running · started 2 min ago'
		);
		expect(turnLine('idle', undefined, now)).toBe('No open turn');
		expect(durationFact({ startedAt: at, state: 'accepted' }, now)).toBe(
			'waiting for the office to start it'
		);
		expect(durationFact({ startedAt: at, state: 'running' }, now)).toBe('running 2 min…');
	});
	it('never says "reported never reported"', () => {
		expect(reportedText(undefined)).toBe('never reported');
		expect(reportedText('2026-11-18T09:01:30Z', now)).toBe('reported 30 s ago');
	});
});
