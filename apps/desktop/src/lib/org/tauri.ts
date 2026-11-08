import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { CommandAccepted, OrgFeedNotice, OrgView } from '$lib/proto';
import type { OrgApi } from './api';
import { ready } from './commands';
import { normalizeKinds } from './model';

/** The structure comes from the Rust shell, which asks the office: the WebView has no network access. */
let queue: Promise<unknown> = Promise.resolve();

export const tauriOrg: OrgApi = {
	async view(organizationId) {
		const view = await invoke<OrgView>('org_snapshot', { organizationId });
		return { ...view, snapshot: normalizeKinds(view.snapshot) };
	},
	command(organizationId, input) {
		// One at a time, in the order sent: the shell runs each call on its own thread, which keeps no order,
		// so a command that depends on the one before it must not overtake it.
		const command = ready(input);
		const answer = queue.then(() => invoke<CommandAccepted>('org_command', { organizationId, command }));
		queue = answer.catch(() => undefined);
		return answer;
	},
	onFeed: (listener) => listen<OrgFeedNotice>('org-feed', (event) => listener(event.payload)),
	retryFeed: (organizationId) => invoke<void>('org_live_retry', { organizationId })
};
