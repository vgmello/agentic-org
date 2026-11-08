import { invoke } from '@tauri-apps/api/core';
import type { OrgView } from '$lib/proto';
import type { OrgApi } from './api';
import { normalizeKinds } from './model';

/** The structure comes from the Rust shell, which asks the office: the WebView has no network access. */
export const tauriOrg: OrgApi = {
	async view(organizationId) {
		const view = await invoke<OrgView>('org_snapshot', { organizationId });
		return { ...view, snapshot: normalizeKinds(view.snapshot) };
	}
};
