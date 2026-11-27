import type { ProjectTeam, WorkItemSummary } from '$lib/proto';
import type { InboxItem } from '$lib/platform/workspace/inbox-types';

export type ProjectHealthTone = 'ok' | 'warn' | 'bad';

export interface ProjectHealthBadge {
	tone: ProjectHealthTone;
	label: 'On track' | 'At risk' | 'Off track';
}

function isSeriousOpenBug(item: WorkItemSummary): boolean {
	return (
		item.kind === 'bug' &&
		item.status !== 'closed' &&
		(item.severity === 'critical' || item.severity === 'major')
	);
}

/**
 * Pure project health classification for sidebar favorite project icons.
 * Mirrors Home scorecard signals (`classifyProjectHealth`) without importing domain modules.
 */
export function deriveProjectHealth(
	projectId: string,
	workItems: readonly WorkItemSummary[],
	inboxItems: readonly InboxItem[],
	projectTeams?: readonly ProjectTeam[]
): ProjectHealthBadge {
	const projItems = workItems.filter((item) => !item.deleted && item.project_id === projectId);
	const blockedCount = projItems.filter((item) => item.status === 'blocked').length;
	const criticalBugs = projItems.filter(isSeriousOpenBug).length;
	const scopedReqsCount = projItems.filter(
		(item) => item.kind === 'request' && item.status === 'scoped'
	).length;

	const openInbox = inboxItems.filter(
		(item) => item.state === 'open' && item.project?.id === projectId
	);
	const problemCount = openInbox.filter((item) => item.kind === 'problem').length;
	const unassignedTeam =
		projectTeams !== undefined && !projectTeams.some((pt) => pt.project_id === projectId);

	if (criticalBugs > 0 || blockedCount >= 2 || (problemCount > 0 && blockedCount > 0)) {
		return { tone: 'bad', label: 'Off track' };
	}
	if (blockedCount > 0 || scopedReqsCount > 0 || openInbox.length >= 2 || unassignedTeam) {
		return { tone: 'warn', label: 'At risk' };
	}
	return { tone: 'ok', label: 'On track' };
}
