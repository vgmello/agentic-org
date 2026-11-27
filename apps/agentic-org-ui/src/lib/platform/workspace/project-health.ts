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
	due?: string;
	doneCount?: number;
	totalCount?: number;
	nowMs?: number;
}

/** Derive project health from snapshot work items, open Inbox items, teams, and optional due date. */
export function deriveProjectHealth(input: ProjectHealthInput): ProjectHealthResult {
	const projItems = input.workItems.filter(
		(item) => !item.deleted && item.project_id === input.projectId
	);
	const doneCount = input.doneCount ?? projItems.filter(isCompletedWorkSummary).length;
	const totalCount = input.totalCount ?? projItems.length;
	const blockedCount = projItems.filter((item) => item.status === 'blocked').length;
	const criticalBugs = projItems.filter(isSeriousOpenBug).length;
	const scopedReqsCount = projItems.filter(
		(item) => item.kind === 'request' && item.status === 'scoped'
	).length;
	const openInbox = input.inboxItems.filter(
		(item) => item.state === 'open' && item.project?.id === input.projectId
	);
	const problemCount = openInbox.filter((item) => item.kind === 'problem').length;
	const unassignedTeam =
		input.projectTeams !== undefined &&
		!input.projectTeams.some((pt) => pt.project_id === input.projectId);

	return classifyProjectHealth({
		criticalBugs,
		blockedCount,
		problemCount,
		isOverdue: isProjectOverdue(input.due, doneCount, totalCount, input.nowMs),
		scopedReqsCount,
		openInboxCount: openInbox.length,
		unassignedTeam
	});
}
