import { officeApi } from '$lib/office/api';
import type { OrgSnapshot } from '$lib/proto';
import { normalizeKinds } from './model';
import { emptyOrg, sampleOrg } from './sample';

/** What the org chart needs from an office: `org.snapshot` (changes and commands come with editing). */
export interface OrgApi {
	/** True while the data is the TEMPORARY invented sample, not the organization's own. */
	readonly sample: boolean;
	snapshot(): Promise<OrgSnapshot>;
}

/**
 * A mock office's organization. `?org=empty` in the URL shows an organization with only its owner.
 * TEMPORARY: until the real host and `org-client` are wired in (the Rust shell then answers
 * `org.snapshot`), the browser and the app both use this TypeScript mock for organization data.
 */
export function createMockOrg(hqOfficeId: () => Promise<string>, empty: () => boolean = () => false, fails: () => boolean = () => false, delayMs: () => number = () => 0): OrgApi {
	return {
		sample: true,
		async snapshot() {
			if (delayMs() > 0) await new Promise((resolve) => setTimeout(resolve, delayMs()));
			if (fails()) throw { source: 'api', code: 'storage_error', message: "The office couldn't read its storage." };
			return normalizeKinds(empty() ? emptyOrg() : sampleOrg(await hqOfficeId()));
		}
	};
}

const wantsEmpty = () =>
	typeof location !== 'undefined' && new URLSearchParams(location.search).get('org') === 'empty';
/**
 * `?org=fails` makes reading the structure fail, `?org=fails_once` only the first time (so Retry can be shown to
 * work), `?org=slow` makes it take a moment: to show how a screen copes.
 */
let failedOnce = false;
const wantsFailure = () => {
	if (typeof location === 'undefined') return false;
	const mode = new URLSearchParams(location.search).get('org');
	if (mode === 'fails_once') return !failedOnce && (failedOnce = true);
	return mode === 'fails';
};
const wantsSlow = () => (typeof location !== 'undefined' && new URLSearchParams(location.search).get('org') === 'slow' ? 2500 : 0);

export const orgApi: OrgApi = createMockOrg(
	async () => (await officeApi.connections()).at(-1)?.office.id ?? 'office-local',
	wantsEmpty,
	wantsFailure,
	wantsSlow
);
