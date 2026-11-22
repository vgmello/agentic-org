export { toAppError } from '../../src/lib/office/api';
export type { OfficeApi } from '../../src/lib/office/api';
import { createMockOffice } from '../../src/lib/office/mock';
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
