import { officeApi } from '$lib/office/api';
import type { OrgSnapshot } from '$lib/proto';
import { emptyOrg, sampleOrg } from './sample';

/** What the org chart needs from an office: `org.snapshot` (changes and commands come with editing). */
export interface OrgApi {
	snapshot(): Promise<OrgSnapshot>;
}

/**
 * A mock office's organization. `?org=empty` in the URL shows an organization with only its owner.
 * TEMPORARY: until the real host and `org-client` are wired in (the Rust shell then answers
 * `org.snapshot`), the browser and the app both use this TypeScript mock for organization data.
 */
export function createMockOrg(hqOfficeId: () => Promise<string>, empty: () => boolean = () => false): OrgApi {
	return {
		async snapshot() {
			return empty() ? emptyOrg() : sampleOrg(await hqOfficeId());
		}
	};
}

const wantsEmpty = () =>
	typeof location !== 'undefined' && new URLSearchParams(location.search).get('org') === 'empty';

export const orgApi: OrgApi = createMockOrg(
	async () => (await officeApi.connections()).at(-1)?.office.id ?? 'office-local',
	wantsEmpty
);
