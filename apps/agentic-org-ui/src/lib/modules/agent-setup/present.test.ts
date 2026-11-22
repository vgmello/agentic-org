import { describe, expect, it } from 'vitest';
import {
	CONNECTION_TONE,
	ageText,
	isStale,
	listDescription,
	modelNote,
	offboardLines,
	officeText,
	remainingHolds,
	reportedText,
	runtimeText,
	sortAgents,
	toolsText
} from './present';

const now = Date.parse('2026-11-19T12:00:00Z');

describe('words', () => {
	it('ages observations and marks the old ones stale', () => {
		expect(ageText('2026-11-19T11:59:40Z', now)).toBe('20 s ago');
		expect(ageText('2026-11-19T11:48:00Z', now)).toBe('12 min ago');
		expect(ageText('2026-11-19T09:00:00Z', now)).toBe('3 h ago');
		expect(ageText('2026-11-10T09:00:00Z', now)).toBe('on 10 Nov');
		expect(ageText(undefined, now)).toBe('never');
		expect(isStale('2026-11-19T11:59:00Z', now)).toBe(false);
		expect(isStale('2026-11-19T11:50:00Z', now)).toBe(true);
		expect(isStale(undefined, now)).toBe(true);
	});

	it('names the office or Unplaced, with when it reported', () => {
		const p = {
			office: { id: 'hq', name: 'HQ' },
			generation: 1,
			observedAt: '2026-11-19T11:58:00Z'
		};
		expect(officeText(p)).toBe('HQ');
		expect(reportedText(p, now)).toBe('reported 2 min ago');
		expect(reportedText({ ...p, observedAt: undefined }, now)).toBe('never reported');
		expect(officeText(null)).toBe('Unplaced');
		expect(reportedText(null, now)).toBeUndefined();
	});

	it('lists runtimes in priority order', () => {
		const e = {
			profile: { id: 'p', name: 'P' },
			modelId: 'm',
			enabled: true,
			runtimes: ['Claude Code', 'Codex']
		};
		expect(runtimeText(e)).toBe('Claude Code, then Codex');
		expect(runtimeText({ ...e, runtimes: [] })).toBe('No runtime');
		expect(runtimeText(null)).toBeUndefined();
	});

	it('orders the built-in HR lead first, then A–Z', () => {
		const rows = [
			{ name: 'Noor', builtIn: false },
			{ name: 'HR lead', builtIn: true },
			{ name: 'Kai', builtIn: false }
		];
		expect(sortAgents(rows).map((r) => r.name)).toEqual(['HR lead', 'Kai', 'Noor']);
	});

	it('says how many need setup only when some do', () => {
		expect(listDescription(5, 0)).not.toMatch(/setup\.$/);
		expect(listDescription(5, 1)).toMatch(/1 of 5 needs setup\.$/);
		expect(listDescription(5, 2)).toMatch(/2 of 5 need setup\.$/);
	});

	it('describes a model offer without promising capacity', () => {
		expect(modelNote({ id: 'm', supporting: 2, ready: 1, stale: false })).toBe(
			'1 of 2 connections ready'
		);
		expect(modelNote({ id: 'm', supporting: 1, ready: 0, stale: true })).toBe(
			'0 of 1 connection ready · last check out of date'
		);
		expect(modelNote({ id: 'm', supporting: 0, ready: 0, stale: false })).toBe(
			'not reported by any connection'
		);
		expect(toolsText([])).toBe('No tools');
		expect(toolsText(['a'])).toBe('1 approved tool');
	});

	it('replaces the summary retention line with the words of §9', () => {
		expect(
			offboardLines(
				['Juno leaves Design.', "Juno's record stays, and the name juno stays reserved."],
				'Juno'
			)
		).toEqual([
			'Juno leaves Design.',
			"Juno's profile and home-team history stay in organization data. The handle stays reserved."
		]);
	});

	it('keeps amber for what the owner must do on the office; a cooldown is neutral', () => {
		expect(CONNECTION_TONE.signed_out).toBe('owner');
		expect(CONNECTION_TONE.binding_missing).toBe('owner');
		expect(CONNECTION_TONE.cooling_down).toBe('neutral');
		expect(CONNECTION_TONE.unknown).toBe('neutral');
		expect(CONNECTION_TONE.ready).toBe('ok');
	});

	it('keeps the holds Resume does not clear', () => {
		expect(remainingHolds(['Manual pause by Alex', 'Daily budget reached'])).toEqual([
			'Daily budget reached'
		]);
	});
});

it('sets commands in a fix apart, and leaves an unpaired backtick as written', async () => {
	const { codeParts } = await import('./present');
	expect(codeParts('Sign in with `codex login`, then wait.')).toEqual([
		{ text: 'Sign in with ', code: false },
		{ text: 'codex login', code: true },
		{ text: ', then wait.', code: false }
	]);
	expect(codeParts('Odd ` tick')).toEqual([{ text: 'Odd ` tick', code: false }]);
	expect(codeParts('No code')).toEqual([{ text: 'No code', code: false }]);
});
