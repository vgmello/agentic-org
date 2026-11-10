/**
 * Temporary composition boundary (D-F14 / D-F14a): preserve the existing browser mock unchanged.
 * Jules's F1-02 per-feature factory cutover replaces this with the platform factory / unavailable default.
 * Route callers (the only allowed callers outside this module):
 * - src/routes/(shell)/chat/+page.svelte
 * Keep this file out of the public index.
 */
import { orgApi } from '$lib/org/api';
import { newId } from '$lib/org/commands';
import { createMockChat } from './api';
import type { ChatAccepted, ChatCall } from './types';

/**
 * The chat API the Chat screen uses. Today the browser mock in every build: the office does not serve chat yet, and
 * the Rust shell has nothing to call. When it does, this picks the Tauri implementation like `workApi` does.
 */
export const chatApi = createMockChat(orgApi);

/** A call without its change id, which `sender` adds (one fresh ULID per call). */
export type ChatDraft = ChatCall extends infer C ? (C extends unknown ? Omit<C, 'change_id'> : never) : never;

export const sender =
	(organizationId: string) =>
	(draft: ChatDraft): Promise<ChatAccepted> =>
		chatApi.command(organizationId, { ...draft, change_id: newId() } as ChatCall);
