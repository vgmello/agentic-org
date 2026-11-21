// What the Project Features screens read and change (features-ux §0–§3, FTR-1..14,
// organization-data 03 & 09). Lifecycle facts live in `projects/<project-id>/features/<feature-id>/feature.md`;
// task counts derive from task `feature_id`, never a stored feature tasks array.
import type { StaleCopy } from '$lib/modules/projects/types';

export type FeatureStatus = 'planned' | 'in_progress' | 'live' | 'retired';

export interface FeatureEmployeeRef {
	id: string;
	name: string;
	kind: 'person' | 'agent';
}

export interface FeatureProgress {
	done: number;
	open: number;
	blocked: number;
	/** Excluded from progress denominator (FTR-10). */
	cancelled: number;
}

export interface FeatureOriginLink {
	id: string;
	ref: string;
	title: string;
	unresolved?: boolean;
}

export interface FeatureTaskItem {
	id: string;
	ref: string;
	type: 'task' | 'bug' | 'request';
	title: string;
	status: 'todo' | 'in_progress' | 'blocked' | 'done' | 'cancelled';
	team: { id: string; name: string };
	assignedTeam: { id: string; name: string } | null;
	assignee: FeatureEmployeeRef | null;
	archived?: boolean;
}

export interface FeatureDocLink {
	id: string;
	key: string | null;
	kind: string;
	title: string;
	status: 'draft' | 'current' | 'deprecated';
}

export interface FeatureReleaseBacklink {
	releaseId: string;
	ref: string;
	title: string;
	versionLabel: string | null;
	status: string;
	productChangelogDocId?: string;
	productChangelogTitle?: string;
}

export interface FeatureRevision {
	revision: number;
	at: string;
	by: FeatureEmployeeRef;
	summary: string;
}

export type FeatureCostSummary =
	| {
			allowed: true;
			knownCostMicroUsd: number | null;
			knownTokens: number | null;
			complete: boolean;
			incompleteReason?: string;
			leadTimeLabel: string | null;
	  }
	| {
			allowed: false;
			reason: string;
	  };

export type FeatureProblem =
	| { kind: 'malformed_metadata'; field: string; message: string }
	| { kind: 'missing_file'; expectedPath: string; message: string };

export interface FeatureRow {
	id: string;
	projectId: string;
	projectNumber: number;
	projectSlug: string;
	/** Null while HQ is unreachable at creation: shown as "Pending number" and usable by stable ID (FTR-13). */
	number: number | null;
	localSlug: string;
	title: string;
	status: FeatureStatus;
	owner: FeatureEmployeeRef | null;
	workType: string;
	progress: FeatureProgress;
	origins: FeatureOriginLink[];
	deliveredAt: string | null;
	retiredReason: string | null;
	replacedBy: { id: string; ref: string; title: string } | null;
	pendingPublication?: boolean;
	updatedAt: string;
	updatedBy: FeatureEmployeeRef;
	problem?: FeatureProblem;
}

export interface FeatureDetail extends FeatureRow {
	revision: number;
	path: string;
	bodyMarkdown: string;
	createdAt: string;
	createdBy: FeatureEmployeeRef;
	/** Implementation tasks linked via task `feature_id` (grouped by current state). */
	tasks: FeatureTaskItem[];
	/** Bugs linked via `details.affects_feature_id`, kept strictly separate from implementation tasks (FTR-11). */
	affectedBugs: FeatureTaskItem[];
	supportingDocs: FeatureDocLink[];
	/** Explicit release manifests selecting this feature (FTR-6, FTR-11). */
	releases: FeatureReleaseBacklink[];
	cost: FeatureCostSummary;
	history: FeatureRevision[];
	canEdit: boolean;
	canLinkTasks: boolean;
	stale?: StaleCopy;
}

export interface EligibleTaskChoice {
	id: string;
	ref: string;
	type: 'task' | 'bug' | 'request';
	title: string;
	status: string;
	currentFeature: { id: string; ref: string; title: string } | null;
}

export interface BreakIntoTaskDraft {
	id: string;
	title: string;
	description: string;
	assignedTeamId: string | null;
	assigneeId: string | null;
	priority: 'urgent' | 'high' | 'normal' | 'low';
}

export interface ProjectFeaturesOverview {
	projectId: string;
	projectName: string;
	projectNumber: number;
	projectSlug: string;
	ownerTeam: { id: string; name: string };
	features: FeatureRow[];
	owners: FeatureEmployeeRef[];
	teams: { id: string; name: string }[];
	canEdit: boolean;
	stale?: StaleCopy;
}
