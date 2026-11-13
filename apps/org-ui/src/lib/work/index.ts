import { newId } from '$lib/org/commands';
import type { WorkAccepted } from '$lib/proto';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { WorkApi } from './api';
import type { WorkCall } from './engine';

export const workApi: WorkApi = Object.freeze({
	snapshot: unavailableCall, get: unavailableCall, command: unavailableCall, activity: unavailableCall, similar: unavailableCall,
	requests: false
});

export type WorkDraft = WorkCall extends infer C ? (C extends unknown ? Omit<C, 'change_id'> : never) : never;

/** Bind one explicit API; test composition uses the same sender without a production fallback. */
export const workSender = (api: WorkApi) => (organizationId: string) => (draft: WorkDraft): Promise<WorkAccepted> =>
	api.command(organizationId, { ...draft, change_id: newId() } as WorkCall);

export const sender = workSender(workApi);
