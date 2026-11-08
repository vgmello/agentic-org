import { invoke } from '@tauri-apps/api/core';
import type { Connection, HelloResult, Organization } from '$lib/proto';
import type { OfficeApi } from './api';

/** Every call goes through the Rust shell: the WebView has no network access of its own. */
export const tauriOffice: OfficeApi = {
	connections: () => invoke<Connection[]>('connections_list'),
	ensureRunning: () => invoke<HelloResult>('office_ensure_running'),
	createOrganization: (name) => invoke<Connection>('organization_create', { name }),
	listOrganizations: () => invoke<Organization[]>('organizations_list'),
	openOrganization: (id) => invoke<Connection>('organization_open', { id })
};
