import { unavailableCall } from '$lib/platform/unavailable-api';
import type { UpdatesOverview } from './types';

/**
 * What Organization settings › Updates reads. Read only: an upgrade is an explicit local command on the instance's
 * machine (server-installation-and-access-ux §5, SRV-15), and the browser offers no restart-to-update before a
 * supported distribution and update flow exists (app-shell-ux §0.1).
 */
export interface UpdatesApi {
	overview(organizationId: string): Promise<UpdatesOverview>;
}

// The web API does not serve this yet: every call refuses, and the screen says so.
// Under `bun run dev:mock`, e2e/fakes/modules/updates.ts stands in for this file.
export const updatesApi: UpdatesApi = {
	overview: unavailableCall
};
