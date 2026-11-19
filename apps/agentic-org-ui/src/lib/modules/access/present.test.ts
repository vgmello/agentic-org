import { describe, expect, it } from 'vitest';
import { groupPeople, stateWord } from './present';
import type { AccessPerson } from './types';

const person = (memberId: string, name: string, state: AccessPerson['state']): AccessPerson => ({
	memberId,
	name,
	handle: memberId,
	homeTeam: 'Product',
	owner: false,
	administrator: false,
	state
});

describe('groupPeople', () => {
	it('splits people by sign-in state and sorts each list by name', () => {
		const groups = groupPeople([
			person('c', 'Zoe', 'no_account'),
			person('a', 'Avery', 'signed_in'),
			person('b', 'Hana', 'invited'),
			person('d', 'bea', 'signed_in'),
			person('e', 'Leo', 'no_account')
		]);
		expect(groups.signedIn.map((p) => p.name)).toEqual(['Avery', 'bea']);
		expect(groups.invited.map((p) => p.name)).toEqual(['Hana']);
		expect(groups.noAccount.map((p) => p.name)).toEqual(['Leo', 'Zoe']);
	});

	it('keeps every person exactly once', () => {
		const people = [person('a', 'A', 'signed_in'), person('b', 'B', 'no_account')];
		const groups = groupPeople(people);
		expect(groups.signedIn.length + groups.invited.length + groups.noAccount.length).toBe(
			people.length
		);
	});

	it('gives an empty list for a state nobody is in', () => {
		expect(groupPeople([]).invited).toEqual([]);
	});
});

describe('stateWord', () => {
	it('names each state in words', () => {
		expect(stateWord.signed_in).toBe('Can sign in');
		expect(stateWord.invited).toBe('Invited, not signed in yet');
		expect(stateWord.no_account).toBe('No account yet');
	});
});
