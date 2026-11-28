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
 * Supplemental server-side project health signals hydrated from `projectsApi.list` (`ProjectRow`)
 * and `homeApi.projects` (`HomeProject`) so Executive Home and sidebar favorites share identical inputs.
 */
export interface ProjectHealthOverlay {
	due?: string;
	doneCount?: number;
	totalCount?: number;
	blockedCount?: number;
	criticalBugs?: number;
	rowTeamIds?: readonly string[];
}

export interface ProjectHealthOverlaySource {
	due?: string;
	homeDone?: number;
	homeTotal?: number;
	homeBlocked?: number;
	rowTasks?: { todo: number; inProgress: number; blocked: number; done: number };
	rowBugs?: { critical: number; major: number };
	rowTeamIds?: readonly string[];
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
	due: string | undefined,
	doneCount: number,
	totalCount: number,
	nowMs = Date.now()
): boolean {
	if (!due || totalCount <= 0 || doneCount >= totalCount) return false;
	const dueMs = parseIsoDateUtcMs(due);
	if (dueMs === null) return false;
	return dueMs < utcNoonMs(nowMs);
}

function resolveOverlayCounts(
	source: ProjectHealthOverlaySource
): { doneCount: number; totalCount: number } | undefined {
	if (source.homeTotal !== undefined && source.homeTotal > 0) {
		return { doneCount: source.homeDone ?? 0, totalCount: source.homeTotal };
	}
	if (!source.rowTasks) return undefined;
	const rowTotal =
		source.rowTasks.todo +
		source.rowTasks.inProgress +
		source.rowTasks.blocked +
		source.rowTasks.done;
	return rowTotal > 0 ? { doneCount: source.rowTasks.done, totalCount: rowTotal } : undefined;
}

/**
 * Build a normalized, frozen `ProjectHealthOverlay` from optional `HomeProject` and `ProjectRow` data.
 * Used identically by Executive Home cards and `ShellData` sidebar sync.
 */
export function buildProjectHealthOverlay(
	source: ProjectHealthOverlaySource
): ProjectHealthOverlay {
	const overlay: ProjectHealthOverlay = {};
	if (source.due) overlay.due = source.due;
	const counts = resolveOverlayCounts(source);
	if (counts) {
		overlay.doneCount = counts.doneCount;
		overlay.totalCount = counts.totalCount;
	}
	const blocked = Math.max(source.rowTasks?.blocked ?? 0, source.homeBlocked ?? 0);
	if (blocked > 0) overlay.blockedCount = blocked;
	if (source.rowBugs) {
		const serious = source.rowBugs.critical + source.rowBugs.major;
		if (serious > 0) overlay.criticalBugs = serious;
	}
	if (source.rowTeamIds !== undefined) {
		overlay.rowTeamIds = Object.freeze(
			source.rowTeamIds.map((id) => id.trim()).filter((id) => id.length > 0)
		);
	}
	return Object.freeze(overlay);
}

/**
 * Single canonical project health classifier shared by Executive Home scorecards and
 * sidebar Favorite projects so both surfaces always show identical health tones and labels.
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
	due?: string;
	doneCount?: number;
	totalCount?: number;
	nowMs?: number;
}

function hasValidTeamAssignment(
	projectId: string,
	projectTeams: readonly ProjectTeam[] | undefined,
	rowTeamIds: readonly string[] | undefined,
	knownTeamIds: ReadonlySet<string> | undefined
): boolean {
	const isKnown = (teamId: string) =>
		teamId.trim().length > 0 && (!knownTeamIds || knownTeamIds.has(teamId));
	if (projectTeams?.some((pt) => pt.project_id === projectId && isKnown(pt.team_id))) {
		return true;
	}
	return rowTeamIds?.some(isKnown) ?? false;
}

/**
 * Single canonical signal builder shared by Executive Home scorecards and sidebar Favorite projects.
 * Evaluates work items, open Inbox items, due dates, server-side row overlays, and team assignment
 * using one consistent rule.
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
		input.overlay?.criticalBugs ?? 0
	);
	const scopedReqsCount = projItems.filter(
		(item) => item.kind === 'request' && item.status === 'scoped'
	).length;
	const openInbox = input.inboxItems.filter(
		(item) => item.state === 'open' && item.project?.id === input.projectId
	);
	const problemCount = openInbox.filter((item) => item.kind === 'problem').length;
	const canCheckTeam = input.projectTeams !== undefined || input.overlay?.rowTeamIds !== undefined;
	const unassignedTeam =
		canCheckTeam &&
		!hasValidTeamAssignment(
			input.projectId,
			input.projectTeams,
			input.overlay?.rowTeamIds,
			input.knownTeamIds
		);
	const due = input.overlay?.due ?? input.due;

	return {
		criticalBugs,
		blockedCount,
		problemCount,
		isOverdue: isProjectOverdue(due, doneCount, totalCount, input.nowMs),
		scopedReqsCount,
		openInboxCount: openInbox.length,
		unassignedTeam
	};
}

/** Derive project health from snapshot work items, open Inbox items, teams, and optional overlay. */
export function deriveProjectHealth(input: ProjectHealthInput): ProjectHealthResult {
	return classifyProjectHealth(buildProjectHealthSignals(input));
}
