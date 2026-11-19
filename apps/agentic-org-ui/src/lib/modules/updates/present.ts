// Words and order for Organization settings › Updates. Pure, so they are unit-tested (present.test.ts).
import type { OfficeVersion, UpdateCheck } from './types';

/**
 * Compares two release versions numerically by their dotted parts ("0.10.0" is newer than "0.9.2"). A pre-release
 * ("1.0.0-rc.1") is older than its release. Returns null when either can't be read as a version, so the screen says
 * "Version unknown" rather than guessing an order.
 */
export function compareVersions(a: string, b: string): -1 | 0 | 1 | null {
	const parse = (v: string) => {
		const m = /^v?(\d+(?:\.\d+)*)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/.exec(v.trim());
		return m ? { parts: m[1].split('.').map(Number), pre: m[2] } : null;
	};
	const x = parse(a);
	const y = parse(b);
	if (!x || !y) return null;
	const length = Math.max(x.parts.length, y.parts.length);
	for (let i = 0; i < length; i++) {
		const d = (x.parts[i] ?? 0) - (y.parts[i] ?? 0);
		if (d !== 0) return d < 0 ? -1 : 1;
	}
	if (x.pre === y.pre) return 0;
	if (x.pre === undefined) return 1;
	if (y.pre === undefined) return -1;
	return x.pre.localeCompare(y.pre, 'en', { numeric: true }) < 0 ? -1 : 1;
}

export type Tone = 'ok' | 'owner' | 'neutral' | 'bad';

/** How an office's release stands against HQ's, in words; amber only when the owner has to act. */
export function officeStanding(
	office: OfficeVersion,
	hqVersion: string
): { word: string; tone: Tone } | null {
	if (office.role === 'hq') return null;
	if (office.holdsBack) return { word: 'Needs update', tone: 'owner' };
	if (office.version === null) return { word: 'Version unknown', tone: 'neutral' };
	switch (compareVersions(office.version, hqVersion)) {
		case 0:
			return { word: 'Same as HQ', tone: 'neutral' };
		case -1:
			return { word: 'Older than HQ', tone: 'neutral' };
		case 1:
			return { word: 'Newer than HQ', tone: 'neutral' };
		default:
			return { word: 'Version unknown', tone: 'neutral' };
	}
}

/** HQ first, then the other offices A–Z (offices-and-teams-ux §6.1). */
export function sortOffices<T extends Pick<OfficeVersion, 'role' | 'name'>>(offices: T[]): T[] {
	return offices.toSorted((a, b) =>
		a.role === b.role
			? a.name.localeCompare(b.name, undefined, { sensitivity: 'base' })
			: a.role === 'hq'
				? -1
				: 1
	);
}

/** The offices spec's own note (§6.1), for the offices that hold new features back. */
export function holdBackNote(offices: Pick<OfficeVersion, 'name' | 'holdsBack'>[]): string | null {
	const names = offices.filter((o) => o.holdsBack).map((o) => o.name);
	if (names.length === 0) return null;
	const list =
		names.length === 1 ? names[0] : `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`;
	return `${list} ${names.length === 1 ? 'runs' : 'run'} an older version of Agentic Org, so new features wait until ${names.length === 1 ? 'it updates' : 'they update'}.`;
}

/** The local command that upgrades the selected instance (server spec §5); quoted only when the name needs it. */
export function upgradeCommand(instance: string): string {
	const name = /^[A-Za-z0-9._-]+$/.test(instance)
		? instance
		: `'${instance.replace(/'/g, `'\\''`)}'`;
	return `agentic-org upgrade --name ${name}`;
}

/**
 * When something happened, as event lists show it (app-shell-ux "Account time display"): relative within 24 hours,
 * otherwise an absolute 24-hour date and time.
 */
export function whenText(iso: string, now: number = Date.now(), timeZone?: string): string {
	const at = Date.parse(iso);
	if (Number.isNaN(at)) return 'at an unknown time';
	const minutes = Math.round((now - at) / 60_000);
	if (minutes >= 0 && minutes < 24 * 60) {
		if (minutes < 1) return 'just now';
		if (minutes < 60) return `${minutes} ${minutes === 1 ? 'minute' : 'minutes'} ago`;
		const hours = Math.floor(minutes / 60);
		return `${hours} ${hours === 1 ? 'hour' : 'hours'} ago`;
	}
	const text = new Intl.DateTimeFormat('en-GB', {
		day: 'numeric',
		month: 'short',
		year: 'numeric',
		hour: '2-digit',
		minute: '2-digit',
		hourCycle: 'h23',
		...(timeZone ? { timeZone } : {})
	}).format(at);
	return `on ${text}`;
}

/** Ends a reason with one full stop, whatever HQ sent. */
const sentence = (text: string) => text.trim().replace(/[.!?]*$/, '.');

/** The update check in words: a status word and tone for the pill, and the sentence beside it. */
export function checkSummary(
	check: UpdateCheck,
	now: number = Date.now()
): { word: string; tone: Tone; detail: string } {
	switch (check.state) {
		case 'up_to_date':
			return {
				word: 'Up to date',
				tone: 'ok',
				detail: `No newer release was found. Checked ${whenText(check.checkedAt, now)}.`
			};
		case 'available':
			return {
				word: 'Update available',
				tone: 'neutral',
				detail: `Version ${check.version} is available${check.staged ? ', downloaded and verified' : ''}. Found ${whenText(check.checkedAt, now)}. Nothing changes until you upgrade.`
			};
		case 'failed':
			return {
				word: 'Check failed',
				tone: 'bad',
				detail: `The last check for a newer release failed ${whenText(check.checkedAt, now)}: ${sentence(check.reason)} This instance keeps running its current release.`
			};
		case 'offline':
			return {
				word: 'Offline',
				tone: 'neutral',
				detail: "This instance runs offline, so it doesn't check for newer releases."
			};
		default:
			return {
				word: 'Not reported',
				tone: 'neutral',
				detail: "HQ hasn't reported whether a newer release exists."
			};
	}
}
