// Adding a person from a team's page (offices-and-teams-ux §7.4, PEOPLE-3/4): the name and the handle are separate.
// The handle is suggested from the name until the person edits it, then left alone; its form is checked here, and
// its uniqueness across people and agents by HQ (the Handle field shows that refusal).

/** Addresses the messaging contract keeps for routing, never a person's handle. */
export const RESERVED_HANDLES = new Set([
	'owner',
	'all',
	'everyone',
	'here',
	'channel',
	'hr',
	'me'
]);

/** "Sam Ortiz" → "sam-ortiz"; letters and digits kept, the rest becomes single hyphens. */
export function suggestHandle(name: string): string {
	return name
		.normalize('NFKD')
		.replace(/[̀-ͯ]/g, '')
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '')
		.slice(0, 32);
}

/** Why a handle can't be used as typed, or undefined when its form is fine. */
export function handleProblem(handle: string): string | undefined {
	const h = handle.trim();
	if (h === '') return 'Choose a handle.';
	if (h.startsWith('@')) return 'Leave out the @: it is added when the handle is shown.';
	if (h !== h.toLowerCase()) return 'Use lowercase letters.';
	if (!/^[a-z0-9][a-z0-9._-]*$/.test(h))
		return 'Use letters, digits, dots, underscores and hyphens, starting with a letter or digit.';
	if (h.length > 32) return 'Keep it to 32 characters.';
	if (RESERVED_HANDLES.has(h)) return `@${h} is reserved. Choose another.`;
	return undefined;
}

/** Why a name can't be used, or undefined. Duplicate display names are allowed (PEOPLE-4). */
export function nameProblem(name: string): string | undefined {
	const n = name.trim();
	if (n === '') return 'Enter their name.';
	// eslint-disable-next-line no-control-regex -- rejects ASCII control characters in member names
	if (/[\u0000-\u001f\u007f]/.test(n)) return 'Remove the control characters.';
	return undefined;
}
