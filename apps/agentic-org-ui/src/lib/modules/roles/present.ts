// Words and checks for the Roles screens (roles-ux §1, ROLE-10). Pure: no API, no DOM.
import { toAppError } from '$lib/proto';
import type { Attribution, Role, RoleDraft, RoleField, RoleSummary } from './types';

/** roles-ux §1, word for word. */
export const HR_CANT_RUN = "The HR lead can't run right now, so it can't draft roles.";

/**
 * The empty list's copy. It only invites creating a role when the viewer can make changes now, and only offers
 * HR's draft when the HR lead can also run (ROLE-10: empty copy never promises what can't happen).
 */
export function emptyRolesText(hrCanRun: boolean, canChange = true): string {
	if (!canChange) return "Roles describe employees' work. None have been created yet.";
	return hrCanRun
		? "Roles describe employees' work. Create one, or ask HR to draft one."
		: "Roles describe employees' work. Create one.";
}

/** Why New role, Ask HR, Edit and Fix are disabled while organization data can't be reached (RL-8). */
export const CHANGES_PAUSED = 'Changes are paused until organization data can be reached.';

/** Shown when Save is pressed with nothing changed: no revision is added. */
export const NOTHING_TO_SAVE = "Nothing changed, so there's nothing to save.";

/** "Can't reach organization data. Showing the last copy from <time>." (roles-ux §1). */
export const staleText = (time: string) =>
	`Can't reach organization data. Showing the last copy from ${time}.`;

/** "<role>: details not recognized (<field>)." (roles-ux §1). */
export const invalidText = (name: string, field: string) =>
	`${name}: details not recognized (${field}).`;

/** The spec's list wording: "4 employees", "1 employee", "no employees yet". */
export function employeesText(count: number): string {
	if (count <= 0) return 'no employees yet';
	return count === 1 ? '1 employee' : `${count} employees`;
}

/** The usual body headings (roles-ux decision 1). They are prose, not enforced fields. */
export const ROLE_TEMPLATE =
	'## Mission\n\n\n## Responsibilities\n\n\n## Working style\n\n\n## Boundaries\n';

/** Roles A–Z by name, then by key, so equal names keep a stable order. */
export function sortRoles<T extends Pick<RoleSummary, 'name' | 'key' | 'id'>>(roles: T[]): T[] {
	return roles.toSorted(
		(a, b) =>
			a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) ||
			(a.key ?? '').localeCompare(b.key ?? '') ||
			a.id.localeCompare(b.id)
	);
}

/** Who wrote a record, in words. A system write (a development process creating a role) is Agentic Org. */
export function actorName(a: Attribution): string {
	if (a.byName) return a.byName;
	if (a.byKind === 'system') return 'Agentic Org';
	if (a.byKind === 'employee') return 'A former employee';
	return 'Not recorded';
}

export const KEY_MAX = 64;
const KEY_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/** Accepts what people type and paste: surrounding spaces go, a key is lower-cased with spaces as hyphens. */
export function normalizeDraft(draft: RoleDraft): RoleDraft {
	const key = (draft.key ?? '').trim().toLowerCase().replace(/\s+/g, '-');
	return {
		name: draft.name.trim().replace(/\s+/g, ' '),
		...(key ? { key } : {}),
		bodyMarkdown: draft.bodyMarkdown.replace(/\s+$/, '') + '\n'
	};
}

/** The editor's own checks before sending; the server stays the authority. Keys are of a normalized draft. */
export function checkDraft(draft: RoleDraft): Partial<Record<RoleField, string>> {
	const errors: Partial<Record<RoleField, string>> = {};
	if (!draft.name) errors.name = 'Give the role a name.';
	else if (draft.name.length > 120) errors.name = 'Use a name of 120 characters or fewer.';
	// eslint-disable-next-line no-control-regex -- rejects ASCII control characters in role names
	else if (/[\u0000-\u001f\u007f]/.test(draft.name))
		errors.name = 'Use a name without line breaks or tabs.';
	if (draft.key !== undefined) {
		if (draft.key.length > KEY_MAX) errors.key = `Use a key of ${KEY_MAX} characters or fewer.`;
		else if (!KEY_PATTERN.test(draft.key))
			errors.key = 'Use lowercase letters, digits and single hyphens, like software-engineer.';
	}
	return errors;
}

/** Whether a normalized draft says the same as the role: saving it would only add a revision. */
export function unchanged(role: Pick<Role, 'name' | 'key' | 'bodyMarkdown'>, draft: RoleDraft) {
	return (
		role.name === draft.name &&
		(role.key ?? '') === (draft.key ?? '') &&
		role.bodyMarkdown.replace(/\s+$/, '') === draft.bodyMarkdown.replace(/\s+$/, '')
	);
}

/** The editor field a refusal is about, when the server named one. */
export function refusedField(e: unknown): RoleField | undefined {
	const field = (e as { field?: unknown } | null)?.field;
	return field === 'name' || field === 'key' || field === 'body' ? field : undefined;
}

/** The role as it is now, when a save was refused because it changed meanwhile. */
export function conflictLatest(e: unknown): Role | undefined {
	if (toAppError(e).code !== 'conflict') return undefined;
	const latest = (e as { latest?: unknown }).latest;
	return latest && typeof latest === 'object' && 'revision' in latest
		? (latest as Role)
		: undefined;
}

/** A role's Markdown body as plain blocks: headings, bullet lists and paragraphs; everything else stays text. */
export type RoleBlock =
	| { kind: 'heading'; text: string }
	| { kind: 'list'; items: string[] }
	| { kind: 'paragraph'; text: string };

export function roleBlocks(body: string): RoleBlock[] {
	const blocks: RoleBlock[] = [];
	let paragraph: string[] = [];
	const flush = () => {
		if (paragraph.length) blocks.push({ kind: 'paragraph', text: paragraph.join(' ') });
		paragraph = [];
	};
	for (const raw of body.split(/\r?\n/)) {
		const line = raw.trim();
		const heading = /^#{1,6}(?:\s+(.*?))?\s*#*$/.exec(line);
		const item = /^(?:[-*+]|\d+[.)])\s+(.*)$/.exec(line);
		if (!line) flush();
		else if (heading) {
			flush();
			if (heading[1]) blocks.push({ kind: 'heading', text: heading[1] });
		} else if (item) {
			flush();
			const last = blocks.at(-1);
			if (last?.kind === 'list') last.items.push(item[1]);
			else blocks.push({ kind: 'list', items: [item[1]] });
		} else paragraph.push(line);
	}
	flush();
	return blocks;
}

/** Whether a body says anything beyond its headings. */
export const hasContent = (body: string) =>
	roleBlocks(body).some((block) => block.kind !== 'heading');
