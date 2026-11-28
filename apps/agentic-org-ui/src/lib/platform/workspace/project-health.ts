import type { ProjectTeam, WorkItemSummary } from '$lib/proto';
import type { InboxItem } from './inbox-types';

export type ProjectHealthTone = 'ok' | 'warn' | 'bad';
export type ProjectHealthLabel = 'On track' | 'At risk' | 'Off track';
export type ProjectStatusPillClass = 't-green' | 't-amber' | 't-red';

export interface ProjectHealthResult {
	tone: ProjectHealthTone;
	label: ProjectHealthLabel;
	statusPillClass: ProjectStatusPillClass;
}

export interface ProjectHealthSignals {
	criticalBugs: number;
	blockedCount: number;
	problemCount: number;
	isOverdue: boolean;
	scopedReqsCount: number;
	openInboxCount: number;
	unassignedTeam: boolean;
}

/**
 * Supplemental project health overlay combining persistent metadata (`dueDate`, `teamIds`)
 * with optional volatile work counts (`doneCount`, `totalCount`, `blockedCount`, `criticalBugCount`).
 */
export interface ProjectHealthOverlay {
	dueDate?: string;
	doneCount?: number;
	totalCount?: number;
	blockedCount?: number;
	criticalBugCount?: number;
	teamIds?: readonly string[];
}

export interface ProjectHealthOverlayInput {
	dueDate?: string;
	doneCount?: number;
	totalCount?: number;
	blockedCount?: number;
	criticalBugCount?: number;
	teamIds?: readonly string[];
}

export interface ProjectHealthOverlayUpdateOptions {
	preserveDueDate?: boolean;
	preserveTeamIds?: boolean;
}

export function isSeriousOpenBug(item: WorkItemSummary): boolean {
	return (
		item.kind === 'bug' &&
		item.status !== 'closed' &&
		(item.severity === 'critical' || item.severity === 'major')
	);
}

function isCompletedWorkSummary(item: WorkItemSummary): boolean {
	if (item.deleted) return false;
	return (
		(item.kind === 'task' && item.status === 'done') ||
		(item.kind === 'bug' && item.status === 'closed') ||
		(item.kind === 'request' && item.status === 'approved')
	);
}

function parseIsoDateUtcMs(iso: string): number | null {
	const match = /^(\d{4})-(\d{2})-(\d{2})/.exec(iso);
	if (!match) return null;
	const ms = Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3]), 12);
	return Number.isNaN(ms) ? null : ms;
}

function utcNoonMs(nowMs: number): number {
	const d = new Date(nowMs);
	return Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate(), 12);
}

/** True when a project has a past target date and incomplete work remaining. */
export function isProjectOverdue(
	dueDate: string | undefined,
	doneCount: number,
	totalCount: number,
	nowMs = Date.now()
): boolean {
	if (!dueDate || totalCount <= 0 || doneCount >= totalCount) return false;
	const dueMs = parseIsoDateUtcMs(dueDate);
	if (dueMs === null) return false;
	return dueMs < utcNoonMs(nowMs);
}

/**
 * Normalize and freeze a domain-neutral `ProjectHealthOverlay`.
 * Volatile counts are included only when meaningful (`totalCount > 0`, counts `> 0`);
 * `teamIds` preserves explicit emptiness (`[]`) when provided so unassigned checks stay honest.
 */
export function buildProjectHealthOverlay(input: ProjectHealthOverlayInput): ProjectHealthOverlay {
	const overlay: ProjectHealthOverlay = {};
	if (input.dueDate) overlay.dueDate = input.dueDate;
	if (input.totalCount !== undefined && input.totalCount > 0) {
		overlay.doneCount = input.doneCount ?? 0;
		overlay.totalCount = input.totalCount;
	}
	if (input.blockedCount !== undefined && input.blockedCount > 0) {
		overlay.blockedCount = input.blockedCount;
	}
	if (input.criticalBugCount !== undefined && input.criticalBugCount > 0) {
		overlay.criticalBugCount = input.criticalBugCount;
	}
	if (input.teamIds !== undefined) {
		overlay.teamIds = Object.freeze(
			input.teamIds.map((id) => id.trim()).filter((id) => id.length > 0)
		);
	}
	return Object.freeze(overlay);
}

/** Keep persistent project metadata (`dueDate`, `teamIds`) and drop volatile count fields. */
export function stripVolatileProjectHealthCounts(
	overlay: ProjectHealthOverlay
): ProjectHealthOverlay | undefined {
	const next: ProjectHealthOverlay = {};
	if (overlay.dueDate) next.dueDate = overlay.dueDate;
	if (overlay.teamIds !== undefined) next.teamIds = Object.freeze([...overlay.teamIds]);
	return next.dueDate !== undefined || next.teamIds !== undefined ? Object.freeze(next) : undefined;
}

/** Strip volatile count fields from an entire overlay map while preserving persistent metadata. */
export function stripVolatileOverlayMap(
	overlays: Readonly<Record<string, ProjectHealthOverlay>>
): Readonly<Record<string, ProjectHealthOverlay>> {
	const next: Record<string, ProjectHealthOverlay> = {};
	for (const [id, overlay] of Object.entries(overlays)) {
		const stripped = stripVolatileProjectHealthCounts(overlay);
		if (stripped) next[id] = stripped;
	}
	return Object.freeze(next);
}

function freezeOverlay(overlay: ProjectHealthOverlay): ProjectHealthOverlay {
	return Object.freeze({
		...overlay,
		...(overlay.teamIds !== undefined ? { teamIds: Object.freeze([...overlay.teamIds]) } : {})
	});
}

function mergeSingleOverlay(
	prev: ProjectHealthOverlay | undefined,
	incoming: ProjectHealthOverlay,
	options: ProjectHealthOverlayUpdateOptions | undefined
): ProjectHealthOverlay {
	const dueDate = incoming.dueDate ?? (options?.preserveDueDate ? prev?.dueDate : undefined);
	const teamIds = incoming.teamIds ?? (options?.preserveTeamIds ? prev?.teamIds : undefined);
	return freezeOverlay({
		...incoming,
		...(dueDate !== undefined ? { dueDate } : {}),
		...(teamIds !== undefined ? { teamIds } : {})
	});
}

function retainPreservedMetadata(
	prev: ProjectHealthOverlay,
	options: ProjectHealthOverlayUpdateOptions
): ProjectHealthOverlay | undefined {
	const next: ProjectHealthOverlay = {};
	if (options.preserveDueDate && prev.dueDate) next.dueDate = prev.dueDate;
	if (options.preserveTeamIds && prev.teamIds !== undefined) {
		next.teamIds = Object.freeze([...prev.teamIds]);
	}
	return next.dueDate !== undefined || next.teamIds !== undefined ? Object.freeze(next) : undefined;
}

/**
 * Merge incoming project health overlays into the previous map, optionally preserving
 * previously known `dueDate` and/or `teamIds` when a backing feed failed.
 */
export function mergeProjectHealthOverlayMap(
	previous: Readonly<Record<string, ProjectHealthOverlay>>,
	incoming: Readonly<Record<string, ProjectHealthOverlay>>,
	options?: ProjectHealthOverlayUpdateOptions
): Readonly<Record<string, ProjectHealthOverlay>> {
	const next: Record<string, ProjectHealthOverlay> = {};
	for (const [id, overlay] of Object.entries(incoming)) {
		next[id] = mergeSingleOverlay(previous[id], overlay, options);
	}
	if (options?.preserveDueDate || options?.preserveTeamIds) {
		for (const [id, prev] of Object.entries(previous)) {
			if (id in next) continue;
			const kept = retainPreservedMetadata(prev, options);
			if (kept) next[id] = kept;
		}
	}
	return Object.freeze(next);
}

/**
 * Single canonical project health classifier shared across shell and workspace surfaces
 * so every project badge displays identical health tones and labels.
 */
export function classifyProjectHealth(signals: ProjectHealthSignals): ProjectHealthResult {
	const offTrack =
		signals.criticalBugs > 0 ||
		signals.blockedCount >= 2 ||
		(signals.problemCount > 0 && signals.blockedCount > 0);
	if (offTrack) {
		return { tone: 'bad', label: 'Off track', statusPillClass: 't-red' };
	}
	const atRisk =
		signals.isOverdue ||
		signals.blockedCount > 0 ||
		signals.scopedReqsCount > 0 ||
		signals.openInboxCount >= 2 ||
		signals.unassignedTeam;
	if (atRisk) {
		return { tone: 'warn', label: 'At risk', statusPillClass: 't-amber' };
	}
	return { tone: 'ok', label: 'On track', statusPillClass: 't-green' };
}

export interface ProjectHealthInput {
	projectId: string;
	workItems: readonly WorkItemSummary[];
	inboxItems: readonly InboxItem[];
	projectTeams?: readonly ProjectTeam[];
	knownTeamIds?: ReadonlySet<string>;
	overlay?: ProjectHealthOverlay;
	dueDate?: string;
	doneCount?: number;
	totalCount?: number;
	nowMs?: number;
}

function hasValidTeamAssignment(
	projectId: string,
	projectTeams: readonly ProjectTeam[] | undefined,
	teamIds: readonly string[] | undefined,
	knownTeamIds: ReadonlySet<string> | undefined
): boolean {
	const isKnown = (teamId: string) =>
		teamId.trim().length > 0 && (!knownTeamIds || knownTeamIds.has(teamId));
	if (projectTeams?.some((pt) => pt.project_id === projectId && isKnown(pt.team_id))) {
		return true;
	}
	return teamIds?.some(isKnown) ?? false;
}

/**
 * Single canonical signal builder evaluating live work items, open Inbox items,
 * team assignment, and optional supplemental overlay using one consistent rule.
 */
export function buildProjectHealthSignals(input: ProjectHealthInput): ProjectHealthSignals {
	const projItems = input.workItems.filter(
		(item) => !item.deleted && item.project_id === input.projectId
	);
	const doneCount =
		input.overlay?.doneCount ?? input.doneCount ?? projItems.filter(isCompletedWorkSummary).length;
	const totalCount = input.overlay?.totalCount ?? input.totalCount ?? projItems.length;
	const workBlocked = projItems.filter((item) => item.status === 'blocked').length;
	const blockedCount = Math.max(workBlocked, input.overlay?.blockedCount ?? 0);
	const criticalBugs = Math.max(
		projItems.filter(isSeriousOpenBug).length,
		input.overlay?.criticalBugCount ?? 0
	);
	const scopedReqsCount = projItems.filter(
		(item) => item.kind === 'request' && item.status === 'scoped'
	).length;
	const openInbox = input.inboxItems.filter(
		(item) => item.state === 'open' && item.project?.id === input.projectId
	);
	const problemCount = openInbox.filter((item) => item.kind === 'problem').length;
	const canCheckTeam = input.projectTeams !== undefined || input.overlay?.teamIds !== undefined;
	const unassignedTeam =
		canCheckTeam &&
		!hasValidTeamAssignment(
			input.projectId,
			input.projectTeams,
			input.overlay?.teamIds,
			input.knownTeamIds
		);
	const dueDate = input.overlay?.dueDate ?? input.dueDate;

	return {
		criticalBugs,
		blockedCount,
		problemCount,
		isOverdue: isProjectOverdue(dueDate, doneCount, totalCount, input.nowMs),
		scopedReqsCount,
		openInboxCount: openInbox.length,
		unassignedTeam
	};
}

/** Derive project health from snapshot work items, open Inbox items, teams, and optional overlay. */
export function deriveProjectHealth(input: ProjectHealthInput): ProjectHealthResult {
	return classifyProjectHealth(buildProjectHealthSignals(input));
}
