/** A document title (app-shell-ux §4): `pageTitle('Offices', 'Organization settings')` → "Offices · Organization settings · Agentic Org". */
export const pageTitle = (...parts: (string | undefined)[]): string =>
	[...parts.filter((part): part is string => !!part), 'Agentic Org'].join(' · ');
