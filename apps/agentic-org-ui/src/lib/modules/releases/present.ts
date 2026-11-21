// Pure presentation helpers for Project Releases (features-ux §2, organization-data 07 & 09).
import type {
	DeploymentOperation,
	DeploymentStatus,
	MilestoneStatus,
	ReleaseMilestone,
	ReleaseRow,
	ReleaseStatus
} from './types';

export const releasesHref = (projectId: string) =>
	`/projects/${encodeURIComponent(projectId)}/releases`;

export const releaseHref = (projectId: string, releaseId: string) =>
	`${releasesHref(projectId)}/${encodeURIComponent(releaseId)}`;

export const releaseRef = (r: Pick<ReleaseRow, 'number'>) => `R-${r.number}`;
export const milestoneRef = (m: Pick<ReleaseMilestone, 'number'>) => `M-${m.number}`;

export const RELEASE_STATUS_WORD: Record<ReleaseStatus, string> = {
	planned: 'Planned',
	in_progress: 'In progress',
	released: 'Released',
	cancelled: 'Cancelled'
};

export const RELEASE_STATUS_TONE: Record<ReleaseStatus, 'neutral' | 'info' | 'ok' | 'bad'> = {
	planned: 'neutral',
	in_progress: 'info',
	released: 'ok',
	cancelled: 'bad'
};

export const MILESTONE_STATUS_WORD: Record<MilestoneStatus, string> = {
	planned: 'Planned',
	in_progress: 'In progress',
	completed: 'Completed',
	cancelled: 'Cancelled'
};

export const DEPLOYMENT_OP_WORD: Record<DeploymentOperation, string> = {
	deploy: 'Deploy',
	rollback: 'Rollback'
};

export const DEPLOYMENT_STATUS_WORD: Record<DeploymentStatus, string> = {
	queued: 'Queued',
	running: 'Running',
	succeeded: 'Succeeded',
	failed: 'Failed',
	cancelled: 'Cancelled'
};

export const DEPLOYMENT_STATUS_TONE: Record<
	DeploymentStatus,
	'neutral' | 'info' | 'ok' | 'bad' | 'owner'
> = {
	queued: 'neutral',
	running: 'info',
	succeeded: 'ok',
	failed: 'bad',
	cancelled: 'neutral'
};

export function milestoneProgressText(m: ReleaseMilestone): string {
	const eligible = m.tasksDone + m.tasksOpen;
	const base =
		eligible === 0
			? 'No tasks yet'
			: `${m.tasksDone} of ${eligible} ${eligible === 1 ? 'task' : 'tasks'} done`;
	return m.tasksCancelled > 0 ? `${base} · ${m.tasksCancelled} cancelled` : base;
}

export function matchesReleaseFilter(
	row: ReleaseRow,
	status: 'all' | ReleaseStatus,
	query: string
): boolean {
	if (status !== 'all' && row.status !== status) return false;
	const q = query.trim().toLowerCase();
	if (!q) return true;
	return [releaseRef(row), row.versionLabel ?? '', row.title, row.id].some((s) =>
		s.toLowerCase().includes(q)
	);
}

export function shortSha(sha: string): string {
	return sha.length > 10 ? sha.slice(0, 10) : sha;
}
