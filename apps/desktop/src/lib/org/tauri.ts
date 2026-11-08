import { invoke } from '@tauri-apps/api/core';
import type { OrgView } from '$lib/proto';
import type { OrgApi } from './api';
import { ready } from './commands';
import { normalizeKinds } from './model';

/** The structure comes from the Rust shell, which asks the office: the WebView has no network access. */
export const tauriOrg: OrgApi = {
	async view(organizationId) {
		const view = await invoke<OrgView>('org_snapshot', { organizationId });
		return { ...view, snapshot: normalizeKinds(view.snapshot) };
	},
	command(organizationId, input) {
		return invoke('org_command', { organizationId, command: ready(input) });
	}
};
