// Pure, strictly reconciled derivation for Team Overview (Prototype Release 1149b4f1).
// Uses real authorized team assignments, project progress/health, activity, and optional completion
// transition records. Never fabricates timing medians, completion timestamps, open counts, or attribution.
import {
	countedTasks,
	doneText,
	progressPercent,
	projectBarTone,
	projectHealthLabel,
	sortActivityNewestFirst,
	type ProjectBarTone
} from '$lib/modules/projects/present';
import type { ProjectRow } from '$lib/modules/projects/types';
import type { ActivityEntry, WorkRow, WorkState } from '$lib/modules/members';
import type { TeamDetail, TeamProjectRow } from '../../types';

export const TEAM_OVERVIEW_WINDOW_DAYS = 28;
export const TEAM_OVERVIEW_MIN_TIMING_SAMPLES = 5;

const DAY_MS = 86_400_000;
const UTC_DAY_FORMATTER = new Intl.DateTimeFormat('en-US', {
	month: 'short',
	day: 'numeric',
	timeZone: 'UTC'
});

/** Authorized completion transition record when served by the backend. */
export interface TeamCompletionRecord {
	/** RFC 3339 completion transition timestamp (must be a real completion event, not mutable updated_at). */
	completedAt: string;
	kind: 'task' | 'bug';
	contributorId?: string;
	contributorName?: string;
	/** Recorded start-to-completion cycle duration in seconds; null when unavailable. */
	cycleSeconds?: number | null;
	/** Recorded waiting-on-person approval duration in seconds; null when unavailable. */
	approvalWaitSeconds?: number | null;
}

export interface TeamOverviewKpi {
	id: 'completed' | 'cycle_time' | 'approval_wait' | 'blocked';
	label: string;
	value: string;
	note: string;
}

export interface TeamWeeklyBucket {
	label: string;
	tasks: number;
	bugs: number;
	total: number;
}

export interface TeamStatusCount {
	state: WorkState;
	label: string;
	count: number;
}

export interface TeamActiveProjectSummary {
	id: string;
	name: string;
	key?: string;
	role: 'owner' | 'contributor';
	openWork?: number;
	/** Null when real project health counts are not served by this server. */
	healthLabel: string | null;
	/** Null when real project health counts are not served by this server. */
	tone: ProjectBarTone | null;
	progressPct: number | null;
	progressText: string;
	doneTasks: number;
	totalTasks: number;
}

export interface TeamContributorRow {
	key: string;
	memberId?: string;
	name: string;
	completed: number | null;
	open: number;
	blocked: number;
	/** True when the server supplied only a display name without a stable assignee/contributor ID. */
	unresolvedAttribution: boolean;
}

export interface TeamOverviewModel {
	windowLabel: string;
	kpis: TeamOverviewKpi[];
	workAvailable: boolean;
	completionsAvailable: boolean;
	completedTotal: number | null;
	weekly: TeamWeeklyBucket[];
	weeklyMax: number;
	openTotal: number | null;
	blockedTotal: number | null;
	openByStatus: TeamStatusCount[];
	openStatusMax: number;
	activeProjects: TeamActiveProjectSummary[];
	contributors: TeamContributorRow[];
	unassignedOpen: number;
	unassignedBlocked: number;
	unresolvedAttributionOpen: number;
	unattributedCompletions: number;
	recentActivity: ActivityEntry[];
}

export interface TeamOverviewInput {
	detail: TeamDetail;
	/** Team work rows in the same scope as the team's Work tab; undefined while loading, failed, or unavailable. */
	workRows?: readonly WorkRow[];
	/** Resource state for `workRows`, used to surface honest unavailability notes on the Blocked KPI. */
	workState?: 'loading' | 'unavailable' | 'unsupported' | 'error' | 'ready';
	/** Active projects owned or contributed to by this team. */
	projects?: readonly TeamProjectRow[];
	/** Optional full project summaries (when an authorized read supplies them) for real progress & health. */
	projectDetails?: readonly ProjectRow[];
	/** Team activity entries. */
	activity?: readonly ActivityEntry[];
	/** Optional authorized completion transition records; null/undefined means completion transitions are not served. */
	completions?: readonly TeamCompletionRecord[] | null;
	/** Optional org-wide member lookup by stable ID for resolving display names of cross-team assignees. */
	memberNameById?: ReadonlyMap<string, string> | Readonly<Record<string, string>>;
	/** Reference timestamp in ms (defaults to Date.now()). */
	nowMs?: number;
}

const OPEN_STATUS_ORDER: readonly { state: Exclude<WorkState, 'done'>; label: string }[] = [
	{ state: 'blocked', label: 'Blocked' },
	{ state: 'in_progress', label: 'In progress' },
	{ state: 'in_review', label: 'In review' },
	{ state: 'todo', label: 'To do' }
];

function startOfUtcDay(ms: number): number {
	const d = new Date(ms);
	return Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate());
}

export function formatUtcDayLabel(ms: number): string {
	return UTC_DAY_FORMATTER.format(new Date(ms));
}

export function computeMedian(values: readonly number[]): number | null {
	const clean = values.filter((v) => Number.isFinite(v) && v >= 0).toSorted((a, b) => a - b);
	if (clean.length === 0) return null;
	const mid = Math.floor(clean.length / 2);
	return clean.length % 2 === 1 ? clean[mid] : (clean[mid - 1] + clean[mid]) / 2;
}

export function formatDurationSeconds(seconds: number): string {
	if (seconds < 60) return '<1m';
	const totalMinutes = Math.round(seconds / 60);
	if (totalMinutes < 60) return `${totalMinutes}m`;
	const hours = totalMinutes / 60;
	if (hours < 48) {
		const rounded = Math.round(hours * 10) / 10;
		return Number.isInteger(rounded) ? `${rounded}h` : `${rounded.toFixed(1)}h`;
	}
	const days = Math.round((hours / 24) * 10) / 10;
	return Number.isInteger(days) ? `${days}d` : `${days.toFixed(1)}d`;
}

function plural(n: number, one: string, many: string): string {
	return `${n} ${n === 1 ? one : many}`;
}

export function deriveTeamOverview(input: TeamOverviewInput): TeamOverviewModel {
	const nowMs = input.nowMs ?? Date.now();
	const endDayMs = startOfUtcDay(nowMs);
	const startDayMs = endDayMs - (TEAM_OVERVIEW_WINDOW_DAYS - 1) * DAY_MS;
	const endExclusiveMs = endDayMs + DAY_MS;
	const windowLabel = `${formatUtcDayLabel(startDayMs)}–${formatUtcDayLabel(endDayMs)}`;

	const workAvailable = Array.isArray(input.workRows);
	const workRows: readonly WorkRow[] = Array.isArray(input.workRows) ? input.workRows : [];
	const openRows = workRows.filter((row) => row.state !== 'done');
	const openTotal = workAvailable ? openRows.length : null;
	const blockedTotal = workAvailable
		? openRows.filter((row) => row.state === 'blocked').length
		: null;

	const openByStatus: TeamStatusCount[] = [];
	if (workAvailable) {
		for (const entry of OPEN_STATUS_ORDER) {
			const count = openRows.filter((row) => row.state === entry.state).length;
			if (count > 0) {
				openByStatus.push({ state: entry.state, label: entry.label, count });
			}
		}
	}
	const openStatusMax = openByStatus.reduce((max, row) => Math.max(max, row.count), 0);

	// Build 4 x 7-day UTC buckets covering the exact 28-day window.
	const completionsAvailable = Array.isArray(input.completions);
	const inWindowCompletions = completionsAvailable
		? (input.completions ?? []).filter((rec) => {
				const ts = Date.parse(rec.completedAt);
				return Number.isFinite(ts) && ts >= startDayMs && ts < endExclusiveMs;
			})
		: [];

	const weekly: TeamWeeklyBucket[] = [];
	for (let i = 0; i < 4; i++) {
		const bucketStart = startDayMs + i * 7 * DAY_MS;
		const bucketLast = Math.min(endDayMs, bucketStart + 6 * DAY_MS);
		const bucketEndExclusive = bucketLast + DAY_MS;
		let tasks = 0;
		let bugs = 0;
		for (const rec of inWindowCompletions) {
			const ts = Date.parse(rec.completedAt);
			if (ts >= bucketStart && ts < bucketEndExclusive) {
				if (rec.kind === 'bug') bugs += 1;
				else tasks += 1;
			}
		}
		weekly.push({
			label: `${formatUtcDayLabel(bucketStart)}–${formatUtcDayLabel(bucketLast)}`,
			tasks,
			bugs,
			total: tasks + bugs
		});
	}
	const weeklyMax = weekly.reduce((max, b) => Math.max(max, b.total), 0);
	const completedTotal = completionsAvailable ? inWindowCompletions.length : null;

	// Timing medians require >= 5 completions with real recorded durations.
	const cycleSamples = inWindowCompletions
		.map((r) => r.cycleSeconds)
		.filter((v): v is number => typeof v === 'number' && Number.isFinite(v) && v >= 0);
	const waitSamples = inWindowCompletions
		.map((r) => r.approvalWaitSeconds)
		.filter((v): v is number => typeof v === 'number' && Number.isFinite(v) && v >= 0);

	const enoughCycle =
		completionsAvailable &&
		inWindowCompletions.length >= TEAM_OVERVIEW_MIN_TIMING_SAMPLES &&
		cycleSamples.length >= TEAM_OVERVIEW_MIN_TIMING_SAMPLES;
	const enoughWait =
		completionsAvailable &&
		inWindowCompletions.length >= TEAM_OVERVIEW_MIN_TIMING_SAMPLES &&
		waitSamples.length >= TEAM_OVERVIEW_MIN_TIMING_SAMPLES;

	const cycleMedian = enoughCycle ? computeMedian(cycleSamples) : null;
	const waitMedian = enoughWait ? computeMedian(waitSamples) : null;

	const cycleNote = !completionsAvailable
		? 'Completion transitions not served yet'
		: inWindowCompletions.length < TEAM_OVERVIEW_MIN_TIMING_SAMPLES
			? `At least ${TEAM_OVERVIEW_MIN_TIMING_SAMPLES} completions needed`
			: cycleMedian === null
				? 'Cycle time not recorded yet'
				: 'From start to completion';

	const waitNote = !completionsAvailable
		? 'Approval wait not served yet'
		: inWindowCompletions.length < TEAM_OVERVIEW_MIN_TIMING_SAMPLES
			? `At least ${TEAM_OVERVIEW_MIN_TIMING_SAMPLES} completions needed`
			: waitMedian === null
				? 'Approval wait not recorded yet'
				: 'Time waiting on you per completed item';

	const blockedNote = workAvailable
		? `Of ${plural(openTotal!, 'open item', 'open items')}`
		: input.workState === 'loading'
			? 'Loading open work…'
			: input.workState === 'error'
				? 'Open work unavailable'
				: 'Open work not served yet';

	const kpis: TeamOverviewKpi[] = [
		{
			id: 'completed',
			label: 'Completed',
			value: completedTotal === null ? '—' : String(completedTotal),
			note: completionsAvailable
				? `Tasks and bugs in the last ${TEAM_OVERVIEW_WINDOW_DAYS} days`
				: 'Completion transitions not served yet'
		},
		{
			id: 'cycle_time',
			label: 'Median cycle time',
			value: cycleMedian === null ? '—' : formatDurationSeconds(cycleMedian),
			note: cycleNote
		},
		{
			id: 'approval_wait',
			label: 'Median approval wait',
			value: waitMedian === null ? '—' : formatDurationSeconds(waitMedian),
			note: waitNote
		},
		{
			id: 'blocked',
			label: 'Blocked now',
			value: blockedTotal === null ? '—' : String(blockedTotal),
			note: blockedNote
		}
	];

	// Active projects: task progress requires served task counts; health labels require BOTH task and bug counts
	// so partial data never claims "On track" by substituting zero bugs.
	const projectMap = new Map<string, ProjectRow>();
	for (const p of input.projectDetails ?? []) {
		projectMap.set(p.id, p);
	}
	const activeProjects: TeamActiveProjectSummary[] = (input.projects ?? [])
		.filter((proj) => !projectMap.get(proj.id)?.archived)
		.map((proj) => {
			const detail = projectMap.get(proj.id);
			const tasks = detail?.tasks ?? proj.tasks;
			const bugs = detail?.bugs ?? proj.bugs;
			const archived = detail?.archived ?? false;
			if (tasks) {
				const totalTasks = countedTasks(tasks);
				const tone = bugs ? projectBarTone(tasks, bugs, archived) : null;
				return {
					id: proj.id,
					name: proj.name,
					key: proj.key,
					role: proj.role,
					openWork: proj.openWork,
					healthLabel: tone ? projectHealthLabel(tone) : null,
					tone,
					progressPct: totalTasks > 0 ? progressPercent(tasks) : null,
					progressText: doneText(tasks),
					doneTasks: tasks.done,
					totalTasks
				};
			}
			return {
				id: proj.id,
				name: proj.name,
				key: proj.key,
				role: proj.role,
				openWork: proj.openWork,
				healthLabel: null,
				tone: null,
				progressPct: null,
				progressText:
					typeof proj.openWork === 'number'
						? proj.openWork === 0
							? 'No open tasks · progress & health unavailable'
							: `${plural(proj.openWork, 'open task', 'open tasks')} · progress & health unavailable`
						: "Progress and health aren't served by this server yet.",
				doneTasks: 0,
				totalTasks: 0
			};
		});

	// Contributors table: strictly keyed by stable ID in a Map (zero prototype collisions;
	// never collapses two people with the same name or guesses a link from name-only rows).
	const memberNameMap = new Map<string, string>();
	for (const m of input.detail.members) {
		memberNameMap.set(m.id, m.name);
	}
	if (input.memberNameById instanceof Map) {
		for (const [id, name] of input.memberNameById) {
			if (!memberNameMap.has(id)) memberNameMap.set(id, name);
		}
	} else if (input.memberNameById) {
		for (const [id, name] of Object.entries(input.memberNameById)) {
			if (!memberNameMap.has(id)) memberNameMap.set(id, name);
		}
	}

	const bucketMap = new Map<string, TeamContributorRow>();
	function ensureContributor(
		memberId: string | undefined,
		fallbackName: string
	): TeamContributorRow {
		const trimmedId = memberId?.trim() || undefined;
		const trimmedName = fallbackName.trim();
		if (trimmedId) {
			const key = `id:${trimmedId}`;
			let entry = bucketMap.get(key);
			if (!entry) {
				const displayName = memberNameMap.get(trimmedId) ?? (trimmedName || trimmedId);
				entry = {
					key,
					memberId: trimmedId,
					name: displayName,
					completed: completionsAvailable ? 0 : null,
					open: 0,
					blocked: 0,
					unresolvedAttribution: false
				};
				bucketMap.set(key, entry);
			}
			return entry;
		}
		const key = `unresolved:${trimmedName.toLowerCase()}`;
		let entry = bucketMap.get(key);
		if (!entry) {
			entry = {
				key,
				memberId: undefined,
				name: trimmedName,
				completed: completionsAvailable ? 0 : null,
				open: 0,
				blocked: 0,
				unresolvedAttribution: true
			};
			bucketMap.set(key, entry);
		}
		return entry;
	}

	let unassignedOpen = 0;
	let unassignedBlocked = 0;
	let unresolvedAttributionOpen = 0;
	for (const row of openRows) {
		const id = row.assigneeId?.trim() || undefined;
		const name = row.assignee?.trim() || '';
		if (!id && !name) {
			unassignedOpen += 1;
			if (row.state === 'blocked') unassignedBlocked += 1;
			continue;
		}
		const entry = ensureContributor(id, name || id!);
		entry.open += 1;
		if (row.state === 'blocked') entry.blocked += 1;
		if (!id) unresolvedAttributionOpen += 1;
	}

	let unattributedCompletions = 0;
	if (completionsAvailable) {
		for (const rec of inWindowCompletions) {
			const id = rec.contributorId?.trim() || undefined;
			const name = rec.contributorName?.trim() || (id ? (memberNameMap.get(id) ?? '') : '');
			if (!id && !name) {
				unattributedCompletions += 1;
				continue;
			}
			const entry = ensureContributor(id, name || id!);
			entry.completed = (entry.completed ?? 0) + 1;
		}
	}

	const contributors = [...bucketMap.values()]
		.filter((row) => row.open > 0 || (row.completed !== null && row.completed > 0))
		.toSorted(
			(a, b) =>
				a.name.localeCompare(b.name, 'en', { sensitivity: 'base' }) || a.key.localeCompare(b.key)
		);

	const recentActivity = sortActivityNewestFirst(input.activity ?? []).slice(0, 3);

	return {
		windowLabel,
		kpis,
		workAvailable,
		completionsAvailable,
		completedTotal,
		weekly,
		weeklyMax,
		openTotal,
		blockedTotal,
		openByStatus,
		openStatusMax,
		activeProjects,
		contributors,
		unassignedOpen,
		unassignedBlocked,
		unresolvedAttributionOpen,
		unattributedCompletions,
		recentActivity
	};
}
