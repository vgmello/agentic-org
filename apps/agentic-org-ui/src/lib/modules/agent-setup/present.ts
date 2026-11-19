// Agent setup's facts in words (our-org-in-the-app §5, §9; employee-supervision-ux §1). Pure: no I/O.
import type { tone } from '$lib/components/pills';
import type {
	AgentSetupRow,
	ConnectionState,
	ExecutionSummary,
	Placement,
	ProfileChoice,
	ProfileModel
} from './types';

/** Observations older than this are labelled as possibly out of date (document 11: 300 s evidence window). */
export const STALE_MS = 5 * 60_000;

const durationText = (ms: number) => {
	const s = Math.max(0, Math.round(ms / 1000));
	if (s < 60) return `${s} s`;
	const m = Math.round(s / 60);
	if (m < 60) return `${m} min`;
	return `${Math.round(m / 60)} h`;
};

/** "20 s ago", "12 min ago", "3 h ago", or the day for older ones. */
export function ageText(iso: string | undefined, now = Date.now()): string {
	if (!iso) return 'never';
	const ms = now - Date.parse(iso);
	if (!Number.isFinite(ms)) return 'at an unknown time';
	if (ms < 24 * 3600_000) return `${durationText(ms)} ago`;
	return `on ${new Date(iso).toLocaleDateString('en-GB', { day: 'numeric', month: 'short' })}`;
}

export const isStale = (iso: string | undefined, now = Date.now()) =>
	!iso || now - Date.parse(iso) > STALE_MS;

/** The list's Office cell: the office, or Unplaced. */
export const officeText = (p: Placement | null) => (p ? p.office.name : 'Unplaced');

/** "reported 2 min ago" / "never reported" under the office. */
export const reportedText = (p: Placement | null, now = Date.now()) =>
	p ? (p.observedAt ? `reported ${ageText(p.observedAt, now)}` : 'never reported') : undefined;

/** The configured runtime(s), in priority order: "Claude Code", "Claude Code, then Codex". */
export const runtimeText = (e: ExecutionSummary | null) =>
	!e ? undefined : e.runtimes.length === 0 ? 'No runtime' : e.runtimes.join(', then ');

/** Agents A–Z by name, the built-in HR lead first: a known name is found fastest in an ordered list. */
export function sortAgents<T extends Pick<AgentSetupRow, 'name' | 'builtIn'>>(rows: T[]): T[] {
	return [...rows].sort(
		(a, b) => Number(b.builtIn) - Number(a.builtIn) || a.name.localeCompare(b.name)
	);
}

export const agentsText = (n: number) => (n === 1 ? 'agent' : 'agents');

/** The list's description: what the page is for, plus how many need setup when any do. */
export function listDescription(total: number, needSetup: number): string {
	const base =
		'How each agent runs: its runtime, model and model profile, its office and its health.';
	if (total === 0 || needSetup === 0) return base;
	return `${base} ${needSetup} of ${total} ${needSetup === 1 ? 'needs' : 'need'} setup.`;
}

export const CONNECTION_STATE: Record<ConnectionState, string> = {
	ready: 'Ready',
	signed_out: 'Not signed in',
	not_installed: 'Runtime not installed',
	binding_missing: 'Not set up on this office',
	cooling_down: 'Cooling down after a limit',
	unknown: 'Not checked yet'
};

/**
 * A connection state's shared tone (`pills.ts`): amber only where the owner has something to do on the office
 * (sign in, install, bind); a cooldown passes by itself and unknown is unknown, so both are neutral.
 */
export const CONNECTION_TONE: Record<ConnectionState, keyof typeof tone> = {
	ready: 'ok',
	signed_out: 'owner',
	not_installed: 'owner',
	binding_missing: 'owner',
	cooling_down: 'neutral',
	unknown: 'neutral'
};

/** A model option in Edit: what the office reports, never a promise of capacity. */
export function modelNote(m: ProfileModel): string {
	if (m.supporting === 0) return 'not reported by any connection';
	const of = `${m.ready} of ${m.supporting} ${m.supporting === 1 ? 'connection' : 'connections'} ready`;
	return m.stale ? `${of} · last check out of date` : of;
}

/** A profile option in Edit: "Primary pool · Claude Code". */
export const profileLabel = (p: ProfileChoice) =>
	p.runtimes.length ? `${p.name} · ${p.runtimes.join(', ')}` : p.name;

/** The tools line: "3 approved tools" or "No tools". */
export const toolsText = (tools: string[]) =>
	tools.length === 0
		? 'No tools'
		: `${tools.length} approved ${tools.length === 1 ? 'tool' : 'tools'}`;

/**
 * The Offboard preview (our-org-in-the-app §9): the organization's own summary of team, task and report changes,
 * with the retention line in the spec's words.
 */
export function offboardLines(summary: string[], name: string): string[] {
	const retention = `${name}'s record stays`;
	return [
		...summary.filter((line) => !line.startsWith(retention)),
		`${name}'s profile and home-team history stay in organization data. The handle stays reserved.`
	];
}

/** After Resume: the holds that still prevent a start (Resume clears only the manual one, RUN-10). */
export const remainingHolds = (holds: string[]) => holds.filter((h) => !/manual/i.test(h));

/** A fix as the office words it, with `commands` in backticks set apart: never interpreted as markup. */
export function codeParts(text: string): { text: string; code: boolean }[] {
	const parts = text.split('`');
	// An unpaired backtick is shown as written.
	if (parts.length % 2 === 0) return [{ text, code: false }];
	return parts.map((part, i) => ({ text: part, code: i % 2 === 1 })).filter((p) => p.text !== '');
}
