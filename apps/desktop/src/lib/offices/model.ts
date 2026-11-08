// The Offices screen's rules as pure functions (UX spec §3.1). Phase 1 shows HQ only and changes nothing:
// adding and removing offices come with the local satellite in phase 2.
import { isFormer } from '$lib/org/model';
import type { Member, OfficeInfo, OfficeStatus, OrgSnapshot } from '$lib/proto';

const THIS_MAC = 'This Mac';

export const isHq = (o: OfficeInfo) => o.role === 'hq';
// On this Mac: it has a local endpoint (the host label is the fallback).
export const isHere = (o: OfficeInfo) => !!o.endpoint || o.host === THIS_MAC;

/** The owner-facing word for an office's role: an unrecognised role is never described as HQ or as an office that keeps a copy. */
export function roleInfo(o: OfficeInfo): { label: string; description: string } {
	switch (o.role) {
		case 'hq':
			return { label: 'HQ', description: 'HQ keeps the records and approves new offices.' };
		case 'satellite':
			return { label: 'Office', description: 'An office keeps a copy of the records.' };
		default:
			return { label: 'Unknown', description: "This app doesn't recognise this office's role." };
	}
}

/** An icon name for a status (see `statusIcon`). */
export type StatusIcon = 'circle' | 'circle-outline' | 'loader' | 'circle-x' | 'circle-arrow-up' | 'mail' | 'circle-help' | 'wifi-off';

/**
 * What the app has verified about the office it is connected to (OFC-12). Until the host reports status live
 * (`office.status_changed`), this is the only live signal: the office answered `hello`, or it didn't.
 */
export type LiveCheck = 'checking' | 'up' | 'down';

/** A status as shown: the saved ones, plus what the app itself knows (spec §3.1, OFC-12). */
export type DisplayStatus = OfficeStatus | 'unreachable' | 'checking' | 'status_unknown';

/**
 * The status to show for an office. The saved status is never shown as if it were live: the office this app is
 * connected to is Online only while the connection is verified AND the host reports it active (the host answering is
 * not enough: a stopped office under a running host must not read Online, OFC-12). If it answers but reports another
 * status (Starting, Stopped…), that status is shown. Any other office reads "Status unknown".
 */
export function displayStatus(connected: boolean, check: LiveCheck, reported?: OfficeStatus): DisplayStatus {
	if (!connected) return 'status_unknown';
	if (check === 'up') return reported && reported !== 'active' ? reported : 'active';
	return check === 'down' ? 'unreachable' : 'checking';
}

/** `displayStatus` for an office in a list: connected means it is the office the app is connected to. */
export const displayStatusFor = (officeId: string, connectedOfficeId: string, check: LiveCheck, reported?: OfficeStatus): DisplayStatus =>
	displayStatus(officeId === connectedOfficeId, check, reported);

/** The owner-facing word for an office's status, with its icon family and tone (spec §3.1). */
export function statusInfo(status: DisplayStatus | undefined): { label: string; tone: 'ok' | 'info' | 'muted' | 'warn' | 'bad' } {
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
		case 'unreachable':
			return { label: 'Unreachable', tone: 'bad' };
		case 'checking':
			return { label: 'Checking…', tone: 'info' };
		default:
			return { label: 'Status unknown', tone: 'muted' };
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
	s.members.filter((m) => m.kind === 'agent' && !isFormer(m) && m.office_id === officeId);

/** Agents placed in an office that isn't in the list: they are "unplaced" (spec §6.4). */
export function unplacedAgents(s: OrgSnapshot, offices: OfficeInfo[]): Member[] {
	const ids = new Set(offices.map((o) => o.id));
	return s.members.filter((m) => m.kind === 'agent' && !isFormer(m) && (!m.office_id || !ids.has(m.office_id)));
}

/** "This Mac", "invitation" or where it runs: the line under an office's name (OFC-1). */
export function secondaryText(o: OfficeInfo): string {
	if (o.status === 'pending') return 'invitation';
	if (isHere(o)) return THIS_MAC;
	return o.host;
}

/** The Lucide icon for each status (spec §3.1). The word always shows too: an icon alone never carries the status. */
export function statusIcon(status: DisplayStatus | undefined): StatusIcon {
	switch (status) {
		case 'active':
			return 'circle';
		case 'starting':
		case 'checking':
			return 'loader';
		case 'unreachable':
			return 'wifi-off';
		case 'offline':
			return 'circle-outline';
		case 'unavailable':
			return 'circle-x';
		case 'update_needed':
			return 'circle-arrow-up';
		case 'pending':
			return 'mail';
		default:
			return 'circle-help';
	}
}

/**
 * What a screen reader hears for an office row (OFC-13): its name, HQ, where it runs, its status and how many
 * agents it has. `agents` is undefined while they are still being counted.
 */
export function rowLabel(o: OfficeInfo, agents: number | undefined, here: boolean, status: DisplayStatus | undefined): string {
	const parts = [o.name];
	if (isHq(o)) parts.push('HQ');
	// An office that can't be reached shows "Can't reach this office" where its location was: the label says the same.
	parts.push(status === 'unreachable' ? "Can't reach this office" : secondaryText(o) + (here ? " (you're here)" : ''));
	parts.push(statusInfo(status).label);
	if (agents !== undefined) parts.push(`${agents} ${agents === 1 ? 'agent' : 'agents'}`);
	parts.push('opens details');
	return parts.join(', ');
}
