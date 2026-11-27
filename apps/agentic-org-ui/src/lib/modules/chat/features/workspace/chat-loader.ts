import type { ChatSnapshot } from '$lib/modules/messaging/types';
import { toAppError } from '$lib/proto';
import type { ChatApi } from '../../api';

export interface ChatSnapshotLoader {
	load: () => Promise<boolean>;
	scheduleLiveReload: () => void;
	dispose: () => void;
}

/**
 * Monotonic, debounced snapshot loader for ChatWorkspace (P1-5, TASK-226).
 * Prevents out-of-order responses and post-unmount state writes.
 */
export function createChatSnapshotLoader(opts: {
	chatApi: ChatApi;
	getOrganizationId: () => string;
	hasSnapshot: () => boolean;
	onSnapshot: (snapshot: ChatSnapshot, nowIso: string) => void;
	onError: (message: string) => void;
	debounceMs?: number;
}): ChatSnapshotLoader {
	let destroyed = false;
	let loadGen = 0;
	let debounceTimer: ReturnType<typeof setTimeout> | undefined;
	let activeAbort: AbortController | undefined;

	async function load(): Promise<boolean> {
		if (destroyed) return false;
		activeAbort?.abort();
		const controller = new AbortController();
		activeAbort = controller;
		const gen = ++loadGen;
		const orgId = opts.getOrganizationId();
		try {
			const nowIso = new Date().toISOString();
			const snap = await opts.chatApi.snapshot(orgId);
			if (
				destroyed ||
				controller.signal.aborted ||
				gen !== loadGen ||
				orgId !== opts.getOrganizationId()
			)
				return false;
			opts.onSnapshot(snap, nowIso);
			return true;
		} catch (e) {
			if (
				destroyed ||
				controller.signal.aborted ||
				gen !== loadGen ||
				orgId !== opts.getOrganizationId()
			)
				return false;
			if (!opts.hasSnapshot()) opts.onError(toAppError(e).message);
			return false;
		} finally {
			if (activeAbort === controller) activeAbort = undefined;
		}
	}

	function scheduleLiveReload(): void {
		if (destroyed) return;
		clearTimeout(debounceTimer);
		debounceTimer = setTimeout(() => {
			debounceTimer = undefined;
			void load();
		}, opts.debounceMs ?? 150);
	}

	function dispose(): void {
		destroyed = true;
		loadGen++;
		activeAbort?.abort();
		activeAbort = undefined;
		clearTimeout(debounceTimer);
		debounceTimer = undefined;
	}

	return { load, scheduleLiveReload, dispose };
}
