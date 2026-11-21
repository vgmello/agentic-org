import { unavailableCall } from '$lib/platform/unavailable-api';
import type { ProjectReleasesOverview, ReleaseDetail } from './types';

/**
 * What the Project Releases screens read and change (features-ux §2, organization-data 07 & 09).
 */
export interface ReleasesApi {
	list(organizationId: string, projectId: string): Promise<ProjectReleasesOverview>;
	get(organizationId: string, projectId: string, releaseId: string): Promise<ReleaseDetail>;
	create(
		organizationId: string,
		projectId: string,
		draft: { title: string; versionLabel: string | null; bodyMarkdown: string }
	): Promise<{ id: string }>;
}

// Not served by the web API yet: production fails closed with zero mock fallback.
// Under `bun run dev:mock`, e2e/fakes/modules/releases.ts stands in for this file.
export const releasesApi: ReleasesApi = {
	list: unavailableCall,
	get: unavailableCall,
	create: unavailableCall
};
