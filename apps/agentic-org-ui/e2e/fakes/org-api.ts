export type { OrgApi, MockStorage } from '../../src/lib/org/api';
export { createMockOrg } from '../../src/lib/org/api';
import { createMockOrg } from '../../src/lib/org/api';
import { officeApi } from '$lib/office/api';

let failedOnce = false;
const scenario = () => new URLSearchParams(location.search).get('org');
export const orgApi = createMockOrg(async (organizationId) => {
	const connection = (await officeApi.connections()).find((c) => c.organization.id === organizationId);
	if (!connection) throw { source: 'api', code: 'not_found', message: "That organization isn't here." };
	return connection.office.id;
}, {
	empty: () => scenario() === 'empty',
	fails: () => scenario() === 'fails' || (scenario() === 'fails_once' && !failedOnce && (failedOnce = true)),
	delayMs: () => scenario() === 'slow' ? 2500 : 0
});
