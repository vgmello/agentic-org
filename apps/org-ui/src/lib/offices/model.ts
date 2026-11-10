// The Offices screen's rules as pure functions (UX spec §3.1). Phase 1 shows HQ only and changes nothing:
// adding and removing offices come with the local satellite in phase 2.
import { isFormer } from '$lib/org/model';
import type { Member, OfficeInfo, OfficeStatus, OfficeStatusReason, OrgSnapshot, StatusUpdate } from '$lib/proto';

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
export function displayStatus(connected: boolean, check: LiveCheck, reported?: OfficeStatus, capabilities?: string[]): DisplayStatus {
	if (!connected) return 'status_unknown';
	// A host whose hello doesn't list `office.status_changed` is not live: what it reported is not trusted (spec rev 9).
	if (check === 'up' && capabilities && !capabilities.includes('office.status_changed')) return 'status_unknown';
	// An office on this Mac never reads Offline, only Stopped (spec §3.1); the connected office is always on this Mac.
	if (check === 'up') return reported === 'offline' ? 'unavailable' : reported && reported !== 'active' ? reported : 'active';
	return check === 'down' ? 'unreachable' : 'checking';
}

/** `displayStatus` for an office in a list: connected means it is the office the app is connected to. */
export const displayStatusFor = (officeId: string, connectedOfficeId: string, check: LiveCheck, reported?: OfficeStatus, capabilities?: string[]): DisplayStatus =>
	displayStatus(officeId === connectedOfficeId, check, reported, capabilities);

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

/**
 * The organization's offices after a status update from the host. The full list replaces what was known (the host
 * lists every organization on this Mac, so only this one's offices are kept); one office's record replaces its
 * older one, or joins the list when it is new to this organization. Another organization's office changes nothing.
 */
export function applyStatusUpdate(known: OfficeInfo[], organizationId: string, update: StatusUpdate): OfficeInfo[] {
	if (update.kind === 'offices') return update.offices.filter((o) => o.organization_id === organizationId);
	const { office } = update;
	if (office.organization_id !== organizationId) return known;
	return known.some((o) => o.id === office.id) ? known.map((o) => (o.id === office.id ? office : o)) : [...known, office];
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
export function rowLabel(o: OfficeInfo, agents: number | undefined, here: boolean, status: DisplayStatus | undefined, why?: string): string {
	const parts = [o.name];
	if (isHq(o)) parts.push('HQ');
	// An office that can't be reached shows "Can't reach this office" where its location was: the label says the same.
	// Where it runs always stays; a stopped office's reason follows its status (spec §6.1).
	parts.push(status === 'unreachable' ? "Can't reach this office" : secondaryText(o) + (here ? " (you're here)" : ''));
	parts.push(statusInfo(status).label + (why ? `: ${why}` : ''));
	if (agents !== undefined) parts.push(`${agents} ${agents === 1 ? 'agent' : 'agents'}`);
	parts.push('opens details');
	return parts.join(', ');
}

// ---- why an office stopped (spec §3.3, OFC-11..13) ----------------------------------------------------------------

/** The line under a stopped office's name, from the host's `status_reason`. */
const REASON_COPY: Record<Exclude<OfficeStatusReason, 'unsupported_version'>, string> = {
	crashed: 'It stopped unexpectedly',
	start_timeout: "It didn't start in time",
	restarts_exhausted: "It kept stopping, so it won't restart on its own",
	storage_error: "It can't read or write its data on this Mac",
	in_use: 'Its data is still in use by another Agentic Org process',
	key_missing: "This office's key isn't available on this Mac, so its data can't be opened",
	stopped_by_owner: 'You stopped it',
	invalid_request: "It couldn't start because of an internal error",
	unknown: "It stopped for a reason this version doesn't know"
};

type OfficeStop = Pick<OfficeInfo, 'host' | 'status' | 'status_reason' | 'next_restart_at'>;

/** The reason line for the screen-reader label: it never ticks, so a pending restart reads "restarting shortly" (spec §3.3). */
export function whyForLabel(o: OfficeStop, now: Date): string | undefined {
	const text = whyStopped({ ...o, next_restart_at: undefined }, now);
	if (text === undefined) return undefined;
	const lower = text.charAt(0).toLowerCase() + text.slice(1);
	const n = secondsUntil(o.next_restart_at, now);
	if (n === undefined || n === 0) return lower;
	// A restart a minute or more away is read as a clock time, which does not tick; a nearer one as "shortly".
	if (n < 60) return `${lower}, restarting shortly`;
	const at = new Date(Date.parse(o.next_restart_at!)).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });
	return `${lower}, restarting at ${at}`;
}

/** Seconds from `now` until `next_restart_at`, rounded up; never negative. Nothing when it is absent or unreadable. */
export function secondsUntil(nextRestartAt: string | undefined, now: Date): number | undefined {
	if (!nextRestartAt) return undefined;
	// RFC 3339 with a time zone only: Date.parse alone accepts junk like "5".
	if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$/.test(nextRestartAt)) return undefined;
	const t = Date.parse(nextRestartAt);
	return Number.isNaN(t) ? undefined : Math.max(0, Math.ceil((t - now.getTime()) / 1000));
}

/**
 * Why a stopped office stopped, in words, with the countdown while a restart is pending ("Restarting in 20 s"; for
 * `in_use` "Trying again in 20 s"). Only an office the host reports `offline` or `unavailable` has a reason line; a
 * missing reason reads "It isn't running". The line comes from the host's status, never from the app's own check.
 */
export function whyStopped(o: OfficeStop, now: Date): string | undefined {
	if (o.status !== 'offline' && o.status !== 'unavailable') return undefined;
	if (o.status_reason === 'unsupported_version')
		return `The release components on ${o.host} don't match. Run diagnostics and repair the selected instance there.`;
	const base = o.status_reason ? (Object.hasOwn(REASON_COPY, o.status_reason) ? REASON_COPY[o.status_reason] : REASON_COPY.unknown) : "It isn't running";
	const n = secondsUntil(o.next_restart_at, now);
	// A restart whose time has come is not announced as "in 0 s": the next status will say what happened.
	if (n === undefined || n === 0) return base;
	return o.status_reason === 'in_use' ? `${base}. Trying again in ${n} s` : `${base}. Restarting in ${n} s`;
}

/** What "Copy details" puts on the clipboard (spec §3.3): the office, its status and reason, the time and the app version. Never keys or tokens. */
export function copyDetails(o: OfficeInfo, statusLabel: string, now: Date, appVersion: string): string {
	return [
		`Office: ${o.name}`,
		`Id: ${o.id}`,
		`Status: ${statusLabel}`,
		`Reason: ${o.status_reason ?? 'not reported'}`,
		...(o.next_restart_at ? [`Next restart: ${o.next_restart_at}`] : []),
		`Time: ${now.toISOString()}`,
		`Agentic Org: ${appVersion}`
	].join('\n');
}

/** What can be done about a stopped office (spec §3.3): the button's words, or why there is none. */
export interface RestartAction {
	/** "Restart office", "Try again" or "Start office"; absent when the contract refuses a restart for this reason. */
	label?: string;
	/** A line instead of a button (a host that can't restart from here). */
	note?: string;
}

export function restartAction(o: Pick<OfficeInfo, 'status' | 'status_reason'>, capabilities?: string[]): RestartAction {
	if (o.status !== 'offline' && o.status !== 'unavailable') return {};
	// The contract refuses a restart only for these two reasons.
	if (o.status_reason === 'unsupported_version' || o.status_reason === 'invalid_request') return {};
	if (capabilities && !capabilities.includes('office.restart')) return { note: 'Update Agentic Org to restart offices from here' };
	if (o.status_reason === 'key_missing') return { label: 'Try again' };
	if (o.status_reason === 'stopped_by_owner') return { label: 'Start office' };
	return { label: 'Restart office' };
}

/** What `office.restart` answered, in the owner's words (spec §3.3); `ignore` is an answer that is no error. */
export function restartOutcome(code: string): { kind: 'wait' | 'unsupported' | 'refresh' | 'error'; message?: string } {
	switch (code) {
		case 'conflict':
			return { kind: 'wait' };
		case 'unknown_method':
			return { kind: 'unsupported', message: 'Update Agentic Org to restart offices from here' };
		case 'not_found':
		case 'unavailable':
			return { kind: 'refresh' };
		case 'not_authorized':
			return { kind: 'error', message: "You can't restart this office from here." };
		case 'not_ready':
			return { kind: 'error', message: 'The office host is still starting. Try again in a moment.' };
		default:
			return { kind: 'error', message: "The office couldn't be restarted. Try again in a moment." };
	}
}
