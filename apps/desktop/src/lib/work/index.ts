import { orgApi } from '$lib/org/api';
import { newId } from '$lib/org/commands';
import type { WorkAccepted } from '$lib/proto';
import { createMockWork } from './api';
import type { WorkCall } from './engine';

/** The work API the screens use: the mock office's, until the real office serves `org_proto::work`. */
export const workApi = createMockWork(orgApi);

/** A call without its change id, which `sender` adds (one fresh ULID per call). */
export type WorkDraft = WorkCall extends infer C ? (C extends unknown ? Omit<C, 'change_id'> : never) : never;

export const sender =
	(organizationId: string) =>
	(draft: WorkDraft): Promise<WorkAccepted> =>
		workApi.command(organizationId, { ...draft, change_id: newId() } as WorkCall);
