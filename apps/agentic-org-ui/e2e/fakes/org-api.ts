export type { OrgApi, MockStorage } from '../../src/lib/org/api';
export { createMockOrg } from '../../src/lib/org/api';
import { createMockOrg } from '../../src/lib/org/api';
import { officeApi } from '$lib/office/api';
import { isPocDataset, POC_HQ_OFFICE_ID } from './mock-mode';
import { pocOrgSnapshot } from './poc-dataset';

let failedOnce = false;
const scenario = () => new URLSearchParams(location.search).get('org');
const base = createMockOrg(
	async (organizationId) => {
		const connection = (await officeApi.connections()).find(
			(c) => c.organization.id === organizationId
		);
		if (!connection)
			throw { source: 'api', code: 'not_found', message: "That organization isn't here." };
		return connection.office.id;
	},
	{
		empty: () => scenario() === 'empty',
		fails: () =>
			scenario() === 'fails' || (scenario() === 'fails_once' && !failedOnce && (failedOnce = true)),
		delayMs: () => (scenario() === 'slow' ? 2500 : 0)
	}
);

export const orgApi: typeof base = {
	...base,
	async view(organizationId) {
		if (isPocDataset()) {
			return { snapshot: structuredClone(pocOrgSnapshot(POC_HQ_OFFICE_ID)), sample: true };
		}
		return base.view(organizationId);
	}
};
