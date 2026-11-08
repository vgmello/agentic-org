import { inTauri, officeApi } from '$lib/office/api';
import type { OrgView } from '$lib/proto';
import { normalizeKinds } from './model';
import { emptyOrg, sampleOrg } from './sample';
import { tauriOrg } from './tauri';

/** What the org chart needs from an office: `org.snapshot` (changes and commands come with editing). */
export interface OrgApi {
	/** The structure of an organization, and whether it is the TEMPORARY invented sample. */
	view(organizationId: string): Promise<OrgView>;
}

/**
 * A mock office's organization, for a plain browser (`bun run dev`, tests): the Tauri app asks the Rust shell
 * instead. `?org=empty` in the URL shows an organization with only its owner. Always sample data.
 */
export function createMockOrg(hqOfficeId: () => Promise<string>, empty: () => boolean = () => false): OrgApi {
	return {
		async view() {
			return { snapshot: normalizeKinds(empty() ? emptyOrg() : sampleOrg(await hqOfficeId())), sample: true };
		}
	};
}

const wantsEmpty = () =>
	typeof location !== 'undefined' && new URLSearchParams(location.search).get('org') === 'empty';

export const orgApi: OrgApi = inTauri()
	? tauriOrg
	: createMockOrg(async () => (await officeApi.connections()).at(-1)?.office.id ?? 'office-local', wantsEmpty);
