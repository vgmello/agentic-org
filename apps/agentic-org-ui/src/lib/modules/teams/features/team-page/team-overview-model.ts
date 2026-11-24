// Pure, strictly reconciled derivation for Team Overview (Prototype Release 1149b4f1).
// Uses real authorized team assignments, project progress/health, activity, and optional completion
// transition records. Never fabricates timing medians, completion timestamps, or attribution.
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
	openWork: number;
	healthLabel: string;
	tone: ProjectBarTone;
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
}

export interface TeamOverviewModel {
	windowLabel: string;
	kpis: TeamOverviewKpi[];
	completionsAvailable: boolean;
	completedTotal: number | null;
	weekly: TeamWeeklyBucket[];
	weeklyMax: number;
	openTotal: number;
	blockedTotal: number;
	openByStatus: TeamStatusCount[];
	openStatusMax: number;
	activeProjects: TeamActiveProjectSummary[];
	contributors: TeamContributorRow[];
	unassignedOpen: number;
	unassignedBlocked: number;
	unattributedCompletions: number;
	recentActivity: ActivityEntry[];
}

export interface TeamOverviewInput {
	detail: TeamDetail;
	/** Team work rows in the same scope as the team's Work tab; undefined while loading/unavailable. */
	workRows?: readonly WorkRow[];
	/** Active projects owned or contributed to by this team. */
	projects?: readonly TeamProjectRow[];
	/** Optional full project summaries (from projectsApi.list) for real progress & health. */
	projectDetails?: readonly ProjectRow[];
	/** Team activity entries. */
	activity?: readonly ActivityEntry[];
	/** Optional authorized completion transition records; null/undefined means completion transitions are not served. */
	completions?: readonly TeamCompletionRecord[] | null;
	/** Optional org-wide member lookup for linking cross-team assignees by name. */
	memberIdByName?: Readonly<Record<string, string>>;
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

	const workRows = input.workRows ?? [];
	const openRows = workRows.filter((row) => row.state !== 'done');
	const openTotal = openRows.length;
	const blockedTotal = openRows.filter((row) => row.state === 'blocked').length;

	const openByStatus: TeamStatusCount[] = [];
	for (const entry of OPEN_STATUS_ORDER) {
		const count = openRows.filter((row) => row.state === entry.state).length;
		if (count > 0) {
			openByStatus.push({ state: entry.state, label: entry.label, count });
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
			value: String(blockedTotal),
			note: `Of ${plural(openTotal, 'open item', 'open items')}`
		}
	];

	// Active projects with real health & progress when projectDetails are served.
	const projectMap: Record<string, ProjectRow> = {};
	for (const p of input.projectDetails ?? []) {
		projectMap[p.id] = p;
	}
	const activeProjects: TeamActiveProjectSummary[] = (input.projects ?? [])
		.filter((proj) => !projectMap[proj.id]?.archived)
		.map((proj) => {
			const detail = projectMap[proj.id];
			if (detail) {
				const totalTasks = countedTasks(detail.tasks);
				const tone = projectBarTone(detail.tasks, detail.bugs, detail.archived);
				return {
					id: proj.id,
					name: proj.name,
					key: proj.key,
					role: proj.role,
					openWork: proj.openWork,
					healthLabel: projectHealthLabel(tone),
					tone,
					progressPct: totalTasks > 0 ? progressPercent(detail.tasks) : null,
					progressText: doneText(detail.tasks),
					doneTasks: detail.tasks.done,
					totalTasks
				};
			}
			return {
				id: proj.id,
				name: proj.name,
				key: proj.key,
				role: proj.role,
				openWork: proj.openWork,
				healthLabel: 'Active',
				tone: 'accent',
				progressPct: null,
				progressText:
					proj.openWork === 0
						? 'No open tasks'
						: `${plural(proj.openWork, 'open task', 'open tasks')}`,
				doneTasks: 0,
				totalTasks: 0
			};
		});

	// Contributors table: shared scope reconciliation across open work + 28d completions, sorted A-Z.
	const memberById: Record<string, { id: string; name: string }> = {};
	const memberIdByLowerName: Record<string, string> = {};
	for (const m of input.detail.members) {
		memberById[m.id] = { id: m.id, name: m.name };
		memberIdByLowerName[m.name.trim().toLowerCase()] = m.id;
	}
	for (const [name, id] of Object.entries(input.memberIdByName ?? {})) {
		const key = name.trim().toLowerCase();
		if (!Object.hasOwn(memberIdByLowerName, key)) {
			memberIdByLowerName[key] = id;
		}
		if (!Object.hasOwn(memberById, id)) {
			memberById[id] = { id, name };
		}
	}

	const bucketMap: Record<string, TeamContributorRow> = {};
	function ensureContributor(memberId: string | undefined, name: string): TeamContributorRow {
		const trimmed = name.trim();
		const resolvedId = memberId ?? memberIdByLowerName[trimmed.toLowerCase()];
		const displayName =
			resolvedId && memberById[resolvedId] ? memberById[resolvedId].name : trimmed;
		const key = resolvedId ? `id:${resolvedId}` : `name:${displayName.toLowerCase()}`;
		if (!Object.hasOwn(bucketMap, key)) {
			bucketMap[key] = {
				key,
				memberId: resolvedId,
				name: displayName,
				completed: completionsAvailable ? 0 : null,
				open: 0,
				blocked: 0
			};
		}
		return bucketMap[key];
	}

	let unassignedOpen = 0;
	let unassignedBlocked = 0;
	for (const row of openRows) {
		const assignee = row.assignee?.trim();
		if (!assignee) {
			unassignedOpen += 1;
			if (row.state === 'blocked') unassignedBlocked += 1;
			continue;
		}
		const entry = ensureContributor(undefined, assignee);
		entry.open += 1;
		if (row.state === 'blocked') entry.blocked += 1;
	}

	let unattributedCompletions = 0;
	if (completionsAvailable) {
		for (const rec of inWindowCompletions) {
			const resolvedName =
				rec.contributorName?.trim() ||
				(rec.contributorId && memberById[rec.contributorId]?.name) ||
				'';
			if (!resolvedName && !rec.contributorId) {
				unattributedCompletions += 1;
				continue;
			}
			const entry = ensureContributor(rec.contributorId, resolvedName || rec.contributorId!);
			entry.completed = (entry.completed ?? 0) + 1;
		}
	}

	const contributors = Object.values(bucketMap)
		.filter((row) => row.open > 0 || (row.completed !== null && row.completed > 0))
		.toSorted((a, b) => a.name.localeCompare(b.name, 'en', { sensitivity: 'base' }));

	const recentActivity = sortActivityNewestFirst(input.activity ?? []).slice(0, 3);

	return {
		windowLabel,
		kpis,
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
		unattributedCompletions,
		recentActivity
	};
}
