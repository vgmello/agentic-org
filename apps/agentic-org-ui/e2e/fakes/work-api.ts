export { workSender } from '../../src/lib/modules/work/index';
export type { WorkDraft } from '../../src/lib/modules/work/index';
import { workSender } from '../../src/lib/modules/work/index';
import { createMockWork } from '../../src/lib/modules/work/mock';
import { orgApi } from '$lib/modules/organization/api';

export const workApi = createMockWork(orgApi);
export const sender = workSender(workApi);
if (typeof window !== 'undefined')
	(window as unknown as { __mockWorkAs: typeof workApi.commandAs }).__mockWorkAs =
		workApi.commandAs;
