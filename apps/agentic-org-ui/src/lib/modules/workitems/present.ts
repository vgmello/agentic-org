// Words, order and grouping for the Work screens (work-items-ux §2.1, §6, §9, §11; costs-and-insights-ux "Shared
// duration definitions"). Pure: the screens only arrange what these return.
import type {
	Priority,
	Resolution,
	Severity,
	TaskLink,
	TaskRow,
	TaskStatus,
	TaskTime,
	TaskType
} from './types';

export const WORK_URL = '/work';
export const taskHref = (ref: string) => `/work/${encodeURIComponent(ref)}`;
export const inboxItemHref = (id: string) => `/inbox?item=${encodeURIComponent(id)}`;
export const projectHref = (id: string) => `/projects/${encodeURIComponent(id)}`;
export const teamHref = (id: string) => `/teams/${encodeURIComponent(id)}`;
export const memberHref = (id: string) => `/members/${encodeURIComponent(id)}`;

/** "T-A42" or "T-14": a complete reference on its own, in any letter case typed into the address. */
export const REF_PATTERN = /^T-[A-Z]{0,3}\d+$/;
export const normalizeRef = (ref: string) => ref.trim().toUpperCase();
export const isRef = (ref: string) => REF_PATTERN.test(normalizeRef(ref));

export const TYPE_WORD: Record<TaskType, string> = { task: 'Task', bug: 'Bug', request: 'Request' };
export const TYPE_PLURAL: Record<TaskType, string> = {
	task: 'Tasks',
	bug: 'Bugs',
	request: 'Requests'
};

export const STATUS_WORD: Record<TaskStatus, string> = {
	todo: 'To do',
	in_progress: 'In progress',
	blocked: 'Blocked',
	done: 'Done',
	cancelled: 'Cancelled'
};
export const STATUSES: TaskStatus[] = ['todo', 'in_progress', 'blocked', 'done', 'cancelled'];
/** Blocked-first order when grouping lists by status (Prototype Release 3c4e3541). */
export const STATUS_GROUP_ORDER: TaskStatus[] = [
	'blocked',
	'todo',
	'in_progress',
	'done',
	'cancelled'
];
export const isOpen = (s: TaskStatus) => s !== 'done' && s !== 'cancelled';

export const PRIORITY_WORD: Record<Priority, string> = {
	urgent: 'Urgent',
	high: 'High',
	normal: 'Normal',
	low: 'Low'
};
export const PRIORITIES: Priority[] = ['urgent', 'high', 'normal', 'low'];
const PRIORITY_RANK: Record<Priority, number> = { urgent: 0, high: 1, normal: 2, low: 3 };

export const SEVERITY_WORD: Record<Severity, string> = {
	critical: 'Critical',
	major: 'Major',
	minor: 'Minor',
	trivial: 'Trivial'
};
export const SEVERITIES: Severity[] = ['critical', 'major', 'minor', 'trivial'];
const SEVERITY_RANK: Record<Severity, number> = { critical: 0, major: 1, minor: 2, trivial: 3 };
export const severityWord = (s: Severity | null) => (s ? SEVERITY_WORD[s] : 'Not set');

/** The report form's explanation of each level (§2.1). */
export const SEVERITY_HELP: Record<Severity, string> = {
	critical: 'Data loss, money or security, or nobody can work',
	major: 'A main feature is broken with no good workaround',
	minor: 'A problem with a workaround',
	trivial: 'Cosmetic: wording or spacing'
};

export const RESOLUTION_WORD: Record<Resolution, string> = {
	fixed: 'Fixed',
	duplicate: 'Duplicate',
	wont_fix: "Won't fix",
	cant_reproduce: "Can't reproduce",
	completed: 'Completed',
	cancelled: 'Cancelled'
};

/** "Bug T-A42": the type word and the reference together (WORK-4). */
export const typedRef = (t: { type: TaskType; ref: string }) => `${TYPE_WORD[t.type]} ${t.ref}`;
export const linkText = (t: TaskLink) => `${typedRef(t)} ${t.title}`;

/** Every column sorts by priority (Urgent first), then age, oldest first (TSK-4). */
export function byPriorityThenAge(a: TaskRow, b: TaskRow): number {
	return (
		PRIORITY_RANK[a.priority] - PRIORITY_RANK[b.priority] ||
		a.createdAt.localeCompare(b.createdAt) ||
		a.ref.localeCompare(b.ref)
	);
}

/** Bugs sort by severity (Not set after Trivial), then priority, then age (§6). */
export function bySeverityThenPriority(a: TaskRow, b: TaskRow): number {
	const rank = (s: Severity | null) => (s ? SEVERITY_RANK[s] : 4);
	return rank(a.severity) - rank(b.severity) || byPriorityThenAge(a, b);
}

export type WorkSortBy = 'default' | 'title' | 'priority' | 'created';
export type WorkSortDir = 'asc' | 'desc';

export const WORK_SORT_OPTIONS: { id: WorkSortBy; label: string }[] = [
	{ id: 'default', label: 'Default' },
	{ id: 'priority', label: 'Priority' },
	{ id: 'title', label: 'Title' },
	{ id: 'created', label: 'Created' }
];

/** Comparator for the Work list Display menu (Sort by + Direction). */
export function workRowComparator(
	tab: TaskType | 'all',
	sortBy: WorkSortBy,
	sortDir: WorkSortDir
): (a: TaskRow, b: TaskRow) => number {
	const base = tab === 'bug' ? bySeverityThenPriority : byPriorityThenAge;
	const factor = sortDir === 'desc' ? -1 : 1;
	if (sortBy === 'default') return sortDir === 'desc' ? (a, b) => -base(a, b) : base;
	return (a, b) => {
		const cmp =
			sortBy === 'title'
				? a.title.localeCompare(b.title)
				: sortBy === 'priority'
					? PRIORITY_RANK[a.priority] - PRIORITY_RANK[b.priority]
					: a.createdAt.localeCompare(b.createdAt);
		return cmp !== 0 ? cmp * factor : base(a, b);
	};
}

// ---- Board ---------------------------------------------------------------------------------------------------------

export type BoardColumn = 'todo' | 'in_progress' | 'blocked' | 'done';
export type BoardStatusColumn =
	'todo' | 'in_progress' | 'blocked' | 'awaiting_verification' | 'done' | 'cancelled';
export type BoardMode = 'stages' | 'statuses';
export const BOARD_COLUMNS: BoardColumn[] = ['todo', 'in_progress', 'blocked', 'done'];
export const BOARD_STATUS_COLUMNS: BoardStatusColumn[] = [
	'todo',
	'in_progress',
	'blocked',
	'awaiting_verification',
	'done',
	'cancelled'
];
export const BOARD_COLUMN_WORD: Record<BoardStatusColumn, string> = {
	...STATUS_WORD,
	awaiting_verification: 'Awaiting verification'
};
export type DoneWindow = '7d' | '30d' | 'all';
export const DONE_WINDOW_LABELS: Record<DoneWindow, string> = {
	'7d': 'Last 7 days',
	'30d': 'Last 30 days',
	all: 'All done'
};
const WEEK_MS = 7 * 24 * 3600 * 1000;
const MONTH_MS = 30 * 24 * 3600 * 1000;

function filterDoneByWindow(
	inColumn: TaskRow[],
	opts: { allDone?: boolean; doneWindow?: DoneWindow; now: Date }
): TaskRow[] {
	const windowMode: DoneWindow = opts.doneWindow ?? (opts.allDone ? 'all' : '7d');
	if (windowMode === 'all') return inColumn;
	const maxAgeMs = windowMode === '30d' ? MONTH_MS : WEEK_MS;
	return inColumn.filter((r) => opts.now.getTime() - new Date(r.statusAt).getTime() <= maxAgeMs);
}

/**
 * The board's columns (TSK-1): To do, In progress, Blocked and Done. Done keeps the last seven days unless
 * 30 days or all are asked for; Cancelled is not a board column in the default 4-stage view.
 */
export function boardColumns(
	rows: TaskRow[],
	opts: { allDone?: boolean; doneWindow?: DoneWindow; now: Date }
): { column: BoardColumn; rows: TaskRow[]; hiddenDone: number }[] {
	return BOARD_COLUMNS.map((column) => {
		const inColumn = rows.filter((r) => r.status === column);
		const shown = column === 'done' ? filterDoneByWindow(inColumn, opts) : inColumn;
		return {
			column,
			rows: [...shown].sort(byPriorityThenAge),
			hiddenDone: inColumn.length - shown.length
		};
	});
}

/**
 * Board columns supporting both the default 4-stage board (`stages`) and the expanded 6-status board (`statuses`)
 * from Prototype Release 3c4e3541 (`Awaiting verification` + `Cancelled` columns).
 */
export function boardColumnsForMode(
	rows: TaskRow[],
	mode: BoardMode,
	opts: { allDone?: boolean; doneWindow?: DoneWindow; now: Date }
): {
	column: BoardStatusColumn;
	rows: TaskRow[];
	hiddenDone: number;
	writableStatus: BoardColumn | null;
}[] {
	if (mode === 'stages') {
		return boardColumns(rows, opts).map((c) => ({ ...c, writableStatus: c.column }));
	}
	return BOARD_STATUS_COLUMNS.map((column) => {
		const inColumn = rows.filter((r) => {
			if (column === 'awaiting_verification')
				return Boolean(r.awaitingVerification) && isOpen(r.status);
			if (r.awaitingVerification && isOpen(r.status)) return false;
			return r.status === column;
		});
		const shown = column === 'done' ? filterDoneByWindow(inColumn, opts) : inColumn;
		const writableStatus: BoardColumn | null =
			column === 'todo' || column === 'in_progress' || column === 'blocked' || column === 'done'
				? column
				: null;
		return {
			column,
			rows: [...shown].sort(byPriorityThenAge),
			hiddenDone: inColumn.length - shown.length,
			writableStatus
		};
	});
}

/** The next column left or right of a status, for Alt+← and Alt+→ on a card. */
export function neighbourColumn(status: TaskStatus, step: -1 | 1): BoardColumn | undefined {
	const at = BOARD_COLUMNS.indexOf(status as BoardColumn);
	if (at < 0) return undefined;
	return BOARD_COLUMNS[at + step];
}

// ---- Grouping ------------------------------------------------------------------------------------------------------

export type GroupBy =
	'assigned_team' | 'severity' | 'priority' | 'status' | 'team' | 'assignee' | 'project' | 'none';

export const GROUP_WORD: Record<GroupBy, string> = {
	assigned_team: 'Assigned team',
	severity: 'Severity',
	priority: 'Priority',
	status: 'Status',
	team: 'Accountable team',
	assignee: 'Assignee',
	project: 'Project',
	none: 'None'
};

export interface Group {
	key: string;
	label: string;
	rows: TaskRow[];
}

/** Groups in a fixed, explicit order; "Unassigned team", "No project" and "Not set" are groups of their own. */
export function groupRows(rows: TaskRow[], by: GroupBy, sort = byPriorityThenAge): Group[] {
	if (by === 'none') return [{ key: 'all', label: 'All', rows: [...rows].sort(sort) }];
	const keyOf = (r: TaskRow): [string, string] => {
		switch (by) {
			case 'assigned_team':
				return r.assignedTeam ? [r.assignedTeam.id, r.assignedTeam.name] : ['', 'Unassigned team'];
			case 'team':
				return [r.team.id, r.team.name];
			case 'assignee':
				return r.assignee ? [r.assignee.id, r.assignee.name] : ['', 'Unassigned'];
			case 'project':
				return r.project ? [r.project.id, r.project.name] : ['', 'No project'];
			case 'severity':
				return [r.severity ?? 'unset', severityWord(r.severity)];
			case 'priority':
				return [r.priority, PRIORITY_WORD[r.priority]];
			case 'status':
				return [r.status, STATUS_WORD[r.status]];
		}
	};
	const groups = new Map<string, Group>();
	for (const r of rows) {
		const [key, label] = keyOf(r);
		const g = groups.get(key) ?? { key, label, rows: [] };
		g.rows.push(r);
		groups.set(key, g);
	}
	const fixedOrder: Partial<Record<GroupBy, string[]>> = {
		severity: [...SEVERITIES, 'unset'],
		priority: PRIORITIES,
		status: STATUS_GROUP_ORDER
	};
	const order = fixedOrder[by];
	const list = [...groups.values()].map((g) => ({ ...g, rows: g.rows.sort(sort) }));
	return list.sort((a, b) => {
		if (order) return order.indexOf(a.key) - order.indexOf(b.key);
		// The explicit "none" group (empty key) goes last; the rest A–Z.
		if (!a.key !== !b.key) return a.key ? -1 : 1;
		return a.label.localeCompare(b.label);
	});
}

/** "1 critical · 2 major · 1 not set": open bugs by severity in words, including unset (BUG-11). */
export function severityCounts(rows: TaskRow[]): string {
	const open = rows.filter((r) => isOpen(r.status));
	const parts = [...SEVERITIES, null].flatMap((s) => {
		const n = open.filter((r) => r.severity === s).length;
		return n ? [`${n} ${severityWord(s).toLowerCase()}`] : [];
	});
	return parts.length ? parts.join(' · ') : 'No open bugs';
}

// ---- Time ----------------------------------------------------------------------------------------------------------

/**
 * The shared aggregate duration (costs "Shared duration definitions"): whole minutes below 1 h, whole hours below
 * 48 h, then days with one decimal, floored. Zero still has its unit.
 */
export function formatDuration(seconds: number): string {
	const s = Math.max(0, Math.floor(seconds));
	if (s < 3600) return `${Math.floor(s / 60)} min`;
	if (s < 48 * 3600) return `${Math.floor(s / 3600)} h`;
	return `${(Math.floor((s * 10) / 86400) / 10).toFixed(1)} d`;
}

/** The precise value for assistive technology and a title: "1 day 4 hours 3 minutes". */
export function preciseDuration(seconds: number): string {
	const s = Math.max(0, Math.floor(seconds));
	const d = Math.floor(s / 86400);
	const h = Math.floor((s % 86400) / 3600);
	const m = Math.floor((s % 3600) / 60);
	const part = (n: number, unit: string) => `${n} ${unit}${n === 1 ? '' : 's'}`;
	const parts = [d ? part(d, 'day') : '', h ? part(h, 'hour') : '', part(m, 'minute')];
	return parts.filter(Boolean).join(' ');
}

/**
 * The lead time split (§9.2, TIME-1): waiting on you, blocked and the rest, which adds up to the lead time. Working
 * time isn't tracked yet, so the rest is never called idle.
 */
export function timeSplit(time: TaskTime): {
	lead: string;
	parts: { label: string; value: string; precise: string }[];
	soFar: boolean;
} | null {
	if (time.leadSeconds === null) return null;
	const rest = Math.max(0, time.leadSeconds - time.waitingOnYouSeconds - time.blockedSeconds);
	const part = (label: string, seconds: number) => ({
		label,
		value: formatDuration(seconds),
		precise: preciseDuration(seconds)
	});
	return {
		lead: formatDuration(time.leadSeconds),
		soFar: time.open,
		parts: [
			part('waiting on you', time.waitingOnYouSeconds),
			part('blocked', time.blockedSeconds),
			part('the rest', rest)
		]
	};
}

/** Relative within 24 hours, absolute after (app-shell "Account time display"). */
export function when(at: string, now = new Date()): string {
	const t = new Date(at);
	const diff = now.getTime() - t.getTime();
	if (diff >= 0 && diff < 24 * 3600 * 1000) {
		const min = Math.floor(diff / 60000);
		if (min < 1) return 'just now';
		if (min < 60) return `${min} min ago`;
		return `${Math.floor(min / 60)} h ago`;
	}
	return t.toLocaleString('en-GB', {
		day: 'numeric',
		month: 'short',
		year: t.getFullYear() === now.getFullYear() ? undefined : 'numeric',
		hour: '2-digit',
		minute: '2-digit',
		hour12: false
	});
}

/** "3 d" since a time, for a row's age. */
export const ageOf = (at: string, now = new Date()) =>
	formatDuration((now.getTime() - new Date(at).getTime()) / 1000);

// ---- Assignment ----------------------------------------------------------------------------------------------------

/** "Platform · Ana", "Platform", "Ana" or "Unassigned": the pair in words, never inferred (§5). */
export function assignmentText(r: Pick<TaskRow, 'assignedTeam' | 'assignee'>): string {
	const parts = [r.assignedTeam?.name, r.assignee?.name].filter(Boolean);
	return parts.length ? parts.join(' · ') : 'Unassigned';
}

/**
 * The screen reader's row (work-items §13): "Bug T-A42, Prevent overlapping reservations, severity major, priority
 * high, accountable Payments, assigned team Platform, assignee Ana, person."
 */
export function rowSpeech(r: TaskRow): string {
	const parts = [typedRef(r), r.title];
	if (r.type === 'bug') parts.push(`severity ${severityWord(r.severity).toLowerCase()}`);
	parts.push(`priority ${PRIORITY_WORD[r.priority].toLowerCase()}`);
	parts.push(`status ${STATUS_WORD[r.status].toLowerCase()}`);
	parts.push(`accountable ${r.team.name}`);
	parts.push(r.assignedTeam ? `assigned team ${r.assignedTeam.name}` : 'no assigned team');
	parts.push(r.assignee ? `assignee ${r.assignee.name}, ${r.assignee.kind}` : 'no assignee');
	return parts.join(', ');
}

// ---- Checklist -----------------------------------------------------------------------------------------------------

export const STEP_MAX = 200;

/** Pasted or typed text becomes steps: one per non-empty line (CHK-2). */
export const splitSteps = (text: string) =>
	text
		.split(/\r?\n/)
		.map((l) => l.trim())
		.filter(Boolean);

/** Why a step's text is refused, or undefined when it is fine (1–200 characters each). */
export function stepError(lines: string[]): string | undefined {
	if (lines.length === 0) return 'Write the step first.';
	const long = lines.findIndex((l) => l.length > STEP_MAX);
	if (long >= 0)
		return lines.length === 1
			? `A step is at most ${STEP_MAX} characters; this one has ${lines[0].length}.`
			: `Line ${long + 1} has ${lines[long].length} characters; a step is at most ${STEP_MAX}.`;
	return undefined;
}

/** Moves one item of a list to a new index, for reordering. */
export function moved<T>(list: T[], from: number, to: number): T[] {
	const next = [...list];
	const [item] = next.splice(from, 1);
	next.splice(Math.max(0, Math.min(to, next.length)), 0, item);
	return next;
}

// ---- Capture -------------------------------------------------------------------------------------------------------

export const TITLE_MAX = 120;

export function titleError(title: string): string | undefined {
	const t = title.trim();
	if (!t) return 'Write a title.';
	if (t.length > TITLE_MAX)
		return `A title is at most ${TITLE_MAX} characters; this one has ${t.length}.`;
	return undefined;
}

/** Known activity event kinds; anything else renders as "Unknown change" (§7.1). */
export const KNOWN_EVENT_KINDS = new Set([
	'created',
	'edited',
	'status',
	'priority',
	'assigned',
	'triaged',
	'step',
	'comment',
	'fixed',
	'verified',
	'closed',
	'reopened',
	'scoped',
	'approved',
	'changes_asked',
	'declined',
	'merged',
	'budget',
	'linked'
]);

export function activityKindLabel(kind: string): string {
	return KNOWN_EVENT_KINDS.has(kind) ? kind : 'unknown';
}
