import { describe, expect, it } from 'vitest';
import {
	ageWords,
	backoff,
	teamsInTreeOrder,
	cleanProposal,
	doneText,
	draftingPhase,
	handleProblem,
	hireUrl,
	hireUrlWith,
	hrUnavailableText,
	isFull,
	missingFields,
	missingReason,
	requestStatus,
	runtimeWarning,
	stepStates,
	teamFullText
} from './hire';

describe('missing fields', () => {
	it('names what Next needs, as the spec words it', () => {
		expect(missingReason(['home team', 'handle'])).toBe(
			'Choose a home team and handle to continue'
		);
		expect(missingReason(['execution profile', 'model', 'office'])).toBe(
			'Choose an execution profile, model and office to continue'
		);
		expect(missingReason(['name'])).toBe('Choose a name to continue');
		expect(missingReason([])).toBeUndefined();
	});
	it('treats optional role, focus and reporting as never missing', () => {
		expect(
			missingFields({
				name: 'Marlowe',
				handle: 'marlowe',
				homeTeamId: 'web',
				executionProfileId: 'p',
				model: 'm',
				officeId: 'o'
			})
		).toEqual([]);
		expect(missingFields({ name: '  ', homeTeamId: 'web' })).toEqual([
			'name',
			'handle',
			'execution profile',
			'model',
			'office'
		]);
	});
	it('sends trimmed values and leaves unset optional fields out', () => {
		expect(
			cleanProposal({
				name: ' Marlowe ',
				handle: 'marlowe',
				homeTeamId: 'web',
				executionProfileId: 'p',
				model: 'm',
				officeId: 'o',
				focus: '  ',
				title: ' QA engineer '
			})
		).toEqual({
			name: 'Marlowe',
			handle: 'marlowe',
			homeTeamId: 'web',
			executionProfileId: 'p',
			model: 'm',
			officeId: 'o',
			title: 'QA engineer'
		});
	});
});

describe('handles', () => {
	it('refuses malformed, reserved and taken handles', () => {
		expect(handleProblem('', [])).toBeUndefined();
		expect(handleProblem('qa-engineer-marlowe', ['noor'])).toBeUndefined();
		expect(handleProblem('-x', [])).toMatch(/starting with a letter or digit/);
		expect(handleProblem('HR', [])).toBe('"hr" is reserved. Choose another.');
		expect(handleProblem('Noor', ['noor'])).toBe('Noor is taken. Choose another.');
	});
});

describe('request states', () => {
	const base = { doneSteps: [] as never[] };
	it('reads each state as the list shows it', () => {
		expect(requestStatus({ state: 'drafting' })).toMatchObject({
			label: 'Drafting',
			action: 'open'
		});
		expect(requestStatus({ state: 'hiring' })).toMatchObject({ label: 'Hiring…', tone: 'working' });
		expect(requestStatus({ state: 'hiring', failedStep: 'messaging' }, 'Rowan')).toEqual({
			label: 'Not finished',
			tone: 'bad',
			detail: "Not finished: couldn't register with messaging",
			action: 'try_again'
		});
		expect(requestStatus({ state: 'hired' }).label).toBe('Hired');
		expect(requestStatus({ state: 'withdrawn' }).label).toBe('Withdrawn');
		// Given up: keeps the step, with no Try again.
		expect(requestStatus({ state: 'failed', failedStep: 'home', closedReason: 'Gave up' })).toEqual(
			{
				label: 'Not finished',
				tone: 'bad',
				detail: "Not finished: couldn't set up the home folder. Given up: Gave up"
			}
		);
		expect(requestStatus({ state: 'failed' }).detail).toBe('Given up: no reason was recorded');
	});
	it('marks the running step, the failed one and the ones done', () => {
		expect(stepStates({ ...base, state: 'hiring' }).map((s) => s.state)).toEqual([
			'doing',
			'waiting',
			'waiting'
		]);
		expect(
			stepStates({ state: 'hiring', doneSteps: ['record', 'home'], failedStep: 'messaging' }).map(
				(s) => s.state
			)
		).toEqual(['done', 'done', 'failed']);
		expect(stepStates({ ...base, state: 'hired' }).every((s) => s.state === 'done')).toBe(true);
	});
	it('names the steps already done', () => {
		expect(doneText({ doneSteps: ['home', 'record'] }, 'Marlowe')).toBe(
			"created Marlowe's record, set up the home folder"
		);
	});
});

describe('the HR lead', () => {
	const startedAt = new Date(0).toISOString();
	it('is still starting after 60 s without an answer', () => {
		expect(draftingPhase({ state: 'reading', startedAt }, 59_999)).toBe('reading');
		expect(draftingPhase({ state: 'reading', startedAt }, 60_000)).toBe('still_starting');
		expect(draftingPhase({ state: 'stopped', startedAt }, 0)).toBe('stopped');
	});
	it("says why it can't draft", () => {
		expect(hrUnavailableText({ state: 'can_run' })).toBeUndefined();
		expect(hrUnavailableText({ state: 'not_running' })).toBe(
			"The HR lead can't run on this server yet."
		);
		expect(
			hrUnavailableText({ state: 'cant_start', reason: "Codex isn't signed in on build-01." })
		).toBe("The HR lead can't start: Codex isn't signed in on build-01.");
	});
});

describe('teams and URLs', () => {
	it('refuses a full team with its limit named', () => {
		expect(isFull({ members: 6, maxMembers: 6 })).toBe(true);
		expect(isFull({ members: 5, maxMembers: 6 })).toBe(false);
		expect(teamFullText({ name: 'App', maxMembers: 6 })).toBe(
			"App already has 6 members, its limit. Raise the limit on the team's page first."
		);
	});
	it('opens the flow with the launching team or a request', () => {
		expect(hireUrl()).toBe('/teams/hire');
		expect(hireUrl({ teamId: 'web' })).toBe('/teams/hire?team=web');
		expect(hireUrl({ requestId: 'h 4' })).toBe('/teams/hire?request=h+4');
		expect(hireUrlWith('?team=web&mock=slow', 'h9')).toBe(
			'/teams/hire?team=web&mock=slow&request=h9'
		);
		expect(hireUrlWith('?team=web&request=h9', undefined)).toBe('/teams/hire?team=web');
		expect(hireUrlWith('?request=h9', undefined)).toBe('/teams/hire');
	});
});

describe('runtime warnings', () => {
	it('warns in Review for a runtime that is signed out, missing or unchecked', () => {
		expect(runtimeWarning('Codex', 'ready', 'build-01', 'Marlowe')).toBeUndefined();
		expect(runtimeWarning('Codex', 'signed_out', 'build-01', 'Marlowe')).toBe(
			"Codex isn't signed in on build-01. Marlowe is hired, but can't start until its administrator completes runtime setup there."
		);
		expect(runtimeWarning('Codex', 'unknown', 'build-01', 'Marlowe')).toBe(
			"Can't check whether Codex is installed and signed in on build-01. Marlowe will be hired; check the runtime setup there before Marlowe starts."
		);
	});
});

describe('ages', () => {
	const at = (min: number) =>
		new Date(Date.parse('2026-11-18T12:00:00Z') - min * 60_000).toISOString();
	const now = Date.parse('2026-11-18T12:00:00Z');
	it('says how old a request is', () => {
		expect(ageWords(at(0), now)).toBe('just now');
		expect(ageWords(at(15), now)).toBe('15 min ago');
		expect(ageWords(at(90), now)).toBe('1 h 30 min ago');
		expect(ageWords(at(120), now)).toBe('2 h ago');
		expect(ageWords('nonsense', now)).toBe('unknown');
	});
});

describe('team order and backoff', () => {
	it('lists teams as the Teams index does: the tree, each level by name', () => {
		const order = teamsInTreeOrder([
			{ id: 'web', name: 'Web', parentId: 'eng' },
			{ id: 'people', name: 'People' },
			{ id: 'eng', name: 'Engineering' },
			{ id: 'app', name: 'App', parentId: 'eng' },
			{ id: 'design', name: 'Design' }
		]);
		expect(order.map((o) => `${o.depth}:${o.team.id}`)).toEqual([
			'0:design',
			'0:eng',
			'1:app',
			'1:web',
			'0:people'
		]);
	});
	it('waits longer after each failure, at most 30 s', () => {
		expect([0, 1, 2, 3].map((n) => backoff(1000, n))).toEqual([1000, 2000, 4000, 8000]);
		expect(backoff(2000, 20)).toBe(30_000);
	});
});
