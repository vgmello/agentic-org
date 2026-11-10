export * from '../../src/lib/office/api';
import { createMockOffice } from '../../src/lib/office/mock';
export const officeApi = createMockOffice({ testHook: true });
