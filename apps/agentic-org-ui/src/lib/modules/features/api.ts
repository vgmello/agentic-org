import { unavailableCall } from '$lib/platform/unavailable-api';
import type {
	BreakIntoTaskDraft,
	EligibleTaskChoice,
	FeatureDetail,
	FeatureStatus,
	ProjectFeaturesOverview
} from './types';

/**
 * What the Project Features screens read and change (features-ux §0–§3, FTR-1..14).
 * Every write is an explicit organization-Git operation against `expectedRevision`.
 */
export interface FeaturesApi {
	list(organizationId: string, projectId: string): Promise<ProjectFeaturesOverview>;
	get(organizationId: string, projectId: string, featureId: string): Promise<FeatureDetail>;
	create(
		organizationId: string,
		projectId: string,
		draft: { title: string; localSlug: string; ownerId: string | null; bodyMarkdown: string }
	): Promise<{ id: string }>;
	save(
		organizationId: string,
		projectId: string,
		featureId: string,
		change: {
			title: string;
			localSlug: string;
			ownerId: string | null;
			bodyMarkdown: string;
			expectedRevision: number;
		}
	): Promise<FeatureDetail>;
	setStatus(
		organizationId: string,
		projectId: string,
		featureId: string,
		change: {
			status: Exclude<FeatureStatus, 'retired'>;
			ownerId: string | null;
			expectedRevision: number;
		}
	): Promise<FeatureDetail>;
	retire(
		organizationId: string,
		projectId: string,
		featureId: string,
		change: { reason: string; replacedByFeatureId: string | null; expectedRevision: number }
	): Promise<FeatureDetail>;
	eligibleTasks(
		organizationId: string,
		projectId: string,
		featureId: string
	): Promise<EligibleTaskChoice[]>;
	linkTask(
		organizationId: string,
		projectId: string,
		featureId: string,
		change: { taskId: string; expectedRevision: number }
	): Promise<void>;
	unlinkTask(
		organizationId: string,
		projectId: string,
		featureId: string,
		change: { taskId: string }
	): Promise<void>;
	breakIntoTasks(
		organizationId: string,
		projectId: string,
		featureId: string,
		drafts: BreakIntoTaskDraft[]
	): Promise<{ created: { id: string; ref: string; title: string }[] }>;
	repair(organizationId: string, projectId: string, featureId: string): Promise<FeatureDetail>;
}

// Not served by the web API yet: production refuses cleanly with no mock fallback.
// Under `bun run dev:mock`, e2e/fakes/modules/features.ts stands in for this file.
export const featuresApi: FeaturesApi = {
	list: unavailableCall,
	get: unavailableCall,
	create: unavailableCall,
	save: unavailableCall,
	setStatus: unavailableCall,
	retire: unavailableCall,
	eligibleTasks: unavailableCall,
	linkTask: unavailableCall,
	unlinkTask: unavailableCall,
	breakIntoTasks: unavailableCall,
	repair: unavailableCall
};
