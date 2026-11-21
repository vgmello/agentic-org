// Pure helpers for Members and access: the lists, username visibility and the words that say each person's state.
import type { AccessAdministrator, AccessOverview, AccessPerson, AccessState } from './types';

export interface AccessGroups {
	signedIn: AccessPerson[];
	invited: AccessPerson[];
	noAccount: AccessPerson[];
	/** Deactivated people: deactivation removed their access, and they can't be invited (offices-and-teams-ux §7.4). */
	deactivated: AccessPerson[];
}

/** Which list a person is in. */
export type AccessList = keyof AccessGroups;

const byName = (a: AccessPerson, b: AccessPerson) =>
	a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) ||
	a.memberId.localeCompare(b.memberId);

/** The list a person belongs in: deactivated people apart, whatever a stale state says. */
export function listOf(person: AccessPerson): AccessList {
	if (!person.active) return 'deactivated';
	if (person.state === 'signed_in') return 'signedIn';
	if (person.state === 'invited') return 'invited';
	return 'noAccount';
}

/** Splits people into the lists the page shows, each sorted A–Z by name. */
export function groupPeople(people: AccessPerson[]): AccessGroups {
	const groups: AccessGroups = { signedIn: [], invited: [], noAccount: [], deactivated: [] };
	for (const person of people) groups[listOf(person)].push(person);
	for (const list of Object.values(groups)) list.sort(byName);
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

type UsernameViewer = Pick<AccessOverview['viewer'], 'memberId' | 'isOwner' | 'isAdministrator'>;

/** Usernames are shown only to the owner, the administrator and the person themself. */
export function visibleUsername(
	person: Pick<AccessPerson, 'memberId' | 'username'>,
	viewer: UsernameViewer
): string | undefined {
	if (!person.username) return undefined;
	return viewer.isOwner ||
		viewer.isAdministrator ||
		(!!viewer.memberId && viewer.memberId === person.memberId)
		? person.username
		: undefined;
}

/** The administrator's username is shown only to the owner, the administrator and the person themself. */
export function visibleAdminUsername(
	admin: Pick<AccessAdministrator, 'memberId' | 'username' | 'isYou'>,
	viewer: UsernameViewer
): string | undefined {
	if (!admin.username) return undefined;
	return viewer.isOwner ||
		viewer.isAdministrator ||
		admin.isYou ||
		(!!admin.memberId && !!viewer.memberId && viewer.memberId === admin.memberId)
		? admin.username
		: undefined;
}
