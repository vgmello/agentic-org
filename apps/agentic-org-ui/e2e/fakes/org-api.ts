export type { OrgApi } from '../../src/lib/modules/organization/api';
export type { MockStorage } from '../../src/lib/modules/organization/mock';
export { createMockOrg } from '../../src/lib/modules/organization/mock';
import { createMockOrg } from '../../src/lib/modules/organization/mock';
import { officeApi } from '$lib/modules/offices/office-api';
import { POC_HQ_OFFICE_ID, POC_ORG_ID } from './mock-mode';
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
		if (organizationId === POC_ORG_ID) {
			return { snapshot: structuredClone(pocOrgSnapshot(POC_HQ_OFFICE_ID)), sample: true };
		}
		return base.view(organizationId);
	}
};
