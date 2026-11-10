export * from '../../src/lib/work/index';
import { workSender } from '../../src/lib/work/index';
import { createMockWork } from '../../src/lib/work/api';
import { orgApi } from '$lib/org/api';
export const workApi = createMockWork(orgApi);
export const sender = workSender(workApi);
if (typeof window !== 'undefined') (window as unknown as { __mockWorkAs: typeof workApi.commandAs }).__mockWorkAs = workApi.commandAs;
