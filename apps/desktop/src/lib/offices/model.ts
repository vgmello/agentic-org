// The Offices screen's rules as pure functions (UX spec §3.1). Phase 1 shows HQ only and changes nothing:
// adding and removing offices come with the local satellite in phase 2.
import type { Member, OfficeInfo, OfficeStatus, OrgSnapshot } from '$lib/proto';

const THIS_MAC = 'This Mac';

export const isHq = (o: OfficeInfo) => o.role === 'hq';
// On this Mac: it has a local endpoint (the host label is the fallback).
export const isHere = (o: OfficeInfo) => !!o.endpoint || o.host === THIS_MAC;

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

const byName = (a: OfficeInfo, b: OfficeInfo) => a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });

/**
 * The list order (OFC-1): HQ, then this Mac's other offices, then the rest, each A–Z, then
 * invitations (offices that haven't joined yet).
 */
export function orderOffices(offices: OfficeInfo[]): OfficeInfo[] {
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
export function unplacedAgents(s: OrgSnapshot, offices: OfficeInfo[]): Member[] {
	const ids = new Set(offices.map((o) => o.id));
	return s.members.filter((m) => m.kind === 'agent' && (!m.office_id || !ids.has(m.office_id)));
}

/** "This Mac", "invitation" or where it runs: the line under an office's name (OFC-1). */
export function secondaryText(o: OfficeInfo): string {
	if (o.status === 'pending') return 'invitation';
	if (isHere(o)) return THIS_MAC;
	return o.host;
}
