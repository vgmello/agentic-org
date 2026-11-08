// The Offices screen's rules as pure functions (UX spec §3.1 and §6). Nothing here changes data.
import type { Member, OfficeStatus, OfficeWithExtras, OrgSnapshot } from '$lib/proto';

export const MAX_OFFICE_NAME = 64;

/** The steps of adding an office on this Mac, in order (spec §6.2). */
export const ADD_STEPS = ['Creating', 'Copying records', 'Starting'] as const;
export type AddStep = (typeof ADD_STEPS)[number];
const THIS_MAC = 'This Mac';

export const isHq = (o: OfficeWithExtras) => o.role === 'hq';
export const isHere = (o: OfficeWithExtras) => o.host === THIS_MAC;

/** The owner-facing word for an office's status, with its icon family and tone (spec §3.1). */
export function statusInfo(status: OfficeStatus | undefined): { label: string; tone: 'ok' | 'info' | 'muted' | 'warn' | 'bad' } {
	switch (status) {
		case 'active':
			return { label: 'Online', tone: 'ok' };
		case 'starting':
			return { label: 'Starting', tone: 'info' };
		case 'pending':
			return { label: 'Invited', tone: 'muted' };
		case 'offline':
			return { label: 'Offline', tone: 'muted' };
		case 'unavailable':
			return { label: 'Stopped', tone: 'bad' };
		case 'update_needed':
			return { label: 'Update needed', tone: 'warn' };
		default:
			return { label: 'Unknown', tone: 'muted' };
	}
}

const byName = (a: OfficeWithExtras, b: OfficeWithExtras) => a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });

/**
 * The list order (OFC-1): HQ, then this Mac's other offices, then the rest, each A–Z, then
 * invitations (offices that haven't joined yet).
 */
export function orderOffices(offices: OfficeWithExtras[]): OfficeWithExtras[] {
	const invited = offices.filter((o) => o.status === 'pending' && !isHq(o));
	const joined = offices.filter((o) => !invited.includes(o));
	const hq = joined.filter(isHq).sort(byName);
	const here = joined.filter((o) => !isHq(o) && isHere(o)).sort(byName);
	const elsewhere = joined.filter((o) => !isHq(o) && !isHere(o)).sort(byName);
	return [...hq, ...here, ...elsewhere, ...invited.sort(byName)];
}

/** The agents placed in an office (people aren't placed in offices). */
export const agentsIn = (s: OrgSnapshot, officeId: string): Member[] =>
	s.members.filter((m) => m.kind === 'agent' && m.office_id === officeId);

/** Agents placed in an office that isn't in the list: they are "unplaced" (spec §6.4). */
export function unplacedAgents(s: OrgSnapshot, offices: OfficeWithExtras[]): Member[] {
	const ids = new Set(offices.map((o) => o.id));
	return s.members.filter((m) => m.kind === 'agent' && (!m.office_id || !ids.has(m.office_id)));
}

/** "last seen 2 h ago", "This Mac", "invitation": the line under an office's name (OFC-1). */
export function secondaryText(o: OfficeWithExtras, now: number): string {
	if (o.status === 'pending') return 'invitation';
	// A stopped office says why, in plain words (spec §3.1).
	if (o.status === 'unavailable' && o.stopped_reason) return o.stopped_reason;
	if (isHere(o)) return THIS_MAC;
	if (o.status === 'offline' && o.last_seen) return `last seen ${ago(o.last_seen, now)}`;
	return o.host;
}

/** A short "2 h ago" / "5 min ago" / "just now". */
export function ago(rfc3339: string, now: number): string {
	const seconds = Math.max(0, Math.round((now - new Date(rfc3339).getTime()) / 1000));
	if (Number.isNaN(seconds)) return rfc3339;
	if (seconds < 60) return 'just now';
	if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
	if (seconds < 86400) return `${Math.floor(seconds / 3600)} h ago`;
	return `${Math.floor(seconds / 86400)} d ago`;
}

export type OfficeNameCheck = { ok: true; name: string } | { ok: false; message: string };

/**
 * An office's name (spec §6.2): required, 1–64 characters, unique among the organization's
 * offices (ignoring case and surrounding spaces).
 */
export function validateOfficeName(raw: string, existing: OfficeWithExtras[]): OfficeNameCheck {
	const name = raw.trim();
	if (name === '') return { ok: false, message: 'Give the office a name.' };
	if ([...name].length > MAX_OFFICE_NAME) return { ok: false, message: `Use ${MAX_OFFICE_NAME} characters or fewer.` };
	// eslint-disable-next-line no-control-regex
	if (/[\u0000-\u001f\u007f-\u009f]/.test(name)) return { ok: false, message: "The name can't contain control characters." };
	const taken = existing.find((o) => o.name.trim().toLowerCase() === name.toLowerCase());
	if (taken) return { ok: false, message: `There's already an office called ${taken.name}.` };
	return { ok: true, name };
}

/** The name offered for a new office: "Office 2", "Office 3"…, the first not taken. */
export function suggestOfficeName(existing: OfficeWithExtras[]): string {
	for (let n = 2; ; n++) {
		const name = `Office ${n}`;
		if (!existing.some((o) => o.name.toLowerCase() === name.toLowerCase())) return name;
	}
}

/**
 * What removing an office does (spec §6.4), for the confirmation: who stops running and becomes
 * unplaced. HQ can't be removed.
 */
export function removeConsequences(o: OfficeWithExtras, s: OrgSnapshot): { allowed: boolean; agents: Member[]; offline: boolean } {
	return { allowed: !isHq(o), agents: agentsIn(s, o.id), offline: o.status === 'offline' };
}

/**
 * The sentences of the remove confirmation (spec §6.4, OFC-8): what happens to the office, its agents
 * and the offline case. Tasks aren't in the snapshot yet, so they are named in general.
 */
export function removeSummary(o: OfficeWithExtras, orgName: string, s: OrgSnapshot): string[] {
	const { agents, offline } = removeConsequences(o, s);
	const lines = [`${o.name} can't sync again.`];
	if (agents.length > 0) {
		const names = agents.map((a) => a.name).join(', ');
		lines.push(
			`Its ${agents.length} ${agents.length === 1 ? 'agent' : 'agents'} (${names}) stop running there and become unplaced: you choose a new office for each, or offboard them. HQ takes back any open tasks they had.`
		);
	} else {
		lines.push('No agents run there.');
	}
	lines.push(
		offline
			? `${o.name} is offline, so it keeps its copy until it next connects, then deletes it.`
			: `If ${o.name} is offline when you remove it, it keeps its copy until it next connects, then deletes it.`
	);
	lines.push(`You can add it to ${orgName} again later.`);
	return lines;
}
