import { describe, expect, it } from 'vitest';
import { groupPeople, listOf } from './present';
import type { AccessPerson } from './types';

const person = (
	memberId: string,
	name: string,
	state: AccessPerson['state'],
	active = true
): AccessPerson => ({
	memberId,
	name,
	handle: memberId,
	homeTeam: 'Product',
	owner: false,
	administrator: false,
	active,
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
		expect(groups.deactivated).toEqual([]);
	});

	it('keeps every person exactly once', () => {
		const people = [
			person('a', 'A', 'signed_in'),
			person('b', 'B', 'no_account'),
			person('c', 'C', 'no_account', false)
		];
		const groups = groupPeople(people);
		expect(Object.values(groups).flat()).toHaveLength(people.length);
	});

	it('orders people with the same name by id, so the order never jumps between reloads', () => {
		const groups = groupPeople([person('z', 'Sam', 'signed_in'), person('a', 'Sam', 'signed_in')]);
		expect(groups.signedIn.map((p) => p.memberId)).toEqual(['a', 'z']);
	});
});

describe('listOf', () => {
	it('puts a deactivated person apart, never among people who can be invited', () => {
		expect(listOf(person('h', 'Hana', 'no_account', false))).toBe('deactivated');
		// A stale state from before the deactivation still doesn't offer them as signed in or invited.
		expect(listOf(person('h', 'Hana', 'signed_in', false))).toBe('deactivated');
		expect(listOf(person('h', 'Hana', 'invited', false))).toBe('deactivated');
	});
});
