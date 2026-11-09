import { inTauri } from '$lib/office/api';
import { orgApi } from '$lib/org/api';
import { newId } from '$lib/org/commands';
import type { WorkAccepted } from '$lib/proto';
import { createMockWork, type MockWorkApi } from './api';
import { tauriWork } from './tauri';
import type { WorkCall } from './engine';

/** The work API the screens use: the shell's mock office in the app, the browser mock in a plain browser, until the real office serves `org_proto::work`. */
const mock: MockWorkApi | undefined = inTauri() ? undefined : createMockWork(orgApi);
export const workApi = mock ?? tauriWork;

// A browser test acts as another member, as an agent would: `window.__mockWorkAs(organizationId, memberId, call)`.
if (mock && typeof window !== 'undefined') (window as unknown as { __mockWorkAs: MockWorkApi['commandAs'] }).__mockWorkAs = mock.commandAs;

/** A call without its change id, which `sender` adds (one fresh ULID per call). */
export type WorkDraft = WorkCall extends infer C ? (C extends unknown ? Omit<C, 'change_id'> : never) : never;

export const sender =
	(organizationId: string) =>
	(draft: WorkDraft): Promise<WorkAccepted> =>
		workApi.command(organizationId, { ...draft, change_id: newId() } as WorkCall);
