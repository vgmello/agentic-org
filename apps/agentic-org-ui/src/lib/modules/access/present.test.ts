import { describe, expect, it } from 'vitest';
import { groupPeople, listOf, visibleAdminUsername, visibleUsername } from './present';
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

describe('username visibility', () => {
	const avery = { memberId: 'avery', username: 'avery' };
	const alex = { memberId: 'alex', username: 'alex' };

	it('shows every username to the owner and to the administrator', () => {
		const owner = { memberId: 'owner', isOwner: true, isAdministrator: false };
		const admin = { memberId: 'admin', isOwner: false, isAdministrator: true };
		expect(visibleUsername(avery, owner)).toBe('avery');
		expect(visibleUsername(avery, admin)).toBe('avery');
		expect(visibleAdminUsername({ memberId: 'alex', username: 'alex', isYou: false }, owner)).toBe(
			'alex'
		);
		expect(visibleAdminUsername({ memberId: 'alex', username: 'alex', isYou: true }, admin)).toBe(
			'alex'
		);
	});

	it('shows a regular member only their own username, never another person or the administrator', () => {
		const member = { memberId: 'avery', isOwner: false, isAdministrator: false };
		expect(visibleUsername(avery, member)).toBe('avery');
		expect(visibleUsername(alex, member)).toBeUndefined();
		expect(
			visibleAdminUsername({ memberId: 'alex', username: 'alex', isYou: false }, member)
		).toBeUndefined();
	});

	it('omits username when absent even for managers', () => {
		const owner = { memberId: 'owner', isOwner: true, isAdministrator: false };
		expect(visibleUsername({ memberId: 'leo' }, owner)).toBeUndefined();
		expect(visibleAdminUsername({ isYou: false }, owner)).toBeUndefined();
	});
});
