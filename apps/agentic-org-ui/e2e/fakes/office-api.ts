export { toAppError } from '../../src/lib/modules/offices/office-api';
export type { OfficeApi } from '../../src/lib/modules/offices/office-api';
import { createMockOffice } from '../../src/lib/modules/offices/mock';
export const officeApi = createMockOffice({ testHook: true });
