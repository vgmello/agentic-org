// Pure helpers for Members and access: the three lists and the words that say each person's state.
import type { AccessPerson, AccessState } from './types';

export interface AccessGroups {
	signedIn: AccessPerson[];
	invited: AccessPerson[];
	noAccount: AccessPerson[];
}

const byName = (a: AccessPerson, b: AccessPerson) =>
	a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) ||
	a.memberId.localeCompare(b.memberId);

/** Splits people into the three lists the page shows, each sorted A–Z by name. */
export function groupPeople(people: AccessPerson[]): AccessGroups {
	const groups: AccessGroups = { signedIn: [], invited: [], noAccount: [] };
	for (const person of people) {
		if (person.state === 'signed_in') groups.signedIn.push(person);
		else if (person.state === 'invited') groups.invited.push(person);
		else groups.noAccount.push(person);
	}
	groups.signedIn.sort(byName);
	groups.invited.sort(byName);
	groups.noAccount.sort(byName);
	return groups;
}

/** The state in words: every state carries its words, never colour alone (app-shell-ux §6a). */
export const stateWord: Record<AccessState, string> = {
	signed_in: 'Can sign in',
	invited: 'Invited, not signed in yet',
	no_account: 'No account yet'
};

/** Shown to viewers who may read the lists but not change them (app-shell-ux §0.1: no actions they can't use). */
export const MANAGERS_ONLY = 'Only the owner or the administrator changes sign-in access.';

/** Refusal shown when an action names a person who has left the organization since the list loaded. */
export const NOT_IN_ORGANIZATION = "That person isn't in this organization any more.";
