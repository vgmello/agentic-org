// What the Project Releases screens read and change (features-ux §2, organization-data 07 & 09).
// Keeps product release knowledge (manifest, product changelog, intended flag settings) strictly separate
// from technical deployment attempts (actual commits, confirmed applied flag snapshots, technical changelog).
import type { StaleCopy } from '$lib/modules/projects/types';

export type ReleaseStatus = 'planned' | 'in_progress' | 'released' | 'cancelled';
export type MilestoneStatus = 'planned' | 'in_progress' | 'completed' | 'cancelled';
export type DeploymentOperation = 'deploy' | 'rollback';
export type DeploymentStatus = 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled';

export interface ReleaseEmployeeRef {
	id: string;
	name: string;
	kind: 'person' | 'agent';
}

export interface ReleaseMilestone {
	id: string;
	number: number;
	title: string;
	status: MilestoneStatus;
	tasksDone: number;
	tasksOpen: number;
	tasksCancelled: number;
}

export interface ReleaseFeatureItem {
	id: string;
	ref: string;
	title: string;
	status: string;
}

export interface ReleaseTaskItem {
	id: string;
	ref: string;
	type: 'task' | 'bug' | 'request';
	title: string;
	status: string;
	milestoneRef?: string;
	archived?: boolean;
}

/** Intended setting in `flag-settings.yaml`, separate from confirmed deployment snapshots (organization-data 09). */
export interface ReleaseFlagSetting {
	flagId: string;
	key: string;
	valueType: 'boolean' | 'string' | 'number' | 'json';
	environment: string;
	target: string;
	intendedValue: string;
	rolloutSummary?: string;
	featureRef?: string;
}

/** Confirmed applied flag snapshot captured on a deployment attempt (organization-data 09). */
export interface DeploymentAppliedFlag {
	flagId: string;
	key: string;
	appliedValue: string;
	rolloutSummary?: string;
	appliedAt: string | null;
}

export interface ReleaseDeployment {
	id: string;
	environment: string;
	target: string;
	operation: DeploymentOperation;
	rollbackOfId: string | null;
	baselineDeploymentId: string | null;
	status: DeploymentStatus;
	startedAt: string | null;
	finishedAt: string | null;
	externalUrl: string | null;
	summary: string;
	repositoryCommits: { repositoryKey: string; commitSha: string }[];
	appliedFlags: DeploymentAppliedFlag[];
	technicalChangelogDocId?: string;
	technicalChangelogTitle?: string;
	actor: ReleaseEmployeeRef;
}

export interface ReleaseRow {
	id: string;
	projectId: string;
	projectNumber: number;
	number: number;
	versionLabel: string | null;
	title: string;
	status: ReleaseStatus;
	owner: ReleaseEmployeeRef | null;
	milestoneCount: number;
	featureCount: number;
	taskCount: number;
	deploymentCount: number;
	latestDeploymentSummary?: string;
	updatedAt: string;
}

export interface ReleaseDetail extends ReleaseRow {
	revision: number;
	path: string;
	bodyMarkdown: string;
	createdAt: string;
	createdBy: ReleaseEmployeeRef;
	updatedBy: ReleaseEmployeeRef;
	milestones: ReleaseMilestone[];
	features: ReleaseFeatureItem[];
	tasks: ReleaseTaskItem[];
	flagSettings: ReleaseFlagSetting[];
	deployments: ReleaseDeployment[];
	productChangelogDocId?: string;
	productChangelogTitle?: string;
	supportingDocs: { id: string; key: string | null; kind: string; title: string; status: string }[];
	canEdit: boolean;
	stale?: StaleCopy;
}

export interface ProjectReleasesOverview {
	projectId: string;
	projectName: string;
	projectNumber: number;
	releases: ReleaseRow[];
	canEdit: boolean;
	stale?: StaleCopy;
}
