import { describe, expect, it } from 'vitest';
import { rowPresence } from '../../present';
import type { MemberRow } from '../../types';
import { directoryView, firstName, formerText, placementText, shownText } from './directory';

const row = (over: Partial<MemberRow>): MemberRow => ({
	id: over.name ?? 'x',
	kind: 'person',
	name: 'Someone',
	handle: 'someone',
	status: 'active',
	owner: false,
	canSignIn: false,
	directReports: 0,
	...over
});

const NOW = Date.parse('2026-11-18T12:00:00Z');

describe('directoryView', () => {
	const rows = [
		row({ name: 'nadia', handle: 'nadia', kind: 'agent' }),
		row({ name: 'Alex Kim', handle: 'alex-kim', owner: true }),
		row({ name: 'Sam', handle: 'sam-b', kind: 'agent', status: 'paused' }),
		row({ name: 'Sam', handle: 'sam-a' }),
		row({ name: 'Old agent', handle: 'old', kind: 'agent', status: 'inactive' }),
		row({ name: 'Old person', handle: 'oldp', status: 'inactive' })
	];

	it('lists current members by name, case-insensitively, then by handle', () => {
		const v = directoryView(rows, 'all');
		expect(v.current.map((r) => r.handle)).toEqual(['alex-kim', 'nadia', 'sam-a', 'sam-b']);
	});

	it('keeps former members apart and out of the counts', () => {
		const v = directoryView(rows, 'all');
		expect(v.former.map((r) => r.handle)).toEqual(['old', 'oldp']);
		expect(v.counts).toEqual({ all: 4, people: 2, agents: 2 });
	});

	it('filters people and agents, former members included', () => {
		const people = directoryView(rows, 'people');
		expect(people.current.every((r) => r.kind === 'person')).toBe(true);
		expect(people.former.map((r) => r.handle)).toEqual(['oldp']);
		const agents = directoryView(rows, 'agents');
		expect(agents.current.map((r) => r.handle)).toEqual(['nadia', 'sam-b']);
		expect(agents.former.map((r) => r.handle)).toEqual(['old']);
	});

	it('counts the same under every filter', () => {
		expect(directoryView(rows, 'agents').counts).toEqual(directoryView(rows, 'all').counts);
	});
});

describe('placementText', () => {
	it('shows an agent’s office and how fresh the report is', () => {
		const observedAt = new Date(NOW - 20_000).toISOString();
		expect(
			placementText({ kind: 'agent', office: { id: 'o', name: 'Studio Mac', observedAt } }, NOW)
		).toBe('Runs at Studio Mac · reported 20 s ago');
	});

	it('says Unplaced for an agent with no office, and nothing when placement is unknown', () => {
		expect(placementText({ kind: 'agent', office: null }, NOW)).toBe('Unplaced');
		expect(placementText({ kind: 'agent' }, NOW)).toBeUndefined();
	});

	it('says never reported for an office without an observation', () => {
		expect(placementText({ kind: 'agent', office: { id: 'o', name: 'Lab' } }, NOW)).toBe(
			'Runs at Lab · never reported'
		);
	});

	it('shows a person’s contact, never an office', () => {
		expect(placementText({ kind: 'person', contact: 'sam@example.com' }, NOW)).toBe(
			'sam@example.com'
		);
		expect(
			placementText({ kind: 'person', office: { id: 'o', name: 'Lab' } }, NOW)
		).toBeUndefined();
	});
});

describe('row presence in the directory (AC-IA08)', () => {
	it('omits unknown presence and keeps every known word', () => {
		expect(rowPresence({ presence: { id: 'unknown' } })).toBeUndefined();
		expect(rowPresence({})).toBeUndefined();
		expect(rowPresence({ presence: { id: 'working' } })).toEqual({ id: 'working' });
	});
});

describe('shownText', () => {
	it('names the filter’s noun, plural-correct', () => {
		expect(shownText(1, 'people')).toBe('1 person shown');
		expect(shownText(3, 'people')).toBe('3 people shown');
		expect(shownText(0, 'agents')).toBe('0 agents shown');
		expect(shownText(2, 'all')).toBe('2 members shown');
	});
});

describe('formerText', () => {
	it('says when a former member left, if recorded', () => {
		expect(formerText({})).toBe('Former member');
		expect(formerText({ inactiveSince: 'not a date' })).toBe('Former member');
		expect(formerText({ inactiveSince: '2026-11-03T10:00:00Z' })).toMatch(/^Left /);
	});
});

describe('firstName', () => {
	it('is the first word of the name, for the second sentence of the Added toast', () => {
		expect(firstName('Sam Ortiz')).toBe('Sam');
		expect(firstName('  Nadia ')).toBe('Nadia');
		expect(firstName('Inês Maria Duarte')).toBe('Inês');
	});
});
