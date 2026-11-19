// Words for the member screens, as pure functions: presence, turn states, who started a turn, durations and ages.
import type {
	AttemptOutcome,
	MemberRow,
	Presence,
	PresenceId,
	StartedByKind,
	Turn,
	TurnState
} from './types';

/** Presence words in the spec's priority order (our-org-in-the-app §5). */
export const PRESENCE: Record<
	PresenceId,
	{ word: string; tone: 'amber' | 'indigo' | 'green' | 'gray' | 'sky' }
> = {
	needs_you: { word: 'Needs you', tone: 'amber' },
	with_you: { word: 'With you', tone: 'indigo' },
	working: { word: 'Working', tone: 'indigo' },
	waiting_answer: { word: 'Waiting for an answer', tone: 'gray' },
	paused: { word: 'Paused', tone: 'gray' },
	available: { word: 'Available', tone: 'green' },
	offline: { word: 'Offline', tone: 'gray' },
	unknown: { word: 'Status unknown', tone: 'gray' }
};

export const presenceLabel = (p: Presence): string =>
	p.detail ? `${PRESENCE[p.id].word} ${p.detail}` : PRESENCE[p.id].word;

/** What a row shows: nothing when unknown (rows omit unknown pills, AC-IA08). */
export const rowPresence = (m: Pick<MemberRow, 'presence'>): Presence | undefined =>
	m.presence && m.presence.id !== 'unknown' ? m.presence : undefined;

/** An agent's header shows Status unknown once; a person's header shows only real presence. */
export function headerPresence(m: Pick<MemberRow, 'presence' | 'kind'>): Presence | undefined {
	if (m.presence && m.presence.id !== 'unknown') return m.presence;
	return m.kind === 'agent' ? { id: 'unknown' } : undefined;
}

export const kindWord = (kind: MemberRow['kind']) => (kind === 'agent' ? 'Agent' : 'Person');

export const TURN_STATE: Record<TurnState | 'idle', string> = {
	idle: 'No open turn',
	accepted: 'Accepted',
	running: 'Running',
	switching: 'Switching connection',
	blocked: 'Blocked',
	recovery_required: 'Recovery required',
	succeeded: 'Succeeded',
	failed: 'Failed',
	cancelled: 'Cancelled'
};

export const ATTEMPT_OUTCOME: Record<AttemptOutcome, string> = {
	running: 'Running',
	succeeded: 'Succeeded',
	failed: 'Failed',
	switched: 'Switched to the next connection',
	cancelled: 'Cancelled',
	skipped: 'Skipped'
};

/** The outcome filter on the Runs tab. */
export const OUTCOME_FILTERS = [
	{ id: 'all', label: 'All' },
	{ id: 'open', label: 'Open' },
	{ id: 'succeeded', label: 'Succeeded' },
	{ id: 'failed', label: 'Failed' },
	{ id: 'other', label: 'Cancelled or blocked' }
] as const;
export type OutcomeFilter = (typeof OUTCOME_FILTERS)[number]['id'];

const OPEN: TurnState[] = ['accepted', 'running', 'switching'];
export const isOpen = (t: Pick<Turn, 'state'>) => OPEN.includes(t.state);

export function matchesOutcome(t: Turn, filter: OutcomeFilter): boolean {
	if (filter === 'all') return true;
	if (filter === 'open') return isOpen(t);
	if (filter === 'succeeded' || filter === 'failed') return t.state === filter;
	return t.state === 'cancelled' || t.state === 'blocked' || t.state === 'recovery_required';
}

/** "Run now by you", "Message from Mira Castillo", "Assignment from Avery Stone", "Recovery". */
export function startedByText(s: Turn['startedBy']): string {
	const who = s.byViewer ? 'you' : s.by?.name;
	const words: Record<StartedByKind, string> = {
		run: 'Run now',
		message: 'Message',
		mention: 'Mention',
		ask: 'Ask',
		handoff: 'Hand-off',
		assignment: 'Assignment',
		answer: 'Answer to its question',
		resume: 'Answer to a required ask',
		recovery: 'Recovery'
	};
	const base = words[s.kind];
	if (!who || s.kind === 'recovery') return base;
	return s.kind === 'run' ? `${base} by ${who}` : `${base} from ${who}`;
}

/** The turn's one-line outcome: the summary, or "Failed: <reason>", or the state in words. */
export function outcomeText(t: Turn): string {
	if (t.state === 'failed') return `Failed: ${t.failure ?? 'no reason recorded'}`;
	if (t.state === 'succeeded') return t.summary ?? 'Succeeded';
	if (t.state === 'recovery_required')
		return `Recovery required${t.failure ? `: ${t.failure}` : ''}`;
	if (t.state === 'blocked') return `Blocked${t.failure ? `: ${t.failure}` : ''}`;
	if (t.state === 'cancelled') return t.summary ?? 'Cancelled';
	return TURN_STATE[t.state];
}

/** "4 min", "1 h 5 min", "38 s". Rounds down, so it never overstates. */
export function durationText(ms: number): string {
	const s = Math.max(0, Math.floor(ms / 1000));
	if (s < 60) return `${s} s`;
	const m = Math.floor(s / 60);
	if (m < 60) return `${m} min`;
	return `${Math.floor(m / 60)} h${m % 60 ? ` ${m % 60} min` : ''}`;
}

export function turnDuration(t: Pick<Turn, 'startedAt' | 'endedAt'>, now = Date.now()): string {
	const end = t.endedAt ? Date.parse(t.endedAt) : now;
	const text = durationText(end - Date.parse(t.startedAt));
	return t.endedAt ? text : `running ${text}…`;
}

/** A turn's time as a row says it: "ran 4 min", "running 2 min…", or "waiting for the office" before it starts. */
export function durationFact(
	t: Pick<Turn, 'startedAt' | 'endedAt' | 'state'>,
	now = Date.now()
): string {
	if (t.state === 'accepted') return 'waiting for the office to start it';
	return t.endedAt ? `ran ${turnDuration(t, now)}` : turnDuration(t, now);
}

/** Why Run now is Busy, in words that don't call a requested turn running (RUN-4). */
export function busyText(
	name: string,
	open: { startedAt: string; state: TurnState },
	now = Date.now()
) {
	const when = ageText(open.startedAt, now);
	const what =
		open.state === 'accepted'
			? `${name} already has a run waiting for the office to start it (requested ${when})`
			: `${name} is already running a turn (started ${when})`;
	return `${what}. It takes new input when that turn ends; nothing is queued.`;
}

/** The execution view's Turn line: "Running · started 2 min ago", "Requested · waiting for the office (1 min ago)". */
export function turnLine(
	turnState: TurnState | 'idle',
	open: { startedAt: string; state: TurnState } | undefined,
	now = Date.now()
): string {
	if (open?.state === 'accepted')
		return `Requested · waiting for the office to start it (requested ${ageText(open.startedAt, now)})`;
	if (open) return `${TURN_STATE[open.state]} · started ${ageText(open.startedAt, now)}`;
	return TURN_STATE[turnState];
}

/** "20 s ago", "12 min ago", "3 h ago", or the date for older observations. */
export function ageText(iso: string | undefined, now = Date.now()): string {
	if (!iso) return 'never reported';
	const ms = now - Date.parse(iso);
	if (!Number.isFinite(ms)) return 'unknown';
	if (ms < 24 * 3600_000) return `${durationText(ms)} ago`;
	return new Date(iso).toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
}

/** "reported 30 s ago", or "never reported" when the office has no observation (never "reported never reported"). */
export const reportedText = (iso: string | undefined, now = Date.now()): string =>
	iso ? `reported ${ageText(iso, now)}` : 'never reported';

/** 24-hour date and time for a row, e.g. "18 Nov, 09:12". */
export const whenText = (iso: string) =>
	new Date(iso).toLocaleString(undefined, {
		day: 'numeric',
		month: 'short',
		hour: '2-digit',
		minute: '2-digit',
		hourCycle: 'h23'
	});

export const usd = (n: number) => `$${n.toFixed(2)}`;

export const tokens = (n: number | undefined) =>
	n === undefined ? 'unknown' : n.toLocaleString('en-US');
