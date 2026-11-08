import { inTauri, officeApi } from '$lib/office/api';
import type { AppError, OrgView } from '$lib/proto';
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
export function createMockOrg(hqOfficeId: (organizationId: string) => Promise<string>, empty: () => boolean = () => false): OrgApi {
	return {
		async view(organizationId) {
			return { snapshot: normalizeKinds(empty() ? emptyOrg() : sampleOrg(await hqOfficeId(organizationId))), sample: true };
		}
	};
}

const wantsEmpty = () =>
	typeof location !== 'undefined' && new URLSearchParams(location.search).get('org') === 'empty';

export const orgApi: OrgApi = inTauri()
	? tauriOrg
	: createMockOrg(async (organizationId) => {
			const connection = (await officeApi.connections()).find((c) => c.organization.id === organizationId);
			if (!connection) throw { source: 'api', code: 'not_found', message: "That organization isn't here." } satisfies AppError;
			return connection.office.id;
		}, wantsEmpty);
