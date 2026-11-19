import { describe, expect, it } from 'vitest';
import type { MemberRow, WorkRow } from '$lib/modules/members';
import { blockedSentence, isBlocked, teamBlockers, type TeamFacts } from './blockers';
import type { TeamProjectRow, TeamRow } from './types';

const team = (id: string, parentId?: string, status: TeamRow['status'] = 'active'): TeamRow => ({
	id,
	name: id[0].toUpperCase() + id.slice(1),
	ref: `TEAM-${id.length}`,
	parentId,
	people: 0,
	agents: 0,
	projects: [],
	maxMembers: 6,
	status
});

const member = (
	id: string,
	home: string | undefined,
	status: MemberRow['status'] = 'active',
	kind: MemberRow['kind'] = 'person'
): MemberRow => ({
	id,
	kind,
	name: id,
	handle: id,
	status,
	owner: false,
	canSignIn: false,
	directReports: 0,
	homeTeam: home ? { id: home, name: home, ref: '' } : undefined
});

const project = (id: string, role: TeamProjectRow['role']): TeamProjectRow => ({
	id,
	name: id,
	role,
	openWork: 0
});
const task = (ref: string, state: WorkRow['state']): WorkRow => ({ ref, title: ref, state });

const facts = (over: Partial<TeamFacts> = {}): TeamFacts => ({
	teams: [
		team('eng'),
		team('web', 'eng'),
		team('platform', 'eng'),
		team('old', 'eng', 'archived'),
		team('deep', 'web')
	],
	members: [
		member('sam', 'eng', 'active', 'agent'),
		member('alex', 'eng'),
		member('paused', 'eng', 'paused'),
		member('gone', 'eng', 'inactive'),
		member('wen', 'web'),
		member('nobody', undefined)
	],
	projects: [],
	work: [],
	...over
});

describe('team blockers', () => {
	it('names active child teams and home employees for Archive, former employees left out', () => {
		const b = teamBlockers('eng', facts(), 'archive');
		expect(b.teams.map((t) => t.id)).toEqual(['platform', 'web']);
		expect(b.employees.map((m) => m.id)).toEqual(['alex', 'paused', 'sam']);
		expect(b.employees.find((m) => m.id === 'sam')?.kind).toBe('agent');
		expect(isBlocked(b)).toBe(true);
	});

	it('counts archived child teams for Delete, which they still point at', () => {
		expect(teamBlockers('eng', facts(), 'delete').teams.map((t) => t.id)).toEqual([
			'old',
			'platform',
			'web'
		]);
	});

	it('names projects it owns and its open tasks, not projects it only contributes to or done tasks', () => {
		const b = teamBlockers(
			'deep',
			facts({
				projects: [project('beacon', 'owner'), project('atlas', 'contributor')],
				work: [task('T-10', 'in_progress'), task('T-9', 'todo'), task('T-2', 'done')]
			}),
			'archive'
		);
		expect(b.projects).toEqual([{ id: 'beacon', name: 'beacon' }]);
		expect(b.tasks.map((t) => t.ref)).toEqual(['T-9', 'T-10']);
		expect(isBlocked(b)).toBe(true);
	});

	it('lets an empty team through', () => {
		const b = teamBlockers('deep', facts(), 'delete');
		expect(b).toEqual({ teams: [], employees: [], projects: [], tasks: [] });
		expect(isBlocked(b)).toBe(false);
	});

	it('says what is left in the spec words, plural-correct', () => {
		expect(blockedSentence('Checkout', teamBlockers('eng', facts(), 'archive'))).toBe(
			'Checkout still has 2 child teams and 3 home employees. Resolve them first.'
		);
		expect(blockedSentence('Web', teamBlockers('web', facts(), 'archive'))).toBe(
			'Web still has 1 child team and 1 home employee. Resolve them first.'
		);
		expect(
			blockedSentence('Web', {
				teams: [],
				employees: [{ id: 'a', name: 'A', kind: 'agent' }],
				projects: [{ id: 'p', name: 'P' }],
				tasks: [
					{ ref: 'T-1', title: '' },
					{ ref: 'T-2', title: '' }
				]
			})
		).toBe(
			'Web still has 1 home employee, 1 project it owns and 2 open tasks. Resolve them first.'
		);
	});
});
