export { toAppError } from '../../src/lib/proto';
export type { OfficeApi } from '../../src/lib/modules/offices/office-api';
import { createMockOffice } from '../../src/lib/modules/offices/mock';
import { ensurePocConnection, isPocDataset } from './mock-mode';

const base = createMockOffice({ testHook: true });

export const officeApi: typeof base = {
	...base,
	async connections() {
		if (isPocDataset()) ensurePocConnection();
		return base.connections();
	},
	async peekOrganizations() {
		if (isPocDataset()) ensurePocConnection();
		return base.peekOrganizations();
	},
	async listOrganizations() {
		if (isPocDataset()) ensurePocConnection();
		return base.listOrganizations();
	}
};
